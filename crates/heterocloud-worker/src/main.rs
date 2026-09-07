use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use clap::Parser;
use heterocloud_domain::{
    OrganizationId, PrincipalId, ProjectId, ServiceInstance, ServiceInstanceId, SyouyuSpec,
};
use heterocloud_provider::{
    AcceptedOperation, PRINCIPAL_CONTEXT_REVOCATION_GRACE_SECONDS, PRINCIPAL_CONTEXT_REVOKE_ACTION,
    PrincipalContextId, PrincipalContextRevocationRequest, ProviderContext, ProviderSigner,
    ReconcileRequest,
};
use heterocloud_store::{OutboxEvent, Store};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio::{fs, signal, time};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;
use url::Url;

const MAX_PROVIDER_ERROR_BODY_BYTES: usize = 8 * 1024;
const MAX_PROVIDER_ERROR_MESSAGE_CHARS: usize = 1_024;

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

    #[arg(long, env = "HETEROCLOUD_SYOUYU_ENDPOINT")]
    syouyu_endpoint: Url,

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
        env = "HETEROCLOUD_SYOUYU_AUDIENCE",
        default_value = "heterocloud-syouyu"
    )]
    syouyu_audience: String,

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
        syouyu: ProviderTarget {
            endpoint: config.syouyu_endpoint.clone(),
            signer: ProviderSigner::from_ed25519_pem(
                &config.issuer,
                &config.syouyu_audience,
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
        syouyu_endpoint = %config.syouyu_endpoint,
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
    if !service_instance_matches_payload(&instance, &payload) {
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
    let mut response = if event.topic == "service-instance.delete" {
        request.send().await?
    } else {
        let spec = provider_reconcile_spec(&payload.provider, instance.spec)?;
        request
            .json(&ReconcileRequest {
                generation: instance.generation,
                name: instance.name,
                spec,
            })
            .send()
            .await?
    };
    let provider_status_code = response.status().as_u16();
    let already_absent = provider_delete_already_absent(&event.topic, &mut response).await?;
    if event.topic == "service-instance.delete"
        && (response.status().is_success() || already_absent)
    {
        let operation_id = if already_absent {
            None
        } else {
            let operation: AcceptedOperation = response.json().await?;
            Some(operation.operation_id)
        };
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
            operation_id = ?operation_id,
            already_absent,
            "service instance deleted"
        );
        return Ok(());
    }
    if !response.status().is_success() {
        if event.topic == "service-instance.reconcile"
            && permanent_reconcile_rejection(provider_status_code)
        {
            let provider_message = bounded_provider_error_message(response).await?;
            let message = provider_message.unwrap_or_else(|| {
                format!(
                    "{} provider rejected the requested configuration with HTTP {}",
                    payload.provider, provider_status_code
                )
            });
            if !store
                .mark_service_instance_error(
                    payload.service_instance_id,
                    &payload.provider,
                    payload.generation,
                    event.id,
                    serde_json::json!({
                        "phase": "error",
                        "message": message,
                        "provider_http_status": provider_status_code,
                    }),
                )
                .await?
            {
                return Err(WorkerError::StalePayload);
            }
            warn!(
                service_instance_id = %payload.service_instance_id,
                provider = payload.provider,
                status = provider_status_code,
                "provider permanently rejected service reconciliation"
            );
            return Ok(());
        }
        return Err(WorkerError::ProviderStatus(provider_status_code));
    }
    let operation: AcceptedOperation = response.json().await?;
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
        warn!(
            service_instance_id = %payload.service_instance_id,
            provider = payload.provider,
            "provider reported a permanent service reconciliation failure"
        );
        return Ok(());
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

#[derive(Serialize)]
struct SyouyuProviderSpec {
    region: String,
    bucket_name: String,
    quota_bytes: u64,
    quota_objects: u64,
}

fn provider_reconcile_spec(provider: &str, spec: Value) -> Result<Value, WorkerError> {
    if provider != "syouyu" {
        return Ok(spec);
    }
    let spec: SyouyuSpec = serde_json::from_value(spec)?;
    serde_json::to_value(SyouyuProviderSpec {
        region: spec.region,
        bucket_name: spec.bucket_name,
        quota_bytes: spec.quota_bytes,
        quota_objects: spec.quota_objects,
    })
    .map_err(WorkerError::from)
}

fn provider_reconcile_failed(status: &Value) -> bool {
    status.get("phase").and_then(Value::as_str) == Some("error")
}

fn service_instance_matches_payload(
    instance: &ServiceInstance,
    payload: &ReconcilePayload,
) -> bool {
    instance.generation == payload.generation
        && instance.organization_id == payload.organization_id
        && instance.project_id == payload.project_id
        && instance.provider == payload.provider
}

const fn permanent_reconcile_rejection(status: u16) -> bool {
    matches!(status, 400 | 409 | 422)
}

#[derive(Deserialize)]
struct ProviderErrorEnvelope {
    error: ProviderErrorBody,
}

#[derive(Deserialize)]
struct ProviderErrorBody {
    code: String,
    message: String,
}

async fn provider_delete_already_absent(
    topic: &str,
    response: &mut reqwest::Response,
) -> Result<bool, reqwest::Error> {
    if topic != "service-instance.delete"
        || response.status() != reqwest::StatusCode::NOT_FOUND
        || !response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))
    {
        return Ok(false);
    }

    // A truncated prefix must never authorize local deletion; require the complete envelope.
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if chunk.len() > MAX_PROVIDER_ERROR_BODY_BYTES - body.len() {
            return Ok(false);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(
        serde_json::from_slice::<ProviderErrorEnvelope>(&body).is_ok_and(|envelope| {
            envelope.error.code == "not_found" && !envelope.error.message.trim().is_empty()
        }),
    )
}

async fn bounded_provider_error_message(
    mut response: reqwest::Response,
) -> Result<Option<String>, reqwest::Error> {
    let mut body = Vec::new();
    while body.len() < MAX_PROVIDER_ERROR_BODY_BYTES {
        let Some(chunk) = response.chunk().await? else {
            break;
        };
        let remaining = MAX_PROVIDER_ERROR_BODY_BYTES - body.len();
        body.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
    }
    let message = serde_json::from_slice::<Value>(&body)
        .ok()
        .and_then(|value| {
            value
                .pointer("/error/message")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|message| !message.is_empty())
                .map(|message| {
                    message
                        .chars()
                        .filter(|character| !character.is_control())
                        .take(MAX_PROVIDER_ERROR_MESSAGE_CHARS)
                        .collect::<String>()
                })
        })
        .filter(|message| !message.is_empty());
    Ok(message)
}

