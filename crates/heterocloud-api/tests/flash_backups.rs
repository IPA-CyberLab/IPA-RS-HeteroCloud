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
async fn backups_enforce_tenant_iam_csrf_and_restore_only_into_a_new_stopped_service()
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
                "ports":[{"name":"http","protocol":"tcp","container_port":8080}], "exposure":{"type":"public", "traffic_mode":"forwarded","endpoint_mode":"web"},
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
        "statements":[{"effect":"Allow", "actions":["flash:CreateBackup","flash:GetInstance"], "resources":[resource]}]}))?;
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
    let backup_owner = store.create_service_account(org, "backup-owner").await?;
    let full_policy: PolicyDocument = serde_json::from_value(
        json!({"version":"2026-07-31","statements":[{"effect":"Allow","actions":["*"],"resources":[format!("hc:org:{org}:*")]}]}),
    )?;
    let full_policy = store
        .create_policy(
            org,
            "backup administration",
            &full_policy,
            &semantics_digest(),
        )
        .await?;
    store
        .create_binding(org, backup_owner.id, full_policy.id)
        .await?;
    let owner_key = "hc_9988776655443322_0123456789abcdefghijklmnopqrstuvwxyzABCDEFG";
    store
        .create_api_key(
            org,
            backup_owner.id,
            "backup owner",
            "9988776655443322",
            &token_hash(owner_key),
            Some(Utc::now() + chrono::Duration::minutes(10)),
        )
        .await?;
    let remote_available = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let remote = remote_available.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = Url::parse(&format!("http://{}/", listener.local_addr()?))?;
    let provider_app = axum::Router::new().fallback(move |request: Request<Body>| {
        let remote = remote.clone();
        async move {
            if !remote.load(std::sync::atomic::Ordering::SeqCst) {
                return (StatusCode::SERVICE_UNAVAILABLE, axum::Json(json!({})));
            }
            assert!(request.headers().get(header::AUTHORIZATION).is_some());
            let path = request.uri().path();
            let value = if path.ends_with("/backup-source") {
                json!({"source_pvc_uid":"fixture-pvc","generation":1,"volume_size_bytes":1048576})
            } else {
                json!({"id":path.rsplit('/').next(),"phase":"ready"})
            };
            (StatusCode::OK, axum::Json(value))
        }
    });
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, provider_app).await;
    });
    let signer=heterocloud_provider::ProviderSigner::from_ed25519_pem("test","flash","backup-test",b"-----BEGIN PRIVATE KEY-----\nMC4CAQAwBQYDK2VwBCIEICKoEEWPLg2OazcyTWzBEw/mMPPXatNOUcEUWDHo2y0Y\n-----END PRIVATE KEY-----\n")?;
    let proxy = heterocloud_api::flash_provider::FlashProviderProxy::new(
        endpoint,
        signer,
        reqwest::Client::builder().no_proxy().build()?,
    );
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
        flash_provider: Some(Arc::new(proxy)),
        vpc_provider: None,
        syouyu_provider: None,
        registry: None,
        registration_limiter: Arc::new(Semaphore::new(2)),
    });
    let path = format!(
        "/api/v1/organizations/{org}/flash/services/{}/backups",
        instance.id
    );
    let request_id = Uuid::now_v7();
    let body = json!({"name":"before-update","request_id":request_id});
    let send = |method: &str,
                path: String,
                credential: Option<&str>,
                body: Value|
     -> Result<Request<Body>, axum::http::Error> {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(key) = credential {
            request = request.header(header::AUTHORIZATION, format!("Bearer {key}"));
        }
        request.body(Body::from(body.to_string()))
    };
    for (credential, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(read_key), StatusCode::FORBIDDEN),
    ] {
        assert_eq!(
            app(state.clone(), None)
                .oneshot(send("POST", path.clone(), credential, body.clone())?)
                .await?
                .status(),
            expected
        );
    }
    let csrf = Request::post(&path)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "http://cloud.example.test")
        .header(header::COOKIE, "hc_session=lifecycle-session")
        .body(Body::from(body.to_string()))?;
    assert_eq!(
        app(state.clone(), None).oneshot(csrf).await?.status(),
        StatusCode::FORBIDDEN
    );
    let response = app(state.clone(), None)
        .oneshot(send("POST", path.clone(), Some(key), body.clone())?)
        .await?;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let backup: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await?)?;
    let id = Uuid::parse_str(backup["id"].as_str().ok_or("backup ID")?)?;
    assert!(backup.get("source_pvc_uid").is_none() && backup.get("source_spec").is_none());
    let response = app(state.clone(), None)
        .oneshot(send("POST", path, Some(key), body)?)
        .await?;
    let repeated: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await?)?;
    assert_eq!(backup["id"], repeated["id"]);
    let backup_path = format!("/api/v1/organizations/{org}/flash/backups/{id}");
    let restore_body =
        json!({"project_id":project.id,"name":"restored","request_id":Uuid::now_v7()});
    assert_eq!(
        app(state.clone(), None)
            .oneshot(send(
                "POST",
                format!("{backup_path}/restore"),
                Some(key),
                restore_body.clone()
            )?)
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app(state.clone(), None)
            .oneshot(send(
                "POST",
                format!("{backup_path}/restore"),
                Some(owner_key),
                restore_body.clone()
            )?)
            .await?
            .status(),
        StatusCode::CONFLICT,
        "incomplete backups cannot be restored"
    );
    store
        .update_flash_backup_status(id, "ready", json!({}))
        .await?;
    remote_available.store(false, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        app(state.clone(), None)
            .oneshot(send(
                "POST",
                format!("{backup_path}/restore"),
                Some(owner_key),
                restore_body.clone()
            )?)
            .await?
            .status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "an unavailable store must not appear restored"
    );
    remote_available.store(true, std::sync::atomic::Ordering::SeqCst);
    for _ in 0..2 {
        let response = app(state.clone(), None)
            .oneshot(send(
                "POST",
                format!("{backup_path}/restore"),
                Some(owner_key),
                restore_body.clone(),
            )?)
            .await?;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let restored: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await?)?;
        assert_ne!(restored["id"], json!(instance.id));
        assert_eq!(restored["spec"]["stopped"], true);
        assert_eq!(restored["spec"]["exposure"]["type"], "internal");
        assert_eq!(restored["spec"]["egress"]["mode"], "disabled");
        assert_eq!(restored["spec"]["env"], json!({}));
    }
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM service_instances WHERE name='restored'")
            .fetch_one(store.pool())
            .await?;
    assert_eq!(count, 1, "retries create exactly one restore target");
    let original = store.service_instance(instance.id).await?.ok_or("source")?;
    assert_eq!(original.generation, instance.generation);
    assert_eq!(original.spec, instance.spec);
    let wrong = format!(
        "/api/v1/organizations/{}/flash/backups/{id}",
        Uuid::now_v7()
    );
    assert_eq!(
        app(state.clone(), None)
            .oneshot(send("DELETE", wrong, Some(owner_key), json!({}))?)
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app(state.clone(), None)
            .oneshot(send("DELETE", backup_path, Some(owner_key), json!({}))?)
            .await?
            .status(),
        StatusCode::ACCEPTED
    );
    // New operational endpoints use the same tenant, IAM and CSRF boundary.
    let service_base = format!("/api/v1/organizations/{org}/flash/services/{}", instance.id);
    for suffix in ["revisions", "events", "backup-policy"] {
        assert_eq!(
            app(state.clone(), None)
                .oneshot(send(
                    "GET",
                    format!("{service_base}/{suffix}"),
                    Some(owner_key),
                    json!({})
                )?)
                .await?
                .status(),
            StatusCode::OK
        );
    }
    let policy =
        json!({"enabled":true,"interval_seconds":3600,"retention_count":2,"verify_restore":true});
    for operation in ["backup-policy", "rollback"] {
        let body = if operation == "rollback" {
            json!({"generation":1,"expected_generation":1})
        } else {
            policy.clone()
        };
        let method = if operation == "rollback" {
            "POST"
        } else {
            "PUT"
        };
        assert_eq!(
            app(state.clone(), None)
                .oneshot(send(
                    method,
                    format!("{service_base}/{operation}"),
                    Some(read_key),
                    body.clone()
                )?)
                .await?
                .status(),
            StatusCode::FORBIDDEN
        );
        let csrf = Request::builder()
            .method(method)
            .uri(format!("{service_base}/{operation}"))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ORIGIN, "http://cloud.example.test")
            .header(header::COOKIE, "hc_session=lifecycle-session")
            .body(Body::from(body.to_string()))?;
        assert_eq!(
            app(state.clone(), None).oneshot(csrf).await?.status(),
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        app(state.clone(), None)
            .oneshot(send(
                "PUT",
                format!("{service_base}/backup-policy"),
                Some(owner_key),
                policy
            )?)
            .await?
            .status(),
        StatusCode::OK
    );
    let request = json!({"generation":1,"expected_generation":0});
    assert_eq!(
        app(state.clone(), None)
            .oneshot(send(
                "POST",
                format!("{service_base}/rollback"),
                Some(owner_key),
                request
            )?)
            .await?
            .status(),
        StatusCode::CONFLICT
    );
    assert!(
        heterocloud_api::flash_operations::allowed(
            &state,
            membership.principal_id,
            org,
            "flash:CreateBackup",
            &resource
        )
        .await
    );
    sqlx::query("UPDATE principals SET enabled=false WHERE id=$1")
        .bind(membership.principal_id.0)
        .execute(store.pool())
        .await?;
    assert!(
        !heterocloud_api::flash_operations::allowed(
            &state,
            membership.principal_id,
            org,
            "flash:CreateBackup",
            &resource
        )
        .await,
        "disabled owners cannot authorize background backup/rollback actions"
    );
    sqlx::query("UPDATE principals SET enabled=true WHERE id=$1")
        .bind(membership.principal_id.0)
        .execute(store.pool())
        .await?;
    sqlx::query("UPDATE users SET status='suspended' WHERE id=$1")
        .bind(owner.user.id.0)
        .execute(store.pool())
        .await?;
    assert!(
        !heterocloud_api::flash_operations::allowed(
            &state,
            membership.principal_id,
            org,
            "flash:CreateBackup",
            &resource
        )
        .await,
        "inactive users cannot authorize background backup/rollback actions"
    );
    server.abort();
    Ok(())
}
