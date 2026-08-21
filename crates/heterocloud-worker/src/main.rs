use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use clap::Parser;
use heterocloud_domain::{OrganizationId, PrincipalId, ProjectId, ServiceInstanceId};
use heterocloud_provider::{
    AcceptedOperation, PRINCIPAL_CONTEXT_REVOCATION_GRACE_SECONDS, PRINCIPAL_CONTEXT_REVOKE_ACTION,
    PrincipalContextId, PrincipalContextRevocationRequest, ProviderContext, ProviderSigner,
    ReconcileRequest,
};
use heterocloud_store::{OutboxEvent, Store};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;
use tokio::{fs, signal, time};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;
use url::Url;

#[derive(Debug, Parser)]
#[command(version, about = "HeteroCloud provider outbox worker")]
struct Config {
    #[arg(long, env = "HETEROCLOUD_DATABASE_URL_FILE")]
    database_url_file: PathBuf,

    #[arg(long, env = "HETEROCLOUD_PROVIDER_SIGNING_KEY_FILE")]
    signing_key_file: PathBuf,

    #[arg(long, env = "HETEROCLOUD_FLOW_ENDPOINT")]
    flow_endpoint: Url,

    #[arg(long, env = "HETEROCLOUD_FLASH_ENDPOINT")]
    flash_endpoint: Url,

    #[arg(
        long,
        env = "HETEROCLOUD_PROVIDER_ISSUER",
        default_value = "heterocloud"
    )]
    issuer: String,

    #[arg(
        long,
        env = "HETEROCLOUD_FLOW_AUDIENCE",
        default_value = "heterocloud-flow"
    )]
    flow_audience: String,

    #[arg(
        long,
        env = "HETEROCLOUD_FLASH_AUDIENCE",
        default_value = "heterocloud-flash"
    )]
    flash_audience: String,

    #[arg(
        long,
        env = "HETEROCLOUD_PROVIDER_KEY_ID",
        default_value = "heterocloud-provider-1"
    )]
    key_id: String,

    #[arg(
        long,
        env = "HETEROCLOUD_WORKER_POLL_MILLISECONDS",
        default_value_t = 500
    )]
    poll_milliseconds: u64,

    #[arg(
        long,
        env = "HETEROCLOUD_DATABASE_MAX_CONNECTIONS",
        default_value_t = 10
    )]
    database_max_connections: u32,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    install_crypto_provider()?;
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("heterocloud_worker=info")),
        )
        .json()
        .init();
    let config = Config::parse();
    let database_url = read_secret(&config.database_url_file).await?;
    let signing_key = read_secret_bytes(&config.signing_key_file).await?;
    let providers = ProviderTargets {
        flow: ProviderTarget {
            endpoint: config.flow_endpoint.clone(),
            signer: ProviderSigner::from_ed25519_pem(
                &config.issuer,
                &config.flow_audience,
                &config.key_id,
                &signing_key,
            )?,
        },
        flash: ProviderTarget {
            endpoint: config.flash_endpoint.clone(),
            signer: ProviderSigner::from_ed25519_pem(
                &config.issuer,
                &config.flash_audience,
                &config.key_id,
                &signing_key,
            )?,
        },
    };
    let store = Store::connect(
        database_url.expose_secret(),
        config.database_max_connections,
    )
    .await?;
    store.migrate().await?;
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .build()?;
    let poll = Duration::from_millis(config.poll_milliseconds.clamp(100, 30_000));
    info!(
        flow_endpoint = %config.flow_endpoint,
        flash_endpoint = %config.flash_endpoint,
        "provider worker is ready"
    );

    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            () = &mut shutdown => {
                info!("provider worker is stopping");
                break;
            }
            () = time::sleep(poll) => {
                if let Err(error) = process_one(
                    &store,
                    &client,
                    &providers,
                ).await {
                    error!(error = %error, "provider event processing failed");
                }
            }
        }
    }
    Ok(())
}

fn install_crypto_provider() -> Result<(), std::io::Error> {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        rustls::crypto::ring::default_provider()
            .install_default()
            .map_err(|_| std::io::Error::other("failed to install the Rustls Ring provider"))?;
    }
    Ok(())
}

