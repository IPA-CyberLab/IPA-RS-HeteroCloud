use std::{env, error::Error, sync::Arc, time::Duration};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use chrono::Utc;
use heterocloud_api::{
    app, config::RuntimeConfig, flow_access::FlowAccessSigner, routes::AppState,
};
use heterocloud_auth::{csrf_token, token_hash};
use heterocloud_domain::{PrincipalId, UserId};
use heterocloud_store::{BootstrapAdmin, Store};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use tokio::sync::Semaphore;
use tower::ServiceExt;
use url::Url;
use uuid::Uuid;

const ORIGIN: &str = "http://cloud.example.test";
const OWNER: &str = "identity-center-owner-session";
const MEMBER: &str = "identity-center-member-session";

async fn call(
    state: Arc<AppState>,
    method: &str,
    path: &str,
    session: &str,
    csrf: Option<&str>,
    body: Option<Value>,
) -> Result<(StatusCode, Value), Box<dyn Error>> {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::ORIGIN, ORIGIN)
        .header(header::COOKIE, format!("hc_session={session}"));
    if let Some(csrf) = csrf {
        request = request.header("x-heterocloud-csrf", csrf);
    }
    let body = if let Some(body) = body {
        request = request.header(header::CONTENT_TYPE, "application/json");
        Body::from(serde_json::to_vec(&body)?)
    } else {
        Body::empty()
    };
    let response = app(state, None).oneshot(request.body(body)?).await?;
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await?;
    Ok((
        status,
        if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes)?
        },
    ))
}

