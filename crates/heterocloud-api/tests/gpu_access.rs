use std::{
    env,
    error::Error,
    sync::{Arc, Mutex},
    time::Duration as StdDuration,
};

use axum::{
    Json, Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
    routing::{get, put},
};
use chrono::{Duration, Utc};
use heterocloud_api::{
    app, config::RuntimeConfig, flash_provider::FlashProviderProxy, flow_access::FlowAccessSigner,
    routes::AppState,
};
use heterocloud_auth::{csrf_token, token_hash};
use heterocloud_provider::ProviderSigner;
use heterocloud_store::{BootstrapAdmin, Store};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use tokio::{net::TcpListener, sync::Semaphore, task::JoinHandle};
use tower::ServiceExt;
use url::Url;
use uuid::Uuid;

const TEST_DATABASE_ENV: &str = "HETEROCLOUD_TEST_DATABASE_URL";
const OWNER_ORIGIN: &str = "http://owner.example.test";
const SESSION_TOKEN: &str = "gpu-owner-session-token";
const TEST_KEY: &[u8] = b"-----BEGIN PRIVATE KEY-----\n\
MC4CAQAwBQYDK2VwBCIEIG45L/crBYvUcHKXo1ZbNr3YBSD3wPhsGq7IKyuU2+ei\n\
-----END PRIVATE KEY-----\n";

#[tokio::test]
async fn owner_can_isolate_private_gpu_without_exposing_it_to_user_catalog()
-> Result<(), Box<dyn Error>> {
    let Some((store, state, provider_task, cookie, csrf)) = test_state().await? else {
        return Ok(());
    };

    let unauthorized = app(state.clone(), None)
        .oneshot(Request::get("/api/v1/flash/gpu-types").body(Body::empty())?)
        .await?;
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let response = app(state.clone(), None)
        .oneshot(
            Request::get("/api/v1/owner/gpus")
                .header(header::HOST, "owner.example.test")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await?)?;
    let device_id = body["items"][0]["id"]
        .as_str()
        .ok_or("owner GPU response has no id")?;
    assert_eq!(body["items"][0]["visibility"], "private");
    assert_eq!(body["items"][0]["available"], true);
    assert_eq!(
        body["items"][0]["assigned_user_ids"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );

    let response = app(state.clone(), None)
        .oneshot(
            Request::get("/api/v1/flash/gpu-types")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await?)?;
    assert_eq!(
        body,
        json!({"items": [{
            "gpu_type": "nvidia-geforce-gtx-1080-ti",
            "display_name": "NVIDIA GeForce GTX 1080 Ti",
            "access": "private",
            "total": 1,
            "available": 1
        }]})
    );

    let response = app(state.clone(), None)
        .oneshot(
            Request::put(format!("/api/v1/owner/gpus/{device_id}"))
                .header(header::HOST, "owner.example.test")
                .header(header::ORIGIN, OWNER_ORIGIN)
                .header(header::COOKIE, &cookie)
                .header("x-heterocloud-csrf", csrf.expose_secret())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&json!({
                    "visibility": "private",
                    "assigned_user_ids": []
                }))?))?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await?)?;
    assert_eq!(body["visibility"], "private");
    assert_eq!(body["assigned_user_ids"], json!([]));

    let response = app(state, None)
        .oneshot(
            Request::get("/api/v1/flash/gpu-types")
                .header(header::COOKIE, cookie)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await?)?;
    assert_eq!(body, json!({"items": []}));

    let stored = store.list_gpu_devices().await?;
    assert_eq!(stored.len(), 1);
    assert!(stored[0].assigned_user_ids.is_empty());
    provider_task.abort();
    Ok(())
}

