use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use clap::Parser;
use heterocloud_domain::{OrganizationId, PrincipalId, ProjectId, ServiceInstanceId};
use heterocloud_provider::{AcceptedOperation, ProviderContext, ProviderSigner, ReconcileRequest};
use heterocloud_store::{OutboxEvent, Store};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
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
    let signer = ProviderSigner::from_ed25519_pem(
        &config.issuer,
        &config.flow_audience,
        &config.key_id,
        &signing_key,
    )?;
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
    info!(endpoint = %config.flow_endpoint, "provider worker is ready");

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
                    &signer,
                    &config.flow_endpoint,
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
    signer: &ProviderSigner,
    flow_endpoint: &Url,
) -> Result<(), WorkerError> {
    let Some(event) = store.claim_outbox_event().await? else {
        return Ok(());
    };
    match deliver(store, client, signer, flow_endpoint, &event).await {
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
    signer: &ProviderSigner,
    flow_endpoint: &Url,
    event: &OutboxEvent,
) -> Result<(), WorkerError> {
    if !matches!(
        event.topic.as_str(),
        "service-instance.reconcile" | "service-instance.delete"
    ) {
        return Err(WorkerError::UnsupportedTopic(event.topic.clone()));
    }
    let payload: ReconcilePayload = serde_json::from_value(event.payload.clone())?;
    if payload.provider != "flow" || payload.service_instance_id.0 != event.aggregate_id {
        return Err(WorkerError::InvalidPayload);
    }
    let instance = match store.service_instance(payload.service_instance_id).await? {
        Some(instance) => instance,
        None if event.topic == "service-instance.delete" => return Ok(()),
        None => return Err(WorkerError::MissingInstance),
    };
    if instance.generation != payload.generation
        || instance.organization_id != payload.organization_id
        || instance.project_id != payload.project_id
    {
        return Err(WorkerError::StalePayload);
    }
    let signed = signer.sign(ProviderContext {
        principal_id: payload.principal_id,
        organization_id: payload.organization_id,
        project_id: payload.project_id,
        service_instance_id: payload.service_instance_id,
        action: event.topic.clone(),
        generation: payload.generation,
    })?;
    let mut url = flow_endpoint.join(&format!(
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
            .complete_delete_service_instance(payload.service_instance_id, payload.generation)
            .await?
        {
            return Err(WorkerError::StalePayload);
        }
        info!(
            service_instance_id = %payload.service_instance_id,
            operation_id = %operation.operation_id,
            "realtime service deleted"
        );
        return Ok(());
    }
    if !store
        .mark_service_instance_ready(
            payload.service_instance_id,
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

#[derive(Deserialize)]
struct ReconcilePayload {
    service_instance_id: ServiceInstanceId,
    organization_id: OrganizationId,
    project_id: ProjectId,
    principal_id: PrincipalId,
    provider: String,
    generation: i64,
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
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Request(#[from] reqwest::Error),
    #[error(transparent)]
    Url(#[from] url::ParseError),
}
