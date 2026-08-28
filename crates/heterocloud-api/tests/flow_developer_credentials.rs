use std::{env, error::Error, sync::Arc, time::Duration as StdDuration};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use chrono::{Duration, Utc};
use heterocloud_api::flow_access::FlowAccessSigner;
use heterocloud_api::{app, config::RuntimeConfig, routes::AppState};
use heterocloud_auth::{csrf_token, token_hash};
use heterocloud_domain::{OrganizationId, ProjectId};
use heterocloud_store::{BootstrapAdmin, Store};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use tokio::sync::Semaphore;
use tower::ServiceExt;
use url::Url;
use uuid::Uuid;

const TEST_DATABASE_ENV: &str = "HETEROCLOUD_FLOW_CREDENTIAL_TEST_DATABASE_URL";
const PUBLIC_ORIGIN: &str = "http://console.example.test";
const SESSION_TOKEN: &str = "flow-developer-credential-test-session";

#[tokio::test]
async fn management_and_developer_routes_issue_list_and_revoke_scoped_contexts()
-> Result<(), Box<dyn Error>> {
    let Some((store, state, organization_id, service_id, session_cookie, csrf)) =
        test_state().await?
    else {
        return Ok(());
    };
    let management_path = format!(
        "/api/v1/organizations/{organization_id}/realtime/services/{service_id}/developer-credentials"
    );
    let created = json_request(
        state.clone(),
        Request::post(&management_path)
            .header(header::ORIGIN, PUBLIC_ORIGIN)
            .header(header::COOKIE, &session_cookie)
            .header("x-heterocloud-csrf", csrf.expose_secret())
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&json!({
                "name": "application backend",
                "permissions": ["flow.room.join", "flow.signal.connect"],
                "expires_in_days": 30
            }))?))?,
    )
    .await?;
    assert_eq!(created.status, StatusCode::CREATED);
    assert_eq!(created.cache_control.as_deref(), Some("no-store"));
    let credential = created.body["credential"]
        .as_str()
        .ok_or("creation did not return the one-time credential")?
        .to_owned();
    assert!(credential.starts_with("hcf_"));
    assert_eq!(created.body["prefix"].as_str(), credential.get(..20));
    assert_eq!(
        created.body["mint_endpoint"],
        json!("http://console.example.test/api/v1/flow/v1/access-credentials")
    );
    let credential_id = created.body["id"]
        .as_str()
        .ok_or("creation did not return credential id")?
        .parse::<Uuid>()?;
    let stored_hash: Vec<u8> =
        sqlx::query_scalar("SELECT secret_hash FROM flow_developer_credentials WHERE id = $1")
            .bind(credential_id)
            .fetch_one(store.pool())
            .await?;
    assert_eq!(stored_hash, token_hash(&credential));

    let listed = json_request(
        state.clone(),
        Request::get(&management_path)
            .header(header::COOKIE, &session_cookie)
            .body(Body::empty())?,
    )
    .await?;
    assert_eq!(listed.status, StatusCode::OK);
    assert_eq!(listed.body["items"].as_array().map(Vec::len), Some(1));
    assert!(listed.body["items"][0].get("credential").is_none());

    let oversized = json_request(
        state.clone(),
        Request::post(&management_path)
            .header(header::ORIGIN, PUBLIC_ORIGIN)
            .header(header::COOKIE, &session_cookie)
            .header("x-heterocloud-csrf", csrf.expose_secret())
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(format!(
                "{{\"name\":\"{}\",\"permissions\":[\"flow.room.join\"],\"expires_in_days\":30}}",
                "x".repeat(17_000)
            )))?,
    )
    .await?;
    assert_eq!(oversized.status, StatusCode::PAYLOAD_TOO_LARGE);

    let denied = json_request(
        state.clone(),
        developer_mint_request(
            &credential,
            json!({
                "principal_id": Uuid::from_u128(100),
                "permissions": ["flow.turn.issue"],
                "expires_in_seconds": 300
            }),
        )?,
    )
    .await?;
    assert_eq!(denied.status, StatusCode::FORBIDDEN);

    let minted = json_request(
        state.clone(),
        developer_mint_request(
            &credential,
            json!({
                "principal_id": Uuid::from_u128(101),
                "permissions": ["flow.room.join"],
                "expires_in_seconds": 60
            }),
        )?,
    )
    .await?;
    assert_eq!(minted.status, StatusCode::CREATED);
    assert_eq!(minted.cache_control.as_deref(), Some("no-store"));
    let developer_context_id = minted.body["context_id"]
        .as_str()
        .ok_or("mint response has no context id")?
        .parse::<Uuid>()?;

    let developer_list = json_request(
        state.clone(),
        Request::get("/api/v1/flow/v1/access-credentials")
            .header(header::AUTHORIZATION, format!("Bearer {credential}"))
            .body(Body::empty())?,
    )
    .await?;
    assert_eq!(developer_list.status, StatusCode::OK);
    assert_eq!(
        developer_list.body["items"][0]["context_id"],
        json!(developer_context_id)
    );
    assert_eq!(
        developer_list.body["items"][0]["credential_id"],
        json!(credential_id)
    );

    let developer_delete_path =
        format!("/api/v1/flow/v1/access-credentials/{developer_context_id}");
    let first_delete = empty_request(
        state.clone(),
        Request::delete(&developer_delete_path)
            .header(header::AUTHORIZATION, format!("Bearer {credential}"))
            .body(Body::empty())?,
    )
    .await?;
    assert_eq!(first_delete, StatusCode::NO_CONTENT);
    let second_delete = empty_request(
        state.clone(),
        Request::delete(&developer_delete_path)
            .header(header::AUTHORIZATION, format!("Bearer {credential}"))
            .body(Body::empty())?,
    )
    .await?;
    assert_eq!(second_delete, StatusCode::NO_CONTENT);
    assert_eq!(
        revocation_event_count(&store, developer_context_id).await?,
        1
    );
    let ledger: (Option<Uuid>, Uuid, Uuid, Option<chrono::DateTime<Utc>>) = sqlx::query_as(
        "SELECT credential_id, service_instance_id, principal_id, revoked_at
         FROM flow_access_contexts WHERE context_id = $1",
    )
    .bind(developer_context_id)
    .fetch_one(store.pool())
    .await?;
    assert_eq!(ledger.0, Some(credential_id));
    assert_eq!(ledger.1, service_id.0);
    assert_eq!(ledger.2, Uuid::from_u128(101));
    assert!(ledger.3.is_some());
    let hcf_audits: Vec<(String, Value)> = sqlx::query_as(
        "SELECT action, metadata
         FROM audit_events
         WHERE action IN ('realtime:MintAccessCredential', 'realtime:RevokeAccessContext')
           AND metadata ->> 'context_id' = $1
         ORDER BY action",
    )
    .bind(developer_context_id.to_string())
    .fetch_all(store.pool())
    .await?;
    assert_eq!(hcf_audits.len(), 3);
    assert!(hcf_audits.iter().all(|(_, metadata)| {
        metadata["authentication"]["credential_id"] == json!(credential_id)
            && !metadata.to_string().contains("hcf_")
            && !metadata.to_string().contains("secret")
    }));

    let browser = json_request(
        state.clone(),
        Request::post(format!(
            "/api/v1/organizations/{organization_id}/realtime/services/{service_id}/access-credentials"
        ))
        .header(header::ORIGIN, PUBLIC_ORIGIN)
        .header(header::COOKIE, &session_cookie)
        .header("x-heterocloud-csrf", csrf.expose_secret())
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&json!({
            "permissions": ["flow.room.join"],
            "expires_in_seconds": 60
        }))?))?,
    )
    .await?;
    assert_eq!(browser.status, StatusCode::CREATED);
    let browser_context_id = browser.body["context_id"]
        .as_str()
        .ok_or("browser mint response has no context id")?
        .parse::<Uuid>()?;
    let browser_credential_id: Option<Uuid> =
        sqlx::query_scalar("SELECT credential_id FROM flow_access_contexts WHERE context_id = $1")
            .bind(browser_context_id)
            .fetch_one(store.pool())
            .await?;
    assert!(browser_credential_id.is_none());

    let management_delete = empty_request(
        state.clone(),
        Request::delete(format!(
            "/api/v1/organizations/{organization_id}/realtime/services/{service_id}/access-contexts/{browser_context_id}"
        ))
        .header(header::ORIGIN, PUBLIC_ORIGIN)
        .header(header::COOKIE, &session_cookie)
        .header("x-heterocloud-csrf", csrf.expose_secret())
        .body(Body::empty())?,
    )
    .await?;
    assert_eq!(management_delete, StatusCode::NO_CONTENT);
    assert_eq!(revocation_event_count(&store, browser_context_id).await?, 1);

    let cascade_minted = json_request(
        state.clone(),
        developer_mint_request(
            &credential,
            json!({
                "principal_id": Uuid::from_u128(103),
                "permissions": ["flow.room.join"],
                "expires_in_seconds": 300
            }),
        )?,
    )
    .await?;
    assert_eq!(cascade_minted.status, StatusCode::CREATED);
    let cascade_context_id = cascade_minted.body["context_id"]
        .as_str()
        .ok_or("cascade mint response has no context id")?
        .parse::<Uuid>()?;

    let revoke_credential = empty_request(
        state.clone(),
        Request::delete(format!("{management_path}/{credential_id}"))
            .header(header::ORIGIN, PUBLIC_ORIGIN)
            .header(header::COOKIE, &session_cookie)
            .header("x-heterocloud-csrf", csrf.expose_secret())
            .body(Body::empty())?,
    )
    .await?;
    assert_eq!(revoke_credential, StatusCode::NO_CONTENT);
    let cascade_revoked_at: Option<chrono::DateTime<Utc>> =
        sqlx::query_scalar("SELECT revoked_at FROM flow_access_contexts WHERE context_id = $1")
            .bind(cascade_context_id)
            .fetch_one(store.pool())
            .await?;
    assert!(cascade_revoked_at.is_some());
    assert_eq!(revocation_event_count(&store, cascade_context_id).await?, 1);
    let after_revoke = json_request(
        state,
        developer_mint_request(
            &credential,
            json!({
                "principal_id": Uuid::from_u128(102),
                "permissions": ["flow.room.join"],
                "expires_in_seconds": 60
            }),
        )?,
    )
    .await?;
    assert_eq!(after_revoke.status, StatusCode::UNAUTHORIZED);
    Ok(())
}