async fn test_state()
-> Result<Option<(Store, Arc<AppState>, JoinHandle<()>, String, SecretString)>, Box<dyn Error>> {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        rustls::crypto::ring::default_provider()
            .install_default()
            .map_err(|_| "failed to install the Rustls Ring provider")?;
    }
    let Ok(database_url) = env::var(TEST_DATABASE_ENV) else {
        return Ok(None);
    };
    let store = Store::connect(&database_url, 8).await?;
    let database_name: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(store.pool())
        .await?;
    if !database_name.starts_with("heterocloud_test_") {
        return Err(format!(
            "{TEST_DATABASE_ENV} must name a disposable database starting with heterocloud_test_"
        )
        .into());
    }
    sqlx::query("DROP SCHEMA public CASCADE")
        .execute(store.pool())
        .await?;
    sqlx::query("CREATE SCHEMA public")
        .execute(store.pool())
        .await?;
    store.migrate().await?;
    let owner = store
        .bootstrap_admin(BootstrapAdmin {
            email: "gpu-owner@example.test",
            display_name: "GPU Owner",
            password_hash: "test-password-hash",
            organization_slug: "gpu-owner",
            organization_name: "GPU Owner",
        })
        .await?;
    store
        .create_session(
            owner.user.id,
            &token_hash(SESSION_TOKEN),
            Utc::now() + Duration::hours(1),
            None,
            "local",
        )
        .await?;

    let catalog = Arc::new(Mutex::new(json!({"items": [{
        "management_id": "uc-k8sp5/GPU-a",
        "gpu_type": "nvidia-geforce-gtx-1080-ti",
        "display_name": "NVIDIA GeForce GTX 1080 Ti",
        "available": true,
        "visibility": "private",
        "assigned_user_ids": [owner.user.id.0, Uuid::from_u128(999)]
    }]})));
    let provider_app = Router::new()
        .route(
            "/internal/v1/gpus",
            get({
                let catalog = Arc::clone(&catalog);
                move || {
                    let catalog = Arc::clone(&catalog);
                    async move {
                        let value = catalog
                            .lock()
                            .map(|value| value.clone())
                            .unwrap_or_else(|_| json!({"items": []}));
                        Json(value)
                    }
                }
            }),
        )
        .route(
            "/internal/v1/gpus/access",
            put({
                let catalog = Arc::clone(&catalog);
                move |Json(update): Json<Value>| {
                    let catalog = Arc::clone(&catalog);
                    async move {
                        if let Ok(mut value) = catalog.lock()
                            && let Some(item) =
                                value["items"].as_array_mut().and_then(|v| v.first_mut())
                        {
                            item["visibility"] = update["visibility"].clone();
                            item["assigned_user_ids"] = update["assigned_user_ids"].clone();
                        }
                        StatusCode::NO_CONTENT
                    }
                }
            }),
        );
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = Url::parse(&format!("http://{}/", listener.local_addr()?))?;
    let provider_task = tokio::spawn(async move {
        let _result = axum::serve(listener, provider_app).await;
    });
    let flash_provider = FlashProviderProxy::new(
        endpoint,
        ProviderSigner::from_ed25519_pem("heterocloud", "heterocloud-flash", "test", TEST_KEY)?,
        reqwest::Client::builder().no_proxy().build()?,
    );
    let csrf_key = SecretString::from("test-csrf-key-at-least-32-bytes");
    let csrf = csrf_token(SESSION_TOKEN, &csrf_key)?;
    let owner_origin = Url::parse(OWNER_ORIGIN)?;
    let state = Arc::new(AppState {
        store: store.clone(),
        config: RuntimeConfig {
            public_origin: owner_origin.clone(),
            allowed_origins: vec![OWNER_ORIGIN.into()],
            trusted_proxy_networks: Vec::new(),
            secure_cookie: false,
            session_ttl: StdDuration::from_secs(3600),
            csrf_key,
            flow_access_signer: FlowAccessSigner::new(
                "heterocloud",
                "heterocloud-flow-data",
                SecretString::from("test-flow-access-secret-at-least-32-bytes"),
            )?,
            flow_public_endpoints: vec![Url::parse("https://flow.example.test")?],
            flow_internal_endpoint: Url::parse("http://flow.example.test")?,
            oidc: None,
            owner_origin: Some(owner_origin),
            owner_email: Some(owner.user.email),
            owner_console_mode: true,
            owner_allowed_networks: Vec::new(),
        },
        flow_client: reqwest::Client::builder().no_proxy().build()?,
        flash_provider: Some(Arc::new(flash_provider)),
        syouyu_provider: None,
        registry: None,
        registration_limiter: Arc::new(Semaphore::new(2)),
    });
    Ok(Some((
        store,
        state,
        provider_task,
        format!("hc_session={SESSION_TOKEN}"),
        csrf,
    )))
}
