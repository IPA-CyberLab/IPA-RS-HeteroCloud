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
use heterocloud_domain::{
    OrganizationId, PolicyDocument, PrincipalId, ProjectId, ServiceInstanceId,
};
use heterocloud_iam::semantics_digest;
use heterocloud_store::{BootstrapAdmin, Store};
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::sync::Semaphore;
use tower::ServiceExt;
use url::Url;
use uuid::Uuid;

#[tokio::test]
async fn task_iam_is_scoped_revocable_and_cli_oauth_can_manage_iam() -> Result<(), Box<dyn Error>> {
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

    let cli_key = "hcu_0123456789abcdef_test-only-CLI-secret-abcdefghijklmnopqrstuvwxyz";
    sqlx::query("INSERT INTO cli_access_tokens (id,user_id,organization_id,prefix,token_hash,expires_at) VALUES($1,$2,$3,$4,$5,now()+interval '1 hour')")
        .bind(Uuid::now_v7()).bind(owner.user.id.0).bind(org.0).bind("0123456789abcdef")
        .bind(token_hash(cli_key).as_slice()).execute(store.pool()).await?;
    let iam = format!("/api/v1/organizations/{org}/iam");
    let call = |method: &str,
                path: String,
                key: &str,
                body: Value|
     -> Result<Request<Body>, axum::http::Error> {
        Request::builder()
            .method(method)
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {key}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
    };
    let response = app(state.clone(), None)
        .oneshot(call(
            "POST",
            format!("{iam}/principals"),
            cli_key,
            json!({"name":"CLI created"}),
        )?)
        .await?;
    assert_eq!(response.status(), StatusCode::CREATED);
    let created: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await?)?;
    assert_eq!(created["kind"], "service_account");
    let principal_id = created["id"].as_str().ok_or("principal")?;
    let response = app(state.clone(), None)
        .oneshot(call(
            "POST",
            format!("{iam}/principals/{principal_id}/api-keys"),
            cli_key,
            json!({"name":"dedicated","expires_in_days":1}),
        )?)
        .await?;
    assert_eq!(
        response.status(),
        StatusCode::CREATED,
        "CLI OAuth can issue a dedicated key"
    );
    let key_response: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await?)?;
    let dedicated = key_response["api_key"].as_str().ok_or("key")?;
    assert_eq!(
        app(state.clone(), None)
            .oneshot(call(
                "GET",
                "/api/v1/auth/identity".into(),
                dedicated,
                Value::Null
            )?)
            .await?
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        app(state.clone(), None)
            .oneshot(call(
                "DELETE",
                format!(
                    "{iam}/principals/{principal_id}/api-keys/{}",
                    key_response["id"].as_str().ok_or("key id")?
                ),
                cli_key,
                Value::Null
            )?)
            .await?
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        app(state.clone(), None)
            .oneshot(call(
                "GET",
                "/api/v1/auth/identity".into(),
                dedicated,
                Value::Null
            )?)
            .await?
            .status(),
        StatusCode::UNAUTHORIZED
    );

    let mut spec = instance.spec.clone();
    spec["task_role"] = json!(principal.id);
    store
        .update_service_instance(
            org,
            ServiceInstanceId(instance.id.0),
            "flash",
            membership.principal_id,
            "workspace",
            spec.clone(),
        )
        .await?;
    let token = "hcw_test-only-workload-short-lived-credential";
    let pod = Uuid::now_v7();
    store
        .mint_workload_token(
            instance.id.0,
            principal.id.0,
            pod,
            &token_hash(token),
            Utc::now() + chrono::Duration::minutes(15),
        )
        .await?;
    let path = format!("/api/v1/organizations/{org}/flash/services/{}", instance.id);
    let identity = app(state.clone(), None)
        .oneshot(call(
            "GET",
            "/api/v1/auth/identity".into(),
            token,
            Value::Null,
        )?)
        .await?;
    assert_eq!(identity.status(), StatusCode::OK);
    let identity: Value =
        serde_json::from_slice(&to_bytes(identity.into_body(), 1024 * 1024).await?)?;
    assert_eq!(identity["type"], "workload");
    assert_eq!(identity["pod_uid"], pod.to_string());
    assert_eq!(
        app(state.clone(), None)
            .oneshot(call("POST", format!("{path}/start"), token, Value::Null)?)
            .await?
            .status(),
        StatusCode::ACCEPTED
    );
    assert_eq!(
        app(state.clone(), None)
            .oneshot(call("GET", path.clone(), token, Value::Null)?)
            .await?
            .status(),
        StatusCode::FORBIDDEN,
        "default deny for actions not granted"
    );
    assert_eq!(
        app(state.clone(), None)
            .oneshot(call(
                "GET",
                format!(
                    "/api/v1/organizations/{}/flash/services/{}",
                    Uuid::now_v7(),
                    instance.id
                ),
                token,
                Value::Null
            )?)
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    let create_path = format!("/api/v1/organizations/{org}/flash/services");
    assert_eq!(
        app(state.clone(), None)
            .oneshot(call(
                "POST",
                create_path.clone(),
                key,
                json!({"project_id":project.id,"name":"ungranted role","spec":spec})
            )?)
            .await?
            .status(),
        StatusCode::FORBIDDEN,
        "a service creator cannot pass an ungranted role"
    );
    let mut foreign = spec.clone();
    foreign["task_role"] = json!(Uuid::now_v7());
    assert_eq!(
        app(state.clone(), None)
            .oneshot(call(
                "POST",
                create_path,
                cli_key,
                json!({"project_id":project.id,"name":"foreign role","spec":foreign})
            )?)
            .await?
            .status(),
        StatusCode::BAD_REQUEST
    );
    let hash = token_hash(token);
    let binding = store
        .list_iam_bindings(org)
        .await?
        .into_iter()
        .find(|b| b["principal_id"] == principal.id.to_string())
        .ok_or("binding")?;
    store
        .delete_binding(
            org,
            Uuid::parse_str(binding["id"].as_str().ok_or("binding id")?)?,
        )
        .await?;
    assert_eq!(
        app(state.clone(), None)
            .oneshot(call("POST", format!("{path}/start"), token, Value::Null)?)
            .await?
            .status(),
        StatusCode::FORBIDDEN,
        "policy removal takes effect on an existing token"
    );
    store
        .set_service_account_enabled(org, principal.id, false)
        .await?;
    assert!(store.authenticate_workload_token(&hash).await?.is_none());
    store
        .set_service_account_enabled(org, principal.id, true)
        .await?;
    assert!(
        store.authenticate_workload_token(&hash).await?.is_none(),
        "old tokens do not return after re-enable"
    );
    for revoke in ["stop", "detach"] {
        store
            .mint_workload_token(
                instance.id.0,
                principal.id.0,
                pod,
                &hash,
                Utc::now() + chrono::Duration::minutes(15),
            )
            .await?;
        if revoke == "stop" {
            store
                .set_flash_service_stopped(
                    org,
                    ServiceInstanceId(instance.id.0),
                    membership.principal_id,
                    true,
                )
                .await?;
            store
                .set_flash_service_stopped(
                    org,
                    ServiceInstanceId(instance.id.0),
                    membership.principal_id,
                    false,
                )
                .await?;
        } else {
            let mut detached = spec.clone();
            detached["task_role"] = Value::Null;
            store
                .update_service_instance(
                    org,
                    ServiceInstanceId(instance.id.0),
                    "flash",
                    membership.principal_id,
                    "workspace",
                    detached,
                )
                .await?;
            store
                .update_service_instance(
                    org,
                    ServiceInstanceId(instance.id.0),
                    "flash",
                    membership.principal_id,
                    "workspace",
                    spec.clone(),
                )
                .await?;
        }
        assert!(
            store.authenticate_workload_token(&hash).await?.is_none(),
            "revoked tokens remain invalid after {revoke}"
        );
    }
    assert!(
        store
            .mint_workload_token(
                instance.id.0,
                Uuid::now_v7(),
                pod,
                &hash,
                Utc::now() + chrono::Duration::minutes(15)
            )
            .await
            .is_err()
    );
    assert!(
        !store
            .enabled_task_role(OrganizationId(Uuid::now_v7()), PrincipalId(principal.id.0))
            .await?
    );
    Ok(())
}