struct ProviderTarget {
    endpoint: Url,
    signer: ProviderSigner,
}

struct ProviderTargets {
    flow: ProviderTarget,
    flash: ProviderTarget,
    syouyu: ProviderTarget,
}

impl ProviderTargets {
    fn target(&self, provider: &str) -> Result<&ProviderTarget, WorkerError> {
        match provider {
            "flow" => Ok(&self.flow),
            "flash" => Ok(&self.flash),
            "syouyu" => Ok(&self.syouyu),
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
    use std::io::{Read, Write};

    use heterocloud_domain::{OrganizationId, PrincipalId, ProjectId, ServiceInstanceId};
    use heterocloud_provider::{
        PRINCIPAL_CONTEXT_REVOKE_ACTION, PrincipalContextId, ProviderContext, ProviderSigner,
    };
    use heterocloud_store::OutboxEvent;
    use serde_json::json;
    use url::Url;

    use super::{
        MAX_PROVIDER_ERROR_BODY_BYTES, PrincipalContextRevocationPayload, ProviderTarget,
        ProviderTargets, ReconcilePayload, ServiceInstance, WorkerError,
        deliver_principal_context_revocation, permanent_reconcile_rejection,
        principal_context_revocation_expired, principal_context_revocation_url,
        provider_delete_already_absent, provider_reconcile_failed, provider_reconcile_spec,
        service_instance_matches_payload,
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
            syouyu: ProviderTarget {
                endpoint: Url::parse("http://syouyu.example.test/")?,
                signer: ProviderSigner::from_ed25519_pem(
                    "heterocloud",
                    "heterocloud-syouyu",
                    "test-key",
                    TEST_ED25519_PRIVATE_KEY,
                )?,
            },
        };
        let flow = targets.target("flow")?;
        let flash = targets.target("flash")?;
        let syouyu = targets.target("syouyu")?;
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
        assert_eq!(
            syouyu.signer.sign(context())?.claims.audience,
            "heterocloud-syouyu"
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

    #[test]
    fn permanent_provider_rejections_do_not_retry_forever() {
        assert!(permanent_reconcile_rejection(400));
        assert!(permanent_reconcile_rejection(409));
        assert!(permanent_reconcile_rejection(422));
        assert!(!permanent_reconcile_rejection(401));
        assert!(!permanent_reconcile_rejection(404));
        assert!(!permanent_reconcile_rejection(429));
        assert!(!permanent_reconcile_rejection(500));
    }

    async fn provider_error_response(
        status: u16,
        content_type: Option<&str>,
        body: &[u8],
        declared_length: Option<usize>,
    ) -> Result<reqwest::Response, Box<dyn std::error::Error>> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .no_proxy()
            .tls_certs_only(Vec::<reqwest::tls::Certificate>::new())
            .timeout(std::time::Duration::from_secs(3))
            .build()?;
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let mut wire = format!(
            "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n",
            declared_length.unwrap_or(body.len())
        );
        if let Some(content_type) = content_type {
            wire.push_str(&format!("Content-Type: {content_type}\r\n"));
        }
        wire.push_str("\r\n");
        let mut wire = wire.into_bytes();
        wire.extend_from_slice(body);
        let server = std::thread::spawn(move || -> std::io::Result<()> {
            let (mut stream, _) = listener.accept()?;
            stream.set_read_timeout(Some(std::time::Duration::from_secs(3)))?;
            stream.set_write_timeout(Some(std::time::Duration::from_secs(3)))?;
            let mut request = [0; 4096];
            let mut length = 0;
            loop {
                let count = stream.read(&mut request[length..])?;
                if count == 0 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "incomplete fixture request headers",
                    ));
                }
                length += count;
                if request[..length]
                    .windows(4)
                    .any(|bytes| bytes == b"\r\n\r\n")
                {
                    break;
                }
                if length == request.len() {
                    return Err(std::io::Error::other("fixture request headers too large"));
                }
            }
            stream.write_all(&wire)
        });
        let response = client
            .delete(format!(
                "http://{address}/internal/v1/service-instances/test?generation=3"
            ))
            .send()
            .await;
        server.join().map_err(|_| "provider fixture failed")??;
        Ok(response?)
    }

    const PROVIDER_NOT_FOUND: &[u8] =
        br#"{"error":{"code":"not_found","message":"resource was not found"}}"#;

    #[tokio::test]
    async fn delete_accepts_typed_provider_not_found() -> Result<(), Box<dyn std::error::Error>> {
        for content_type in ["application/json", "application/json; charset=utf-8"] {
            let mut response =
                provider_error_response(404, Some(content_type), PROVIDER_NOT_FOUND, None).await?;
            assert!(
                provider_delete_already_absent("service-instance.delete", &mut response).await?
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn only_delete_404_can_be_already_absent() -> Result<(), Box<dyn std::error::Error>> {
        for (topic, status) in [
            ("service-instance.reconcile", 404),
            (PRINCIPAL_CONTEXT_REVOKE_ACTION, 404),
            ("unknown", 404),
            ("service-instance.delete", 200),
            ("service-instance.delete", 401),
            ("service-instance.delete", 403),
            ("service-instance.delete", 409),
            ("service-instance.delete", 502),
        ] {
            let mut response =
                provider_error_response(status, Some("application/json"), PROVIDER_NOT_FOUND, None)
                    .await?;
            assert!(!provider_delete_already_absent(topic, &mut response).await?);
            assert_eq!(response.bytes().await?.as_ref(), PROVIDER_NOT_FOUND);
        }
        Ok(())
    }

    #[tokio::test]
    async fn delete_rejects_proxy_auth_and_untyped_not_found()
    -> Result<(), Box<dyn std::error::Error>> {
        for body in [
            "",
            "<html>404 Not Found</html>",
            r#"{"error":{"code":"invalid_credentials","message":"not found"}}"#,
            r#"{"error":{"code":"permission_denied","message":"not found"}}"#,
            r#"{"error":{"code":"route_not_found","message":"not found"}}"#,
            r#"{"error":{"code":"stale_generation","message":"not found"}}"#,
            r#"{"error":{"code":404,"message":"not_found"}}"#,
            r#"{"code":"not_found","message":"resource was not found"}"#,
            r#"{"error":{"message":"not_found"}}"#,
            r#"{"error":{"code":"not_found"}}"#,
            r#"{"error":{"code":"not_found","message":" "}}"#,
            r#"{"error":{"code":"not_found","message":false}}"#,
            r#"{"error":{"code":"not_found","message":"missing"}"#,
        ] {
            let mut response =
                provider_error_response(404, Some("application/json"), body.as_bytes(), None)
                    .await?;
            assert!(
                !provider_delete_already_absent("service-instance.delete", &mut response).await?,
                "unexpected acknowledgement for {body}"
            );
        }
        for content_type in [None, Some("text/html"), Some("text/plain")] {
            let mut response =
                provider_error_response(404, content_type, PROVIDER_NOT_FOUND, None).await?;
            assert!(
                !provider_delete_already_absent("service-instance.delete", &mut response).await?
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn delete_requires_a_complete_bounded_error_envelope()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut body = PROVIDER_NOT_FOUND.to_vec();
        body.resize(MAX_PROVIDER_ERROR_BODY_BYTES, b' ');
        let mut response =
            provider_error_response(404, Some("application/json"), &body, None).await?;
        assert!(provider_delete_already_absent("service-instance.delete", &mut response).await?);

        body.push(b' ');
        let mut response =
            provider_error_response(404, Some("application/json"), &body, None).await?;
        assert!(!provider_delete_already_absent("service-instance.delete", &mut response).await?);

        let mut response = provider_error_response(
            404,
            Some("application/json"),
            PROVIDER_NOT_FOUND,
            Some(PROVIDER_NOT_FOUND.len() + 1),
        )
        .await?;
        assert!(
            provider_delete_already_absent("service-instance.delete", &mut response)
                .await
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn delete_payload_still_requires_current_generation_and_scope()
    -> Result<(), Box<dyn std::error::Error>> {
        let id = PrincipalContextId::from_u128(1);
        let organization_id = PrincipalContextId::from_u128(2);
        let project_id = PrincipalContextId::from_u128(3);
        let instance: ServiceInstance = serde_json::from_value(json!({
            "id": id,
            "organization_id": organization_id,
            "project_id": project_id,
            "provider": "syouyu",
            "name": "audit",
            "generation": 3,
            "state": "deleting",
            "spec": {},
            "status": {},
            "created_at": "2026-09-07T00:00:00Z",
            "updated_at": "2026-09-07T00:00:00Z",
        }))?;
        let mut payload: ReconcilePayload = serde_json::from_value(json!({
            "service_instance_id": id,
            "organization_id": organization_id,
            "project_id": project_id,
            "principal_id": PrincipalContextId::from_u128(4),
            "provider": "syouyu",
            "generation": 3,
        }))?;
        assert!(service_instance_matches_payload(&instance, &payload));
        for generation in [0, 2, 4] {
            payload.generation = generation;
            assert!(!service_instance_matches_payload(&instance, &payload));
        }
        payload.generation = 3;
        payload.provider = "flash".into();
        assert!(!service_instance_matches_payload(&instance, &payload));
        payload.provider = "syouyu".into();
        payload.organization_id = OrganizationId(PrincipalContextId::from_u128(5));
        assert!(!service_instance_matches_payload(&instance, &payload));
        payload.organization_id = OrganizationId(organization_id);
        payload.project_id = ProjectId(PrincipalContextId::from_u128(6));
        assert!(!service_instance_matches_payload(&instance, &payload));
        Ok(())
    }

    #[test]
    fn syouyu_provider_payload_excludes_management_metadata()
    -> Result<(), Box<dyn std::error::Error>> {
        let converted = provider_reconcile_spec(
            "syouyu",
            json!({
                "region": "heteronet-global",
                "bucket_name": "escape-persistent",
                "quota_bytes": 10_737_418_240_u64,
                "quota_objects": 1_000_000,
                "metadata": {"flash_service": "escape"},
            }),
        )?;
        assert_eq!(
            converted,
            json!({
                "region": "heteronet-global",
                "bucket_name": "escape-persistent",
                "quota_bytes": 10_737_418_240_u64,
                "quota_objects": 1_000_000,
            })
        );
        assert_eq!(
            provider_reconcile_spec("flash", json!({"metadata": {"key": "value"}}))?,
            json!({"metadata": {"key": "value"}})
        );
        Ok(())
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
