use std::{env, error::Error, sync::Arc, time::Duration};

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use chrono::Utc;
use heterocloud_api::{
    app, config::RuntimeConfig, flow_access::FlowAccessSigner, routes::AppState,
};
use heterocloud_auth::{csrf_token, token_hash};
use heterocloud_domain::{PolicyDocument, PrincipalId, UserId};
use heterocloud_iam::semantics_digest;
use heterocloud_store::{BootstrapAdmin, Store};
use secrecy::{ExposeSecret, SecretString};
use serde_json::json;
use tokio::sync::Semaphore;
use tower::ServiceExt;
use url::Url;
use uuid::Uuid;

const ORIGIN: &str = "http://cloud.example.test";
const OWNER: &str = "budget-owner-session";
const MEMBER: &str = "budget-member-session";

async fn mutate(
    state: Arc<AppState>,
    method: &str,
    path: &str,
    token: &str,
    csrf: bool,
) -> Result<StatusCode, Box<dyn Error>> {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::ORIGIN, ORIGIN)
        .header(header::COOKIE, format!("hc_session={token}"));
    if csrf {
        request = request.header(
            "x-heterocloud-csrf",
            csrf_token(token, &state.config.csrf_key)?.expose_secret(),
        );
    }
    let body = if method == "PUT" {
        request = request.header(header::CONTENT_TYPE, "application/json");
        Body::from(
            json!({"currency":"USD", "amount_microunits":10000000, "warning_percentage":80})
                .to_string(),
        )
    } else {
        Body::empty()
    };
    Ok(app(state, None)
        .oneshot(request.body(body)?)
        .await?
        .status())
}

#[tokio::test]
async fn budget_mutation_requires_admin_csrf_and_iam_permission() -> Result<(), Box<dyn Error>> {
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
    sqlx::raw_sql("DROP SCHEMA public CASCADE; CREATE SCHEMA public;")
        .execute(store.pool())
        .await?;
    store.migrate().await?;
    let owner = store
        .bootstrap_admin(BootstrapAdmin {
            email: "owner-budget@example.test",
            display_name: "Owner",
            password_hash: "test-only",
            organization_slug: "budget-test",
            organization_name: "Budget",
        })
        .await?;
    let org = owner
        .memberships
        .first()
        .ok_or("missing owner membership")?
        .organization_id;
    let other_org = store
        .create_organization(owner.user.id, "budget-other", "Other")
        .await?
        .id;
    let member = Uuid::now_v7();
    let principal = Uuid::now_v7();
    sqlx::query("INSERT INTO users (id,email,display_name,password_hash,status) VALUES ($1,'member-budget@example.test','Member','test-only','active')").bind(member).execute(store.pool()).await?;
    sqlx::query("INSERT INTO principals (id,organization_id,kind,name,user_id,quota_user_id,enabled) VALUES ($1,$2,'user','Member',$3,$3,true)").bind(principal).bind(org.0).bind(member).execute(store.pool()).await?;
    sqlx::query("INSERT INTO organization_memberships (organization_id,user_id,principal_id,role) VALUES ($1,$2,$3,'member')").bind(org.0).bind(member).bind(principal).execute(store.pool()).await?;
    for (user, token) in [(owner.user.id, OWNER), (UserId(member), MEMBER)] {
        store
            .create_session(
                user,
                &token_hash(token),
                Utc::now() + chrono::Duration::hours(1),
                None,
                "local",
            )
            .await?;
    }
    let state = Arc::new(AppState {
        workload_identity: None,
        ssh_gateway: None,
        store: store.clone(),
        config: RuntimeConfig {
            public_origin: Url::parse(ORIGIN)?,
            secret_manager_origin: None,
            allowed_origins: vec![ORIGIN.into()],
            trusted_proxy_networks: vec![],
            secure_cookie: false,
            session_ttl: Duration::from_secs(3600),
            cli_token_ttl: Duration::from_secs(3600),
            csrf_key: SecretString::from("test-csrf-key-at-least-32-bytes"),
            flow_access_signer: FlowAccessSigner::new(
                "heterocloud",
                "heterocloud-flow-data",
                SecretString::from("test-flow-access-secret-at-least-32-bytes"),
            )?,
            flow_public_endpoints: vec![Url::parse("http://flow.example.test")?],
            flow_internal_endpoint: Url::parse("http://flow.example.test")?,
            oidc: None,
            owner_origin: None,
            owner_email: None,
            owner_console_mode: false,
            owner_allowed_networks: vec![],
        },
        flow_client: reqwest::Client::new(),
        flash_provider: None,
        vpc_provider: None,
        syouyu_provider: None,
        registry: None,
        registration_limiter: Arc::new(Semaphore::new(2)),
    });
    let path = format!("/api/v1/organizations/{org}/flash/weekly-budget");
    let other_path = format!("/api/v1/organizations/{other_org}/flash/weekly-budget");
    assert_eq!(
        mutate(state.clone(), "PUT", &path, OWNER, false).await?,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        mutate(state.clone(), "PUT", &path, MEMBER, true).await?,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        mutate(state.clone(), "PUT", &other_path, MEMBER, true).await?,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        mutate(state.clone(), "PUT", &path, OWNER, true).await?,
        StatusCode::OK
    );
    assert!(store.flash_weekly_budget(other_org).await?.is_none());
    sqlx::query(
        "UPDATE organization_memberships SET role='admin' WHERE organization_id=$1 AND user_id=$2",
    )
    .bind(org.0)
    .bind(member)
    .execute(store.pool())
    .await?;
    assert_eq!(
        mutate(state.clone(), "PUT", &path, MEMBER, true).await?,
        StatusCode::OK
    );
    let deny = PolicyDocument::from_aws_json(
        json!({"Statement":{"Effect":"Deny", "Action":"billing:ManageBudget", "Resource":"*"}}),
    )?;
    let deny = store
        .create_policy(org, "DenyBudget", &deny, &semantics_digest())
        .await?;
    store
        .create_binding(org, PrincipalId(principal), deny.id)
        .await?;
    assert_eq!(
        mutate(state.clone(), "PUT", &path, MEMBER, true).await?,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        mutate(state.clone(), "DELETE", &path, MEMBER, true).await?,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        mutate(state.clone(), "DELETE", &path, OWNER, true).await?,
        StatusCode::NO_CONTENT
    );
    assert!(store.flash_weekly_budget(org).await?.is_none());
    Ok(())
}