#[tokio::test]
async fn browser_assignments_enforce_permissions_scp_and_revocation() -> Result<(), Box<dyn Error>>
{
    let Ok(url) = env::var("HETEROCLOUD_TEST_DATABASE_URL") else {
        return Ok(());
    };
    let _ = rustls::crypto::ring::default_provider().install_default();
    let store = Store::connect(&url, 8).await?;
    let db: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(store.pool())
        .await?;
    if !db.starts_with("heterocloud_test_") {
        return Err("a disposable test database is required".into());
    }
    sqlx::raw_sql("DROP SCHEMA public CASCADE; CREATE SCHEMA public;")
        .execute(store.pool())
        .await?;
    store.migrate().await?;
    let owner = store
        .bootstrap_admin(BootstrapAdmin {
            email: "ic-api-owner@example.test",
            display_name: "Owner",
            password_hash: "test-only",
            organization_slug: "ic-api",
            organization_name: "Identity Center",
        })
        .await?;
    let management = owner.memberships[0].organization_id;
    let org = store
        .cloud_organization_for_account(management)
        .await?
        .ok_or("cloud org")?;
    let target = store
        .create_cloud_member_account(
            management,
            org.root_id,
            owner.user.id,
            "ic-target",
            "Target",
        )
        .await?;
    let quota_project = store
        .create_project(target.id, "quota-test", "Quota test")
        .await?;
    let member = UserId(Uuid::now_v7());
    let principal = PrincipalId(Uuid::now_v7());
    sqlx::query("INSERT INTO users(id,email,display_name,password_hash,status) VALUES($1,'ic-api-member@example.test','Member','test-only','active')").bind(member.0).execute(store.pool()).await?;
    sqlx::query("INSERT INTO principals(id,organization_id,kind,name,user_id,quota_user_id,enabled) VALUES($1,$2,'user','Member',$3,$3,true)").bind(principal.0).bind(management.0).bind(member.0).execute(store.pool()).await?;
    sqlx::query("INSERT INTO organization_memberships(organization_id,user_id,principal_id,role) VALUES($1,$2,$3,'member')").bind(management.0).bind(member.0).bind(principal.0).execute(store.pool()).await?;
    for (user, token) in [(owner.user.id, OWNER), (member, MEMBER)] {
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
    let csrf_key = SecretString::from("test-csrf-key-at-least-32-bytes");
    let owner_csrf = csrf_token(OWNER, &csrf_key)?;
    let member_csrf = csrf_token(MEMBER, &csrf_key)?;
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
            csrf_key,
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
    sqlx::query("UPDATE cloud_security_activation SET scp_enabled=true,roles_enabled=true,browser_roles_enabled=true,iam_conditions_enabled=true WHERE singleton=true").execute(store.pool()).await?;
    let base = format!("/api/v1/organizations/{management}");
    let center = format!("{base}/identity-center");
    let set_body = json!({"name":"Reader","document":{"Statement":{"Effect":"Allow","Action":["project:List","sts:AssumeRole"],"Resource":"*"}}});
    assert_eq!(
        call(
            state.clone(),
            "POST",
            &format!("{center}/permission-sets"),
            OWNER,
            Some(owner_csrf.expose_secret()),
            Some(set_body.clone())
        )
        .await?
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    store.enable_identity_center().await?;
    assert_eq!(
        call(
            state.clone(),
            "GET",
            &format!("{center}/users"),
            MEMBER,
            None,
            None
        )
        .await?
        .0,
        StatusCode::FORBIDDEN
    );
    let (status, set) = call(
        state.clone(),
        "POST",
        &format!("{center}/permission-sets"),
        OWNER,
        Some(owner_csrf.expose_secret()),
        Some(set_body),
    )
    .await?;
    assert_eq!(status, StatusCode::CREATED);
    let set_id = set["id"].as_str().ok_or("permission set")?;
    let (status, group) = call(
        state.clone(),
        "POST",
        &format!("{center}/groups"),
        OWNER,
        Some(owner_csrf.expose_secret()),
        Some(json!({"name":"Developers"})),
    )
    .await?;
    assert_eq!(status, StatusCode::CREATED);
    let group_id = group["id"].as_str().ok_or("group")?;
    let member_path = format!("{center}/groups/{group_id}/users/{member}");
    assert_eq!(
        call(
            state.clone(),
            "PUT",
            &member_path,
            OWNER,
            Some(owner_csrf.expose_secret()),
            None
        )
        .await?
        .0,
        StatusCode::NO_CONTENT
    );
    let (status,assignment)=call(state.clone(),"POST",&format!("{center}/assignments"),OWNER,Some(owner_csrf.expose_secret()),Some(json!({"permission_set_id":set_id,"account_id":target.id,"subject_type":"group","subject_id":group_id}))).await?;
    assert_eq!(status, StatusCode::CREATED);
    let assignment_id = assignment["id"].as_str().ok_or("assignment")?;
    let role_arn = assignment["role_arn"].as_str().ok_or("role ARN")?;
    let target_number = store
        .cloud_organization_for_account(target.id)
        .await?
        .ok_or("target account")?
        .current_account_number;
    let (status, chain_role) = call(state.clone(), "POST", &format!("/api/v1/organizations/{}/iam/roles",target.id), OWNER,
        Some(owner_csrf.expose_secret()), Some(json!({"name":"ChainTarget","trust_policy":{"Version":"2012-10-17","Statement":{"Effect":"Allow","Action":"sts:AssumeRole","Principal":{"AWS":format!("arn:aws:iam::{target_number}:root")}}}}))).await?;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        call(
            state.clone(),
            "POST",
            &format!("{base}/sts/assume-role"),
            OWNER,
            Some(owner_csrf.expose_secret()),
            Some(json!({"role_arn":role_arn,"session_name":"ordinary"}))
        )
        .await?
        .0,
        StatusCode::FORBIDDEN
    );
    let mine = format!("{center}/my-assignments");
    let (status, available) = call(state.clone(), "GET", &mine, MEMBER, None, None).await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(available["items"].as_array().map(Vec::len), Some(1));
    let start = format!("{mine}/{assignment_id}/session");
    assert_eq!(
        call(
            state.clone(),
            "POST",
            &start,
            MEMBER,
            Some(member_csrf.expose_secret()),
            Some(json!({"switch_console":true}))
        )
        .await?
        .0,
        StatusCode::OK
    );
    let projects = format!("/api/v1/organizations/{}/projects", target.id);
    assert_eq!(
        call(
            state.clone(),
            "POST",
            &format!("/api/v1/organizations/{}/sts/assume-role", target.id),
            MEMBER,
            Some(member_csrf.expose_secret()),
            Some(json!({"role_arn":chain_role["arn"],"session_name":"chain"}))
        )
        .await?
        .0,
        StatusCode::FORBIDDEN,
        "role chaining must not discard the user's quota and revocation lineage"
    );
    assert_eq!(
        call(state.clone(), "GET", &projects, MEMBER, None, None)
            .await?
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            state.clone(),
            "POST",
            &projects,
            MEMBER,
            Some(member_csrf.expose_secret()),
            Some(json!({"name":"Denied","slug":"denied"}))
        )
        .await?
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            state.clone(),
            "GET",
            &format!("/api/v1/organizations/{}/quotas", target.id),
            MEMBER,
            None,
            None
        )
        .await?
        .0,
        StatusCode::OK,
        "identity center users must see their own target-account quota"
    );
    let set_path = format!("{center}/permission-sets/{set_id}");
    let deny = json!({"document":{"Statement":{"Effect":"Deny","Action":"project:List","Resource":"*"}},"session_duration_seconds":3600});
    assert_eq!(
        call(
            state.clone(),
            "PUT",
            &set_path,
            OWNER,
            Some(owner_csrf.expose_secret()),
            Some(deny)
        )
        .await?
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(state.clone(), "GET", &projects, MEMBER, None, None)
            .await?
            .0,
        StatusCode::FORBIDDEN
    );
    let quota_admin = json!({"document":{"Statement":{"Effect":"Allow","Action":["project:List","flash:CreateInstance","identity:ListMemberQuotas","identity:UpdateMemberQuota"],"Resource":"*"}},"session_duration_seconds":3600});
    assert_eq!(
        call(
            state.clone(),
            "PUT",
            &set_path,
            OWNER,
            Some(owner_csrf.expose_secret()),
            Some(quota_admin)
        )
        .await?
        .0,
        StatusCode::OK
    );
    let quota_path = format!("/api/v1/organizations/{}/quotas", target.id);
    let (status, quota) = call(state.clone(), "GET", &quota_path, MEMBER, None, None).await?;
    assert_eq!(status, StatusCode::OK);
    assert!(
        quota["members"]
            .as_array()
            .is_some_and(|members| !members.is_empty()),
        "an authorized permission set must see the account's members"
    );
    let mut member_limits = quota["member"]["effective_limits"].clone();
    member_limits["flash"]["max_services"] = json!(1);
    let (quota_status, quota_response) = call(
        state.clone(),
        "PUT",
        &format!("/api/v1/organizations/{}/members/{member}/quota", target.id),
        MEMBER,
        Some(member_csrf.expose_secret()),
        Some(member_limits),
    )
    .await?;
    assert_eq!(
        quota_status,
        StatusCode::OK,
        "an authorized permission set must be able to manage member quotas: {quota_response}"
    );
    let flash_path = format!("/api/v1/organizations/{}/flash/services", target.id);
    let flash_spec = json!({"region":"test","image":"example.test/image:v1","replicas":1,"cpu_millis":100,"memory_mib":256,
        "ephemeral_storage_gib":1,"ports":[],"exposure":{"type":"internal","traffic_mode":"forwarded"},"env":{},"command":[],"args":[],"metadata":{}});
    let first_flash = json!({"project_id":quota_project.id,"name":"first","spec":flash_spec});
    let (status, created) = call(
        state.clone(),
        "POST",
        &flash_path,
        MEMBER,
        Some(member_csrf.expose_secret()),
        Some(first_flash.clone()),
    )
    .await?;
    assert_eq!(
        status,
        StatusCode::ACCEPTED,
        "the first service must fit the personal quota: {created}"
    );
    let service_id = Uuid::parse_str(created["id"].as_str().ok_or("service id")?)?;
    let charged: Uuid =
        sqlx::query_scalar("SELECT quota_user_id FROM service_instances WHERE id=$1")
            .bind(service_id)
            .fetch_one(store.pool())
            .await?;
    assert_eq!(
        charged, member.0,
        "the assigned user, not the shared role, must own the usage"
    );
    let mut second_flash = first_flash;
    second_flash["name"] = json!("second");
    let (status, rejected) = call(
        state.clone(),
        "POST",
        &flash_path,
        MEMBER,
        Some(member_csrf.expose_secret()),
        Some(second_flash),
    )
    .await?;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "the second service must exceed the user's personal quota: {rejected}"
    );
    let allow = json!({"document":{"Statement":{"Effect":"Allow","Action":"project:List","Resource":"*"}},"session_duration_seconds":3600});
    assert_eq!(
        call(
            state.clone(),
            "PUT",
            &set_path,
            OWNER,
            Some(owner_csrf.expose_secret()),
            Some(allow)
        )
        .await?
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(state.clone(), "GET", &projects, MEMBER, None, None)
            .await?
            .0,
        StatusCode::OK
    );
    let (status,scp)=call(state.clone(),"POST",&format!("{base}/organization/policies"),OWNER,Some(owner_csrf.expose_secret()),Some(json!({"name":"DenyProjects","document":{"Statement":{"Effect":"Deny","Action":"project:List","Resource":"*"}}}))).await?;
    assert_eq!(status, StatusCode::CREATED);
    let scp_path = format!(
        "{base}/organization/policies/{}/targets/account/{}",
        scp["id"].as_str().ok_or("scp")?,
        target.id
    );
    assert_eq!(
        call(
            state.clone(),
            "PUT",
            &scp_path,
            OWNER,
            Some(owner_csrf.expose_secret()),
            None
        )
        .await?
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(state.clone(), "GET", &projects, MEMBER, None, None)
            .await?
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            state.clone(),
            "DELETE",
            &scp_path,
            OWNER,
            Some(owner_csrf.expose_secret()),
            None
        )
        .await?
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(state.clone(), "GET", &projects, MEMBER, None, None)
            .await?
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            state.clone(),
            "DELETE",
            &member_path,
            OWNER,
            Some(owner_csrf.expose_secret()),
            None
        )
        .await?
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(state.clone(), "GET", &projects, MEMBER, None, None)
            .await?
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            state.clone(),
            "PUT",
            &member_path,
            OWNER,
            Some(owner_csrf.expose_secret()),
            None
        )
        .await?
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(state.clone(), "GET", &projects, MEMBER, None, None)
            .await?
            .0,
        StatusCode::FORBIDDEN,
        "old sessions must not revive when a group member is readded"
    );
    assert_eq!(
        call(
            state.clone(),
            "POST",
            "/api/v1/auth/role/exit",
            MEMBER,
            Some(member_csrf.expose_secret()),
            None
        )
        .await?
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(
            state.clone(),
            "POST",
            &start,
            MEMBER,
            Some(member_csrf.expose_secret()),
            Some(json!({"switch_console":true}))
        )
        .await?
        .0,
        StatusCode::OK
    );
    let assignment_path = format!("{center}/assignments/{assignment_id}");
    assert_eq!(
        call(
            state.clone(),
            "DELETE",
            &assignment_path,
            OWNER,
            Some(owner_csrf.expose_secret()),
            None
        )
        .await?
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(state.clone(), "GET", &projects, MEMBER, None, None)
            .await?
            .0,
        StatusCode::FORBIDDEN
    );
    Ok(())
}