struct JsonResponse {
    status: StatusCode,
    cache_control: Option<String>,
    body: Value,
}

async fn json_request(
    state: Arc<AppState>,
    request: Request<Body>,
) -> Result<JsonResponse, Box<dyn Error>> {
    let response = app(state, None).oneshot(request).await?;
    let status = response.status();
    let cache_control = response
        .headers()
        .get(header::CACHE_CONTROL)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = to_bytes(response.into_body(), 1024 * 1024).await?;
    let body = if body.is_empty() {
        Value::Null
    } else {
        match serde_json::from_slice(&body) {
            Ok(value) => value,
            Err(_) => Value::String(String::from_utf8_lossy(&body).into_owned()),
        }
    };
    Ok(JsonResponse {
        status,
        cache_control,
        body,
    })
}

async fn empty_request(
    state: Arc<AppState>,
    request: Request<Body>,
) -> Result<StatusCode, Box<dyn Error>> {
    Ok(app(state, None).oneshot(request).await?.status())
}

fn developer_mint_request(
    credential: &str,
    payload: Value,
) -> Result<Request<Body>, Box<dyn Error>> {
    Ok(Request::post("/api/v1/flow/v1/access-credentials")
        .header(header::AUTHORIZATION, format!("Bearer {credential}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&payload)?))?)
}

async fn revocation_event_count(store: &Store, context_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT count(*)
         FROM outbox_events
         WHERE topic = 'principal-context.revoke' AND aggregate_id = $1",
    )
    .bind(context_id)
    .fetch_one(store.pool())
    .await
}

async fn test_state() -> Result<
    Option<(
        Store,
        Arc<AppState>,
        OrganizationId,
        heterocloud_domain::ServiceInstanceId,
        String,
        SecretString,
    )>,
    Box<dyn Error>,
> {
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
            email: "flow-api-owner@example.test",
            display_name: "Flow API Owner",
            password_hash: "test-owner-password-hash",
            organization_slug: "flow-api-test",
            organization_name: "Flow API Test",
        })
        .await?;
    let membership = owner
        .memberships
        .first()
        .ok_or("bootstrap owner has no membership")?;
    let organization_id = membership.organization_id;
    let project = store
        .create_project(organization_id, "flow-project", "Flow Project")
        .await?;
    let instance = store
        .create_service_instance(
            organization_id,
            ProjectId(project.id.0),
            membership.principal_id,
            "flow",
            "production-flow",
            json!({
                "region": "global",
                "max_participants": 100,
                "max_rooms": 100,
                "rate_limit": {"requests_per_second": 20, "burst": 40},
                "metadata": {}
            }),
        )
        .await?;
    assert!(
        store
            .mark_service_instance_ready(
                instance.id,
                "flow",
                instance.generation,
                Uuid::from_u128(10),
                json!({"phase": "ready"}),
            )
            .await?
    );
    let session_hash = token_hash(SESSION_TOKEN);
    store
        .create_session(
            owner.user.id,
            &session_hash,
            Utc::now() + Duration::hours(1),
        )
        .await?;
    let csrf_key = SecretString::from("test-csrf-key-at-least-32-bytes");
    let csrf = csrf_token(SESSION_TOKEN, &csrf_key)?;
    let state = Arc::new(AppState {
        store: store.clone(),
        config: RuntimeConfig {
            public_origin: Url::parse(PUBLIC_ORIGIN)?,
            allowed_origins: vec![PUBLIC_ORIGIN.to_owned()],
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
            owner_origin: None,
            owner_email: None,
            owner_console_mode: false,
            owner_allowed_networks: Vec::new(),
        },
        flow_client: reqwest::Client::builder()
            .tls_certs_only(Vec::<reqwest::tls::Certificate>::new())
            .build()?,
        flash_provider: None,
        registry: None,
        registration_limiter: Arc::new(Semaphore::new(2)),
    });
    Ok(Some((
        store,
        state,
        organization_id,
        instance.id,
        format!("hc_session={SESSION_TOKEN}"),
        csrf,
    )))
}
