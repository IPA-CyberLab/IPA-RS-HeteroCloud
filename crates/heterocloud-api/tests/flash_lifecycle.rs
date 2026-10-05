use std::{env, error::Error, sync::Arc, time::Duration};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use chrono::Utc;
use heterocloud_api::{
    app, config::RuntimeConfig, flow_access::FlowAccessSigner, routes::AppState,
};
use heterocloud_auth::token_hash;
use heterocloud_domain::{OrganizationId, PolicyDocument, ProjectId};
use heterocloud_iam::semantics_digest;
use heterocloud_store::{BootstrapAdmin, Store};
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::sync::Semaphore;
use tower::ServiceExt;
use url::Url;
use uuid::Uuid;

#[tokio::test]
async fn flash_stop_start_is_scoped_idempotent_and_preserves_configuration()
-> Result<(), Box<dyn Error>> {
    let Ok(url) = env::var("HETEROCLOUD_TEST_DATABASE_URL") else {
        return Ok(());
    };
    let _ = rustls::crypto::ring::default_provider().install_default();
    let store = Store::connect(&url, 8).await?;
    let db: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(store.pool())
        .await?;
    if !db.starts_with("heterocloud_test_") {
        return Err("a disposable heterocloud_test_ database is required".into());
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
            email: "lifecycle@example.test",
            display_name: "Lifecycle owner",
            password_hash: "test-only",
            organization_slug: "lifecycle",
            organization_name: "Lifecycle",
        })
        .await?;
    let membership = owner.memberships.first().ok_or("membership")?;
    let org = OrganizationId(membership.organization_id.0);
    let project = store
        .create_project(org, "workspaces", "Workspaces")
        .await?;
    let instance = store
        .create_service_instance(
            org,
            ProjectId(project.id.0),
            membership.principal_id,
            "flash",
            "workspace",
            json!({
                "region":"test", "image":"example/workspace:v1", "replicas":1,
                "cpu_millis":100, "memory_mib":256, "ephemeral_storage_gib":1,
                "ports":[], "exposure":{"type":"internal", "traffic_mode":"forwarded"},
                "env":{"HOME":"/root", "KEEP":"value"}, "command":["/bin/sh"],
                "args":["-c", "sleep infinity"], "metadata":{"workspace_id":"retained"}
            }),
        )
        .await?;
    let principal = store
        .create_service_account(org, "workspace-controller")
        .await?;
    let resource = format!("hc:org:{org}:flash/instance/{}", instance.id);
    let policy: PolicyDocument = serde_json::from_value(json!({"version":"2026-07-31",
        "statements":[{"effect":"Allow", "actions":["flash:UpdateInstance"], "resources":[resource]}]}))?;
    let policy = store
        .create_policy(org, "workspace lifecycle", &policy, &semantics_digest())
        .await?;
    store.create_binding(org, principal.id, policy.id).await?;
    let key = "hc_0123456789abcdef_lifecycle-test-secret";
    store
        .create_api_key(
            org,
            principal.id,
            "lifecycle",
            "0123456789abcdef",
            &token_hash(key),
            Some(Utc::now() + chrono::Duration::minutes(10)),
        )
        .await?;
    let reader = store.create_service_account(org, "read-only").await?;
    let read_key = "hc_fedcba9876543210_lifecycle-reader-secret";
    store
        .create_api_key(
            org,
            reader.id,
            "read only",
            "fedcba9876543210",
            &token_hash(read_key),
            Some(Utc::now() + chrono::Duration::minutes(10)),
        )
        .await?;
    store
        .create_session(
            owner.user.id,
            &token_hash("lifecycle-session"),
            Utc::now() + chrono::Duration::hours(1),
            None,
            "local",
        )
        .await?;
    let origin = Url::parse("http://cloud.example.test")?;
    let state = Arc::new(AppState {
        store: store.clone(),
        config: RuntimeConfig {
            public_origin: origin,
            secret_manager_origin: None,
            allowed_origins: vec!["http://cloud.example.test".into()],
            trusted_proxy_networks: Vec::new(),
            secure_cookie: false,
            session_ttl: Duration::from_secs(3600),
            cli_token_ttl: Duration::from_secs(3600),
            csrf_key: SecretString::from("test-csrf-key-at-least-32-bytes"),
            flow_access_signer: FlowAccessSigner::new(
                "test",
                "test",
                SecretString::from("test-flow-signing-key-at-least-32-bytes"),
            )?,
            flow_public_endpoints: vec![Url::parse("https://flow.example.test")?],
            flow_internal_endpoint: Url::parse("http://flow.example.test")?,
            oidc: None,
            owner_origin: None,
            owner_email: None,
            owner_console_mode: false,
            owner_allowed_networks: Vec::new(),
        },
        flow_client: reqwest::Client::builder().no_proxy().build()?,
        flash_provider: None,
        vpc_provider: None,
        syouyu_provider: None,
        registry: None,
        registration_limiter: Arc::new(Semaphore::new(2)),
    });
    let path = format!("/api/v1/organizations/{org}/flash/services/{}", instance.id);
    let post = |path: String, key: Option<&str>| -> Result<Request<Body>, axum::http::Error> {
        let mut r = Request::post(path);
        if let Some(key) = key {
            r = r.header(header::AUTHORIZATION, format!("Bearer {key}"));
        }
        r.body(Body::empty())
    };
    assert_eq!(
        app(state.clone(), None)
            .oneshot(post(format!("{path}/stop"), None)?)
            .await?
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app(state.clone(), None)
            .oneshot(post(format!("{path}/stop"), Some(read_key))?)
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    let csrf = Request::post(format!("{path}/stop"))
        .header(header::COOKIE, "hc_session=lifecycle-session")
        .header(header::ORIGIN, "http://cloud.example.test")
        .body(Body::empty())?;
    assert_eq!(
        app(state.clone(), None).oneshot(csrf).await?.status(),
        StatusCode::FORBIDDEN
    );
    let foreign = format!(
        "/api/v1/organizations/{}/flash/services/{}/stop",
        Uuid::now_v7(),
        instance.id
    );
    assert_eq!(
        app(state.clone(), None)
            .oneshot(post(foreign, Some(key))?)
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    let original_quota = store.effective_resource_quota(org).await?;
    let mut reduced = original_quota.clone();
    reduced.flash.max_memory_mib_per_vm = 128;
    store.set_organization_resource_quota(org, &reduced).await?;
    let response = app(state.clone(), None)
        .oneshot(post(format!("{path}/stop"), Some(key))?)
        .await?;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let stopped: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await?)?;
    assert_eq!(stopped["spec"]["stopped"], true);
    assert_eq!(stopped["generation"], instance.generation + 1);
    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM outbox_events")
        .fetch_one(store.pool())
        .await?;
    assert_eq!(
        app(state.clone(), None)
            .oneshot(post(format!("{path}/stop"), Some(key))?)
            .await?
            .status(),
        StatusCode::ACCEPTED
    );
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM outbox_events")
        .fetch_one(store.pool())
        .await?;
    assert_eq!(before, after, "idempotent stop must not enqueue work");
    let response = app(state.clone(), None)
        .oneshot(post(format!("{path}/start"), Some(key))?)
        .await?;
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "resume must respect reduced quota"
    );
    assert_eq!(
        store
            .service_instance(instance.id)
            .await?
            .ok_or("instance")?
            .generation,
        instance.generation + 1
    );
    store
        .set_organization_resource_quota(org, &original_quota)
        .await?;
    let response = app(state.clone(), None)
        .oneshot(post(format!("{path}/start"), Some(key))?)
        .await?;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let resumed: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await?)?;
    assert_eq!(resumed["spec"], instance.spec);
    assert_eq!(resumed["id"], instance.id.to_string());
    assert_eq!(resumed["generation"], instance.generation + 2);
    let stored = store
        .service_instance(instance.id)
        .await?
        .ok_or("instance")?;
    store
        .begin_delete_service_instance(org, instance.id, "flash", membership.principal_id)
        .await?;
    assert_eq!(
        app(state, None)
            .oneshot(post(format!("{path}/stop"), Some(key))?)
            .await?
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(stored.name, "workspace");
    Ok(())
}
