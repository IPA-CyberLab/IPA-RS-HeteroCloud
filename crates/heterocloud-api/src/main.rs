use std::sync::Arc;

use clap::Parser;
use heterocloud_api::{app, config::Config, routes::AppState};
use heterocloud_auth::hash_password;
use heterocloud_store::{BootstrapAdmin, Store};
use secrecy::ExposeSecret;
use tokio::sync::Semaphore;
use tokio::{net::TcpListener, signal};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    install_crypto_provider()?;
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("heterocloud_api=info,tower_http=info")),
        )
        .json()
        .init();

    let config = Config::parse();
    let secrets = config.load_secrets().await?;
    let store = Store::connect(
        secrets.database_url.expose_secret(),
        config.database_max_connections,
    )
    .await?;
    store.migrate().await?;

    if let (Some(email), Some(password)) = (&config.bootstrap_email, &secrets.bootstrap_password) {
        let password_hash = hash_password(password)?;
        store
            .bootstrap_admin(BootstrapAdmin {
                email,
                display_name: &config.bootstrap_display_name,
                password_hash: &password_hash,
                organization_slug: &config.bootstrap_organization_slug,
                organization_name: &config.bootstrap_organization_name,
            })
            .await?;
        warn!(
            email,
            "bootstrap account is present; remove bootstrap settings after first deployment"
        );
    }

    let runtime = config.runtime(
        secrets.csrf_key,
        secrets.flow_access_secret,
        secrets.oidc_client_secret,
    )?;
    let state = Arc::new(AppState {
        store,
        config: runtime,
        flow_client: reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(3))
            .timeout(std::time::Duration::from_secs(10))
            .build()?,
        registration_limiter: Arc::new(Semaphore::new(4)),
    });
    let router = app(state, config.console_dir.as_deref());
    if let (Some(cert), Some(key)) = (&config.tls_cert_file, &config.tls_key_file) {
        let tls = axum_server::tls_rustls::RustlsConfig::from_pem_file(cert, key).await?;
        let handle = axum_server::Handle::new();
        let shutdown_handle = handle.clone();
        tokio::spawn(async move {
            shutdown_signal().await;
            shutdown_handle.graceful_shutdown(Some(std::time::Duration::from_secs(30)));
        });
        info!(listen = %config.listen, tls = true, "HeteroCloud API is ready");
        axum_server::bind_rustls(config.listen, tls)
            .handle(handle)
            .serve(router.into_make_service())
            .await?;
    } else {
        let listener = TcpListener::bind(config.listen).await?;
        info!(listen = %config.listen, tls = false, "HeteroCloud API is ready");
        axum::serve(listener, router)
            .with_graceful_shutdown(shutdown_signal())
            .await?;
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

async fn shutdown_signal() {
    let ctrl_c = async {
        if signal::ctrl_c().await.is_err() {
            warn!("failed to install Ctrl-C handler");
        }
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