async fn process_one(
    store: &Store,
    client: &reqwest::Client,
    providers: &ProviderTargets,
) -> Result<(), WorkerError> {
    let Some(event) = store.claim_outbox_event().await? else {
        return Ok(());
    };
    match deliver(store, client, providers, &event).await {
        Ok(()) => store.mark_outbox_delivered(event.id).await?,
        Err(WorkerError::StalePayload) => {
            warn!(
                event_id = %event.id,
                "stale provider event was superseded by a newer generation"
            );
            store.mark_outbox_delivered(event.id).await?;
        }
        Err(error) => {
            warn!(
                event_id = %event.id,
                attempts = event.attempts,
                error = %error,
                "provider delivery will be retried"
            );
            store.retry_outbox_event(event.id, event.attempts).await?;
        }
    }
    Ok(())
}

async fn deliver(
    store: &Store,
    client: &reqwest::Client,
    providers: &ProviderTargets,
    event: &OutboxEvent,
) -> Result<(), WorkerError> {
    if event.topic == PRINCIPAL_CONTEXT_REVOKE_ACTION {
        let payload: PrincipalContextRevocationPayload =
            serde_json::from_value(event.payload.clone())?;
        return deliver_principal_context_revocation(
            client,
            &providers.flow.signer,
            &providers.flow.endpoint,
            event,
            payload,
        )
        .await;
    }
    if !matches!(
        event.topic.as_str(),
        "service-instance.reconcile" | "service-instance.delete"
    ) {
        return Err(WorkerError::UnsupportedTopic(event.topic.clone()));
    }
    let payload: ReconcilePayload = serde_json::from_value(event.payload.clone())?;
    if payload.service_instance_id.0 != event.aggregate_id {
        return Err(WorkerError::InvalidPayload);
    }
    let target = providers.target(&payload.provider)?;
    let instance = match store.service_instance(payload.service_instance_id).await? {
        Some(instance) => instance,
        None if event.topic == "service-instance.delete" => return Ok(()),
        None => return Err(WorkerError::MissingInstance),
    };
    if instance.generation != payload.generation
        || instance.organization_id != payload.organization_id
        || instance.project_id != payload.project_id
        || instance.provider != payload.provider
    {
        return Err(WorkerError::StalePayload);
    }
    let signed = target.signer.sign(ProviderContext {
        principal_id: payload.principal_id,
        organization_id: payload.organization_id,
        project_id: payload.project_id,
        service_instance_id: payload.service_instance_id,
        action: event.topic.clone(),
        generation: payload.generation,
    })?;
    let mut url = target.endpoint.join(&format!(
        "internal/v1/service-instances/{}",
        payload.service_instance_id
    ))?;
    if event.topic == "service-instance.delete" {
        url.query_pairs_mut()
            .append_pair("generation", &instance.generation.to_string());
    }
    let request = client
        .request(
            if event.topic == "service-instance.delete" {
                reqwest::Method::DELETE
            } else {
                reqwest::Method::PUT
            },
            url,
        )
        .bearer_auth(signed.token)
        .header("idempotency-key", signed.claims.jwt_id.to_string());
    let response = if event.topic == "service-instance.delete" {
        request.send().await?
    } else {
        request
            .json(&ReconcileRequest {
                generation: instance.generation,
                name: instance.name,
                spec: instance.spec,
            })
            .send()
            .await?
    };
    if !response.status().is_success() {
        return Err(WorkerError::ProviderStatus(response.status().as_u16()));
    }
    let operation: AcceptedOperation = response.json().await?;
    if event.topic == "service-instance.delete" {
        if !store
            .complete_delete_service_instance(
                payload.service_instance_id,
                &payload.provider,
                payload.generation,
            )
            .await?
        {
            return Err(WorkerError::StalePayload);
        }
        info!(
            service_instance_id = %payload.service_instance_id,
            provider = payload.provider,
            operation_id = %operation.operation_id,
            "service instance deleted"
        );
        return Ok(());
    }
    if provider_reconcile_failed(&operation.status) {
        if !store
            .mark_service_instance_error(
                payload.service_instance_id,
                &payload.provider,
                payload.generation,
                operation.operation_id,
                operation.status,
            )
            .await?
        {
            return Err(WorkerError::StalePayload);
        }
        return Err(WorkerError::ProviderReconcileFailed);
    }
    if !store
        .mark_service_instance_ready(
            payload.service_instance_id,
            &payload.provider,
            payload.generation,
            operation.operation_id,
            operation.status,
        )
        .await?
    {
        return Err(WorkerError::StalePayload);
    }
    Ok(())
}

