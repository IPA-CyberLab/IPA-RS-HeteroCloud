use std::{
    env,
    error::Error,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use chrono::Utc;
use heterocloud_api::{
    app, config::RuntimeConfig, flash_provider::FlashProviderProxy, flow_access::FlowAccessSigner,
    routes::AppState,
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
        "statements":[{"effect":"Allow", "actions":["flash:UpdateInstance"], "resources":[resource]},
            {"effect":"Allow", "actions":["flash:CreateInstance"], "resources":[format!("hc:org:{org}:flash/*")]}]}))?;
    let policy = store
        .create_policy(org, "workspace lifecycle", &policy, &semantics_digest())
        .await?;
    store.create_binding(org, principal.id, policy.id).await?;
    let key = "hc_0123456789abcdef_0123456789abcdefghijklmnopqrstuvwxyzABCDEFG";
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
    let read_key = "hc_fedcba9876543210_ABCDEFG0123456789abcdefghijklmnopqrstuvwxyz";
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
        workload_identity: None,
        ssh_gateway: None,
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
    let collection = format!("/api/v1/organizations/{org}/flash/services");
    let provider_calls = Arc::new(AtomicUsize::new(0));
    let calls = Arc::clone(&provider_calls);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = Url::parse(&format!("http://{}/", listener.local_addr()?))?;
    let preview_provider = axum::Router::new().route(
        "/internal/v1/service-instances/{id}/preflight",
        axum::routing::post(
            move |headers: axum::http::HeaderMap, axum::Json(value): axum::Json<Value>| {
                calls.fetch_add(1, Ordering::SeqCst);
                async move {
                    assert!(headers.get(header::AUTHORIZATION).is_some());
                    assert_eq!(value["spec"]["region"], "test");
                    if value["existing"] == true {
                        axum::Json(
                            json!({"status":"blocked","reason":"insufficient_compute_capacity"}),
                        )
                    } else {
                        axum::Json(json!({"status":"feasible"}))
                    }
                }
            },
        ),
    );
    let server = tokio::spawn(async move { axum::serve(listener, preview_provider).await });
    let signer = heterocloud_provider::ProviderSigner::from_ed25519_pem(
        "test", "flash", "placement-test",
        b"-----BEGIN PRIVATE KEY-----\nMC4CAQAwBQYDK2VwBCIEICKoEEWPLg2OazcyTWzBEw/mMPPXatNOUcEUWDHo2y0Y\n-----END PRIVATE KEY-----\n",
    )?;
    let placement_state = Arc::new(AppState {
        flash_provider: Some(Arc::new(FlashProviderProxy::new(
            endpoint,
            signer,
            reqwest::Client::builder().no_proxy().build()?,
        ))),
        ..state.as_ref().clone()
    });
    let desired = json!({"name":"preview-only", "project_id":project.id, "spec":instance.spec});
    let preview_request = |method: &str,
                           path: &str,
                           auth_key: &str,
                           body: Option<&Value>|
     -> Result<Request<Body>, axum::http::Error> {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {auth_key}"))
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(body) = body {
            request.body(Body::from(body.to_string()))
        } else {
            request.body(Body::empty())
        }
    };
    let outbox_before: i64 = sqlx::query_scalar("SELECT count(*) FROM outbox_events")
        .fetch_one(store.pool())
        .await?;
    let create_preview = format!("{collection}/preflight");
    let existing_preview = format!("{path}/preflight");
    for (route, body, expected) in [
        (create_preview.as_str(), Some(&desired), "feasible"),
        (existing_preview.as_str(), None, "blocked"),
        (existing_preview.as_str(), Some(&desired), "blocked"),
    ] {
        let response = app(placement_state.clone(), None)
            .oneshot(preview_request("POST", route, key, body)?)
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::CACHE_CONTROL)
                .and_then(|value| value.to_str().ok()),
            Some("no-store")
        );
        let value: Value = serde_json::from_slice(&to_bytes(response.into_body(), 1024).await?)?;
        assert_eq!(value["status"], expected);
    }
    let denied = app(placement_state.clone(), None)
        .oneshot(preview_request("POST", &existing_preview, read_key, None)?)
        .await?;
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let blocked = app(placement_state, None)
        .oneshot(preview_request(
            "PUT",
            &format!("{path}?check_placement=true"),
            key,
            Some(&desired),
        )?)
        .await?;
    assert_eq!(blocked.status(), StatusCode::CONFLICT);
    let failure: Value = serde_json::from_slice(&to_bytes(blocked.into_body(), 1024).await?)?;
    assert_eq!(failure["error"]["code"], "flash_placement_blocked");
    assert_eq!(provider_calls.load(Ordering::SeqCst), 4);
    assert_eq!(
        store
            .list_service_instances(org, None, Some("flash"))
            .await?
            .len(),
        1
    );
    let after = store
        .service_instance(instance.id)
        .await?
        .ok_or("service was removed")?;
    assert_eq!(after.generation, instance.generation);
    assert_eq!(after.spec, instance.spec);
    let outbox_after: i64 = sqlx::query_scalar("SELECT count(*) FROM outbox_events")
        .fetch_one(store.pool())
        .await?;
    assert_eq!(outbox_before, outbox_after);
    server.abort();
    // Log access is distinct from both service updates and anonymous access.
    let log_path = format!("{path}/logs");
    assert_eq!(
        app(state.clone(), None)
            .oneshot(Request::get(&log_path).body(Body::empty())?)
            .await?
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app(state.clone(), None)
            .oneshot(
                Request::get(&log_path)
                    .header(header::AUTHORIZATION, format!("Bearer {key}"))
                    .body(Body::empty())?
            )
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    let log_reader = store.create_service_account(org, "log-reader").await?;
    let log_policy: PolicyDocument = serde_json::from_value(
        json!({"version":"2026-07-31", "statements":[{"effect":"Allow","actions":["flash:GetLogs"],"resources":[format!("hc:org:{org}:flash/instance/{}", instance.id)]}]}),
    )?;
    let log_policy = store
        .create_policy(org, "log reading", &log_policy, &semantics_digest())
        .await?;
    store
        .create_binding(org, log_reader.id, log_policy.id)
        .await?;
    let log_key = "hc_1234567890abcdef_0123456789abcdefghijklmnopqrstuvwxyzABCDEFG";
    store
        .create_api_key(
            org,
            log_reader.id,
            "logs",
            "1234567890abcdef",
            &token_hash(log_key),
            Some(Utc::now() + chrono::Duration::minutes(10)),
        )
        .await?;
    assert_eq!(
        app(state.clone(), None)
            .oneshot(
                Request::get(&log_path)
                    .header(header::AUTHORIZATION, format!("Bearer {log_key}"))
                    .body(Body::empty())?
            )
            .await?
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    let other_path = format!(
        "/api/v1/organizations/{org}/flash/services/{}/logs",
        Uuid::now_v7()
    );
    assert_eq!(
        app(state.clone(), None)
            .oneshot(
                Request::get(other_path)
                    .header(header::AUTHORIZATION, format!("Bearer {log_key}"))
                    .body(Body::empty())?
            )
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
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
