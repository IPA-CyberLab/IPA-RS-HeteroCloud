use std::{
    error::Error,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use super::super::{GetPolicy, ProviderFailure, provider_get_json};
use super::*;
use axum::{body::Body, http::StatusCode, middleware, response::IntoResponse};

fn fast_policy() -> GetPolicy {
    GetPolicy {
        total: Duration::from_millis(200),
        attempt: Duration::from_millis(120),
        backoff: Duration::from_millis(5),
        jitter: Duration::ZERO,
    }
}

#[tokio::test]
async fn get_recovery_and_permanent_failures() -> Result<(), Box<dyn Error>> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    for (status, body, limit, expected_attempts, success) in [
        (503, "{}", 128, 2, true),
        (502, "{}", 128, 2, true),
        (504, "{}", 128, 2, true),
        (429, "{}", 128, 1, false),
        (408, "{}", 128, 1, false),
        (400, "sensitive-provider-body", 128, 1, false),
        (401, "{}", 128, 1, false),
        (500, "{}", 128, 1, false),
        (302, "{}", 128, 1, false),
        (200, "not-json-secret", 128, 1, false),
        (200, "{\"secret\":1234}", 4, 1, false),
    ] {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        let app = Router::new().route(
            "/",
            get(move || {
                let first = counter.fetch_add(1, Ordering::SeqCst) == 0;
                async move {
                    (
                        StatusCode::from_u16(if first { status } else { 200 })
                            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                        body,
                    )
                }
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("http://{}/", listener.local_addr()?).parse()?;
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .build()?;
        let result =
            provider_get_json::<Value>(&client, url, "discovery", limit, fast_policy()).await;
        server.abort();
        assert_eq!(result.is_ok(), success, "status {status}, limit {limit}");
        assert_eq!(calls.load(Ordering::SeqCst), expected_attempts);
    }
    Ok(())
}

#[tokio::test]
async fn unavailable_get_stops_after_two_attempts() -> Result<(), Box<dyn Error>> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let app = Router::new().route(
        "/",
        get(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            async { StatusCode::SERVICE_UNAVAILABLE }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}/", listener.local_addr()?).parse()?;
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let client = reqwest::Client::builder()
        .retry(reqwest::retry::never())
        .build()?;
    let result = provider_get_json::<Value>(&client, url, "jwks", 128, fast_policy()).await;
    server.abort();
    assert!(matches!(result, Err(OidcError::ProviderUnavailable)));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    Ok(())
}

#[tokio::test]
async fn total_deadline_includes_headers_and_body() -> Result<(), Box<dyn Error>> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    for stall_body in [false, true] {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        let app = Router::new().route(
            "/",
            get(move || {
                counter.fetch_add(1, Ordering::SeqCst);
                async move {
                    if !stall_body {
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                    Body::from_stream(futures_util::stream::pending::<
                        Result<String, std::io::Error>,
                    >())
                }
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("http://{}/", listener.local_addr()?).parse()?;
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let client = reqwest::Client::builder()
            .retry(reqwest::retry::never())
            .build()?;
        let started = tokio::time::Instant::now();
        let policy = GetPolicy {
            total: Duration::from_secs(2),
            attempt: Duration::from_millis(1800),
            ..fast_policy()
        };
        let result = provider_get_json::<Value>(&client, url, "jwks", 128, policy).await;
        let elapsed = started.elapsed();
        server.abort();
        assert!(matches!(result, Err(OidcError::ProviderUnavailable)));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(elapsed >= policy.total);
        // A fresh timeout for attempt two would take at least 3605 ms.
        // Allow scheduler slack without accepting that deadline-reset regression.
        assert!(elapsed < Duration::from_millis(2800), "elapsed: {elapsed:?}");
    }
    Ok(())
}

#[tokio::test]
async fn callback_retries_gets_but_never_token_post_or_validation() -> Result<(), Box<dyn Error>> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    for (token_failure, bad_signature) in [(false, false), (true, false), (false, true)] {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let issuer = format!("http://{}", listener.local_addr()?);
        let counts = Arc::new([
            AtomicUsize::new(0),
            AtomicUsize::new(0),
            AtomicUsize::new(0),
        ]);
        let counters = counts.clone();
        let app = Router::new()
            .route("/.well-known/openid-configuration", get(discovery))
            .route("/token", post(token))
            .route("/jwks", get(jwks))
            .with_state(TestProvider {
                issuer: issuer.clone(),
                endpoint_origin: issuer.clone(),
            })
            .layer(middleware::from_fn(
                move |request: axum::extract::Request, next: middleware::Next| {
                    let counters = counters.clone();
                    async move {
                        let index = match request.uri().path() {
                            "/token" => 1,
                            "/jwks" => 2,
                            _ => 0,
                        };
                        let count = counters[index].fetch_add(1, Ordering::SeqCst);
                        if (index != 1 && count == 0) || (index == 1 && token_failure) {
                            return StatusCode::SERVICE_UNAVAILABLE.into_response();
                        }
                        next.run(request).await
                    }
                },
            ));
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let config = OidcConfig::new(
            issuer.parse()?,
            None,
            TEST_CLIENT_ID.to_owned(),
            SecretString::from("test-client-secret-value"),
            "http://console.example.test/api/v1/auth/oidc/callback".parse()?,
            true,
        )?;
        let key = SecretString::from("test-cookie-key-with-at-least-32-bytes");
        let start = config
            .begin_login(&key, false, OidcLoginIntent::Authenticate)
            .await?;
        let params: HashMap<String, String> =
            start.authorization_url.query_pairs().into_owned().collect();
        let nonce = params.get("nonce").ok_or("missing nonce")?;
        let result = config
            .complete_login(
                &OidcCallbackQuery {
                    code: Some(if bad_signature {
                        format!("invalid-signature:{nonce}")
                    } else {
                        nonce.clone()
                    }),
                    state: params.get("state").cloned(),
                    error: None,
                    error_description: None,
                },
                Some(start.transaction_cookie.value()),
                &key,
            )
            .await;
        server.abort();
        if token_failure {
            assert!(matches!(result, Err(OidcError::ProviderUnavailable)));
        } else if bad_signature {
            assert!(matches!(result, Err(OidcError::InvalidToken)));
        } else {
            assert!(result.is_ok());
        }
        assert_eq!(counts[0].load(Ordering::SeqCst), 3);
        assert_eq!(counts[1].load(Ordering::SeqCst), 1);
        assert_eq!(
            counts[2].load(Ordering::SeqCst),
            if token_failure { 0 } else { 2 }
        );
    }
    Ok(())
}

#[tokio::test]
async fn invalid_issuer_and_endpoints_are_not_retried() -> Result<(), Box<dyn Error>> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    for field in [
        "issuer",
        "authorization_endpoint",
        "token_endpoint",
        "jwks_uri",
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let issuer = format!("http://{}", listener.local_addr()?);
        let provider = TestProvider {
            issuer: issuer.clone(),
            endpoint_origin: issuer.clone(),
        };
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        let app = Router::new().route(
            "/.well-known/openid-configuration",
            get(move || {
                counter.fetch_add(1, Ordering::SeqCst);
                let provider = provider.clone();
                async move {
                    let Json(mut metadata) = discovery(State(provider)).await;
                    metadata[field] = json!("https://untrusted.example.test/");
                    Json(metadata)
                }
            }),
        );
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let config = OidcConfig::new(
            issuer.parse()?,
            None,
            TEST_CLIENT_ID.to_owned(),
            SecretString::from("test-client-secret-value"),
            "http://console.example.test/api/v1/auth/oidc/callback".parse()?,
            true,
        )?;
        let result = config.discovery().await;
        server.abort();
        assert!(matches!(result, Err(OidcError::ProviderUnavailable)));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    Ok(())
}

#[derive(Clone)]
struct LogBuffer(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for LogBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| std::io::Error::other("log lock"))?
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn diagnostics_contain_only_sanitized_fields() -> Result<(), Box<dyn Error>> {
    let buffer = LogBuffer(Arc::new(Mutex::new(Vec::new())));
    let writer = buffer.clone();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    let error = reqwest::Client::new()
        .get("http://[secret-code-token")
        .build()
        .err()
        .ok_or("expected invalid URL")?;
    tracing::subscriber::with_default(subscriber, || {
        ProviderFailure::transport(&error).report("token_exchange", 1, tokio::time::Instant::now());
        ProviderFailure::status(StatusCode::SERVICE_UNAVAILABLE).report(
            "jwks",
            2,
            tokio::time::Instant::now(),
        );
    });
    let output = String::from_utf8(buffer.0.lock().map_err(|_| "log lock")?.clone())?;
    for expected in [
        "token_exchange",
        "jwks",
        "failure_class",
        "transport",
        "http_status",
        "503",
        "attempt",
        "elapsed_ms",
    ] {
        assert!(output.contains(expected), "missing {expected}");
    }
    for secret in [
        "secret-code-token",
        "http://",
        "https://",
        "Authorization",
        "client_secret",
    ] {
        assert!(!output.contains(secret));
    }
    Ok(())
}