fn provider_reconcile_failed(status: &Value) -> bool {
    status.get("phase").and_then(Value::as_str) == Some("error")
}

struct ProviderTarget {
    endpoint: Url,
    signer: ProviderSigner,
}

struct ProviderTargets {
    flow: ProviderTarget,
    flash: ProviderTarget,
}

impl ProviderTargets {
    fn target(&self, provider: &str) -> Result<&ProviderTarget, WorkerError> {
        match provider {
            "flow" => Ok(&self.flow),
            "flash" => Ok(&self.flash),
            other => Err(WorkerError::UnsupportedProvider(other.to_owned())),
        }
    }
}

async fn deliver_principal_context_revocation(
    client: &reqwest::Client,
    signer: &ProviderSigner,
    flow_endpoint: &Url,
    event: &OutboxEvent,
    payload: PrincipalContextRevocationPayload,
) -> Result<(), WorkerError> {
    if payload.provider != "flow"
        || payload.context_id != event.aggregate_id
        || payload.generation <= 0
    {
        return Err(WorkerError::InvalidPayload);
    }
    let now = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| WorkerError::InvalidClock)?
            .as_secs(),
    )
    .map_err(|_| WorkerError::InvalidClock)?;
    if principal_context_revocation_expired(payload.expires_at, now) {
        info!(
            context_id = %payload.context_id,
            expires_at = payload.expires_at,
            "expired principal context revocation was dropped"
        );
        return Ok(());
    }
    let signed = signer.sign(ProviderContext {
        principal_id: payload.principal_id,
        organization_id: payload.organization_id,
        project_id: payload.project_id,
        service_instance_id: payload.service_instance_id,
        action: PRINCIPAL_CONTEXT_REVOKE_ACTION.to_owned(),
        generation: payload.generation,
    })?;
    let response = client
        .put(principal_context_revocation_url(
            flow_endpoint,
            payload.service_instance_id,
            payload.context_id,
        ))
        .bearer_auth(signed.token)
        .header("idempotency-key", event.id.to_string())
        .json(&PrincipalContextRevocationRequest {
            expires_at: payload.expires_at,
        })
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(WorkerError::ProviderStatus(response.status().as_u16()));
    }
    Ok(())
}

fn principal_context_revocation_expired(expires_at: i64, now: i64) -> bool {
    expires_at.saturating_add(PRINCIPAL_CONTEXT_REVOCATION_GRACE_SECONDS) <= now
}

fn principal_context_revocation_url(
    flow_endpoint: &Url,
    service_instance_id: ServiceInstanceId,
    context_id: PrincipalContextId,
) -> Url {
    let mut url = flow_endpoint.clone();
    url.set_path(&format!(
        "/internal/v1/service-instances/{service_instance_id}/principal-contexts/{context_id}/revocation"
    ));
    url.set_query(None);
    url.set_fragment(None);
    url
}

