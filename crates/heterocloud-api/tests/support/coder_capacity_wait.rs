use std::{
    env,
    error::Error,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use axum::{
    extract::Request,
    http::{HeaderValue, StatusCode, header},
    middleware::{self, Next},
    routing::post,
};
use heterocloud_api::{
    app,
    routes::{AppState, flash_capacity_waits::spawn_reconciler},
};
use heterocloud_auth::{generate_token, token_hash};
use heterocloud_domain::{OrganizationId, PrincipalId, ProjectId};
use heterocloud_iam::semantics_digest;
use secrecy::ExposeSecret;
use serde_json::json;
use url::Url;

pub async fn run(
    state: Arc<AppState>,
    org: OrganizationId,
    project: ProjectId,
    owner: PrincipalId,
    available: Arc<AtomicBool>,
    database: &str,
) -> Result<(), Box<dyn Error>> {
    let Ok(coder) = env::var("HETEROCLOUD_TEST_CODER") else {
        return Ok(());
    };
    let mut coder_database = Url::parse(database)?;
    coder_database.set_path("/heterocloud_test_coder_wait");
    coder_database
        .query_pairs_mut()
        .append_pair("sslmode", "disable");
    sqlx::query("CREATE DATABASE heterocloud_test_coder_wait")
        .execute(state.store.pool())
        .await?;
    let mut limits = state.store.effective_resource_quota(org).await?;
    limits.flash.max_memory_mib_per_vm = limits.flash.max_memory_mib_per_vm.max(8192);
    limits.flash.max_disk_gib_per_vm = limits.flash.max_disk_gib_per_vm.max(37);
    state
        .store
        .set_organization_resource_quota(org, &limits)
        .await?;
    let vpc = state
        .store
        .create_service_instance(
            org,
            project,
            owner,
            "vpc",
            "coder-capacity-fixture",
            json!({"region":"heteronet-global","security_groups":["workspaces"]}),
        )
        .await?;
    let actor = state
        .store
        .create_service_account(org, "coder-capacity-fixture")
        .await?;
    let policy = state
        .store
        .create_policy(
            org,
            "coder-capacity-fixture",
            &serde_json::from_value(json!({"version":"2026-07-31", "statements":[{
                "effect":"Allow","actions":["flash:*"],"resources":[format!("hc:org:{org}:flash/*")]
            },{
                "effect":"Allow","actions":["vpc:AttachSecurityGroup"],
                "resources":[format!("hc:org:{org}:vpc/network/{}/security-group/workspaces", vpc.id)]
            }]}))?,
            &semantics_digest(),
        )
        .await?;
    state.store.create_binding(org, actor.id, policy.id).await?;
    let api_key = format!("hc_2222222222222222_{}", generate_token()?.expose_secret());
    state
        .store
        .create_api_key(
            org,
            actor.id,
            "fixture",
            "2222222222222222",
            &token_hash(&api_key),
            Some(chrono::Utc::now() + chrono::Duration::minutes(15)),
        )
        .await?;
    let token = HeaderValue::from_str(&format!("Bearer {api_key}"))?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    available.store(false, Ordering::SeqCst);
    let router = app(state.clone(), None)
        .route(
            "/fixture/release-capacity",
            post(move || async move {
                available.store(true, Ordering::SeqCst);
                StatusCode::NO_CONTENT
            }),
        )
        .layer(middleware::from_fn(
            move |mut request: Request, next: Next| {
                let token = token.clone();
                async move {
                    // Loopback only, with a key scoped to the disposable database.
                    request.headers_mut().insert(header::AUTHORIZATION, token);
                    next.run(request).await
                }
            },
        ));
    let api = tokio::spawn(async move { axum::serve(listener, router).await });
    let reconciler = spawn_reconciler(state.clone());
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let result = tokio::process::Command::new("python3")
        .arg(root.join("scripts/e2e/coder_capacity_wait.py"))
        .env("CAPACITY_FIXTURE_CODER", coder)
        .env("CAPACITY_FIXTURE_CODER_DB", coder_database.as_str())
        .env("CAPACITY_FIXTURE_BRIDGE", origin)
        .env("CAPACITY_FIXTURE_ORG", org.to_string())
        .env("CAPACITY_FIXTURE_PROJECT", project.to_string())
        .env("CAPACITY_FIXTURE_VPC", vpc.id.to_string())
        .kill_on_drop(true)
        .status()
        .await;
    api.abort();
    reconciler.abort();
    if !result?.success() {
        return Err("isolated Coder capacity wait E2E failed".into());
    }
    Ok(())
}
