use super::{OidcConfig, SecretString, TestProvider, discovery};
use axum::{Router, routing::get};

const CA: &[u8] = include_bytes!("tls-fixtures/ca.fixture");
const WRONG_CA: &[u8] = include_bytes!("tls-fixtures/wrong-ca.fixture");
const SERVER: &[u8] = include_bytes!("tls-fixtures/server.fixture");
const SERVER_KEY: &[u8] = include_bytes!("tls-fixtures/server-key.fixture");

#[tokio::test]
async fn oidc_private_ca_preserves_certificate_verification()
-> Result<(), Box<dyn std::error::Error>> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let issuer = format!("https://{}", listener.local_addr()?);
    let tls = axum_server::tls_rustls::RustlsConfig::from_pem(SERVER.to_vec(), SERVER_KEY.to_vec())
        .await?;
    let router = Router::new()
        .route("/.well-known/openid-configuration", get(discovery))
        .with_state(TestProvider {
            issuer: issuer.clone(),
            endpoint_origin: issuer.clone(),
        });
    let serving = axum_server::from_tcp_rustls(listener, tls)?;
    let server = tokio::spawn(async move { serving.serve(router.into_make_service()).await });
    let build = |roots: &[reqwest::Certificate]| {
        OidcConfig::new_with_root_certificates(
            issuer.parse().expect("fixture URL"),
            None,
            "fixture-client".to_owned(),
            SecretString::from("fixture-client-secret-only"),
            "https://console.example.test/api/v1/auth/oidc/callback"
                .parse()
                .expect("fixture callback"),
            false,
            roots,
        )
    };
    let missing = build(&[])?.discovery().await;
    let wrong = build(&[reqwest::Certificate::from_pem(WRONG_CA)?])?
        .discovery()
        .await;
    let trusted = build(&[reqwest::Certificate::from_pem(CA)?])?;
    let valid = trusted.discovery().await;
    let wrong_host = trusted
        .client
        .get(issuer.replace("127.0.0.1", "localhost"))
        .send()
        .await;
    server.abort();
    let _ = server.await;
    assert!(
        missing.is_err(),
        "private CA must not be trusted by default"
    );
    assert!(
        wrong.is_err(),
        "an unrelated CA must not authenticate the issuer"
    );
    assert!(
        valid.is_ok(),
        "the explicitly configured CA must work: {:?}",
        valid.as_ref().err()
    );
    assert!(
        wrong_host.is_err(),
        "additional CA must not disable hostname validation"
    );
    Ok(())
}