#[derive(Deserialize)]
struct ReconcilePayload {
    service_instance_id: ServiceInstanceId,
    organization_id: OrganizationId,
    project_id: ProjectId,
    principal_id: PrincipalId,
    provider: String,
    generation: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PrincipalContextRevocationPayload {
    context_id: PrincipalContextId,
    service_instance_id: ServiceInstanceId,
    organization_id: OrganizationId,
    project_id: ProjectId,
    principal_id: PrincipalId,
    provider: String,
    generation: i64,
    expires_at: i64,
}

async fn read_secret(path: &Path) -> Result<SecretString, WorkerError> {
    let bytes = read_secret_bytes(path).await?;
    let value = String::from_utf8(bytes).map_err(|_| WorkerError::InvalidSecret)?;
    let value = value.trim_end_matches(['\r', '\n']);
    if value.is_empty() {
        return Err(WorkerError::InvalidSecret);
    }
    Ok(SecretString::from(value.to_owned()))
}

async fn read_secret_bytes(path: &Path) -> Result<Vec<u8>, WorkerError> {
    let metadata = fs::metadata(path).await?;
    if !metadata.file_type().is_file() {
        return Err(WorkerError::InvalidSecret);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.mode() & 0o037 != 0 {
            return Err(WorkerError::UnsafeSecretPermissions);
        }
    }
    let bytes = fs::read(path).await?;
    if bytes.is_empty() {
        return Err(WorkerError::InvalidSecret);
    }
    Ok(bytes)
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _result = signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match signal::unix::signal(signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}

#[derive(Debug, Error)]
enum WorkerError {
    #[error("system clock cannot be represented as Unix seconds")]
    InvalidClock,
    #[error("secret file is invalid")]
    InvalidSecret,
    #[error("provider payload is invalid")]
    InvalidPayload,
    #[error("service instance no longer exists")]
    MissingInstance,
    #[error(transparent)]
    Provider(#[from] heterocloud_provider::ProviderError),
    #[error("provider returned HTTP {0}")]
    ProviderStatus(u16),
    #[error("provider reported a service reconciliation failure")]
    ProviderReconcileFailed,
    #[error("provider payload is stale")]
    StalePayload,
    #[error(transparent)]
    Store(#[from] heterocloud_store::StoreError),
    #[error("secret file permissions are too broad")]
    UnsafeSecretPermissions,
    #[error("unsupported outbox topic: {0}")]
    UnsupportedTopic(String),
    #[error("unsupported provider: {0}")]
    UnsupportedProvider(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Request(#[from] reqwest::Error),
    #[error(transparent)]
    Url(#[from] url::ParseError),
}

#[cfg(test)]
mod tests {
    use heterocloud_domain::{OrganizationId, PrincipalId, ProjectId, ServiceInstanceId};
    use heterocloud_provider::{
        PRINCIPAL_CONTEXT_REVOKE_ACTION, PrincipalContextId, ProviderContext, ProviderSigner,
    };
    use heterocloud_store::OutboxEvent;
    use serde_json::json;
    use url::Url;

    use super::{
        PrincipalContextRevocationPayload, ProviderTarget, ProviderTargets, WorkerError,
        deliver_principal_context_revocation, principal_context_revocation_expired,
        principal_context_revocation_url, provider_reconcile_failed,
    };

    const TEST_ED25519_PRIVATE_KEY: &[u8] = b"-----BEGIN PRIVATE KEY-----\n\
MC4CAQAwBQYDK2VwBCIEIG45L/crBYvUcHKXo1ZbNr3YBSD3wPhsGq7IKyuU2+ei\n\
-----END PRIVATE KEY-----\n";

    #[test]
    fn service_events_select_provider_specific_endpoint_and_audience()
    -> Result<(), Box<dyn std::error::Error>> {
        let targets = ProviderTargets {
            flow: ProviderTarget {
                endpoint: Url::parse("http://flow.example.test/")?,
                signer: ProviderSigner::from_ed25519_pem(
                    "heterocloud",
                    "heterocloud-flow",
                    "test-key",
                    TEST_ED25519_PRIVATE_KEY,
                )?,
            },
            flash: ProviderTarget {
                endpoint: Url::parse("http://flash.example.test/")?,
                signer: ProviderSigner::from_ed25519_pem(
                    "heterocloud",
                    "heterocloud-flash",
                    "test-key",
                    TEST_ED25519_PRIVATE_KEY,
                )?,
            },
        };
        let flow = targets.target("flow")?;
        let flash = targets.target("flash")?;
        assert_eq!(flow.endpoint.as_str(), "http://flow.example.test/");
        assert_eq!(flash.endpoint.as_str(), "http://flash.example.test/");
        let context = || ProviderContext {
            principal_id: PrincipalId(PrincipalContextId::from_u128(1)),
            organization_id: OrganizationId(PrincipalContextId::from_u128(2)),
            project_id: ProjectId(PrincipalContextId::from_u128(3)),
            service_instance_id: ServiceInstanceId(PrincipalContextId::from_u128(4)),
            action: "service-instance.reconcile".into(),
            generation: 1,
        };
        assert_eq!(
            flow.signer.sign(context())?.claims.audience,
            "heterocloud-flow"
        );
        assert_eq!(
            flash.signer.sign(context())?.claims.audience,
            "heterocloud-flash"
        );
        assert!(matches!(
            targets.target("unknown"),
            Err(WorkerError::UnsupportedProvider(provider)) if provider == "unknown"
        ));
        Ok(())
    }

    #[test]
    fn revocation_uses_exact_provider_action_and_rooted_path()
    -> Result<(), Box<dyn std::error::Error>> {
        let service_id = ServiceInstanceId(PrincipalContextId::from_u128(1));
        let context_id = PrincipalContextId::from_u128(2);
        let base = Url::parse("https://flow.example.test/stale/path?query=1#fragment")?;
        assert_eq!(PRINCIPAL_CONTEXT_REVOKE_ACTION, "principal-context.revoke");
        assert_eq!(
            principal_context_revocation_url(&base, service_id, context_id).as_str(),
            format!(
                "https://flow.example.test/internal/v1/service-instances/{service_id}/principal-contexts/{context_id}/revocation"
            )
        );
        Ok(())
    }

    #[test]
    fn revocation_outbox_payload_is_bounded_to_the_command_contract()
    -> Result<(), Box<dyn std::error::Error>> {
        let payload = json!({
            "context_id": PrincipalContextId::from_u128(1),
            "service_instance_id": PrincipalContextId::from_u128(2),
            "organization_id": PrincipalContextId::from_u128(3),
            "project_id": PrincipalContextId::from_u128(4),
            "principal_id": PrincipalContextId::from_u128(5),
            "provider": "flow",
            "generation": 7,
            "expires_at": 1_785_480_300_i64,
        });
        let decoded: PrincipalContextRevocationPayload = serde_json::from_value(payload.clone())?;
        assert_eq!(decoded.context_id, PrincipalContextId::from_u128(1));
        assert_eq!(decoded.expires_at, 1_785_480_300);
        let mut unknown = payload;
        unknown["credential"] = json!("must-never-enter-the-outbox");
        assert!(serde_json::from_value::<PrincipalContextRevocationPayload>(unknown).is_err());
        Ok(())
    }

    #[test]
    fn revocation_delivery_covers_flow_clock_skew() {
        assert!(!principal_context_revocation_expired(100, 100));
        assert!(!principal_context_revocation_expired(100, 114));
        assert!(principal_context_revocation_expired(100, 115));
    }

    #[test]
    fn only_an_explicit_provider_error_phase_is_a_reconcile_failure() {
        assert!(provider_reconcile_failed(&json!({
            "phase": "error",
            "message": "container image cannot start"
        })));
        assert!(!provider_reconcile_failed(
            &json!({"phase": "provisioning"})
        ));
        assert!(!provider_reconcile_failed(&json!({"phase": "ready"})));
        assert!(!provider_reconcile_failed(&json!({})));
    }

    #[tokio::test]
    async fn expired_revocation_succeeds_without_contacting_flow()
    -> Result<(), Box<dyn std::error::Error>> {
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            rustls::crypto::ring::default_provider()
                .install_default()
                .map_err(|_| "failed to install the Rustls Ring provider")?;
        }
        let signer = ProviderSigner::from_ed25519_pem(
            "heterocloud",
            "heterocloud-flow",
            "test-key",
            TEST_ED25519_PRIVATE_KEY,
        )?;
        let client = reqwest::Client::builder()
            .tls_certs_only(Vec::<reqwest::tls::Certificate>::new())
            .connect_timeout(std::time::Duration::from_millis(10))
            .build()?;
        let context_id = PrincipalContextId::from_u128(1);
        let event = OutboxEvent {
            id: PrincipalContextId::from_u128(2),
            topic: PRINCIPAL_CONTEXT_REVOKE_ACTION.into(),
            aggregate_id: context_id,
            payload: json!({}),
            attempts: 1,
        };
        deliver_principal_context_revocation(
            &client,
            &signer,
            &Url::parse("http://127.0.0.1:1/")?,
            &event,
            PrincipalContextRevocationPayload {
                context_id,
                service_instance_id: ServiceInstanceId(PrincipalContextId::from_u128(3)),
                organization_id: OrganizationId(PrincipalContextId::from_u128(4)),
                project_id: ProjectId(PrincipalContextId::from_u128(5)),
                principal_id: PrincipalId(PrincipalContextId::from_u128(6)),
                provider: "flow".into(),
                generation: 1,
                expires_at: 1,
            },
        )
        .await?;
        Ok(())
    }
}
