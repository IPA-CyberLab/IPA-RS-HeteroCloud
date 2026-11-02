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
use heterocloud_domain::{OrganizationId, PolicyDocument};
use heterocloud_iam::semantics_digest;
use heterocloud_store::{BootstrapAdmin, Store};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use tokio::sync::Semaphore;
use tower::ServiceExt;
use url::Url;

const ORIGIN: &str = "http://cloud.example.test";
const TOKEN: &str = "organization-test-session";

async fn request(
    state: Arc<AppState>,
    method: &str,
    path: &str,
    body: Option<Value>,
    csrf: Option<&str>,
    bearer: Option<&str>,
) -> Result<(StatusCode, Value), Box<dyn Error>> {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(header::ORIGIN, ORIGIN)
        .header("idempotency-key", "01900000-0000-7000-8000-000000000001")
        .header(header::COOKIE, format!("hc_session={TOKEN}"));
    if let Some(token) = csrf {
        builder = builder.header("x-heterocloud-csrf", token);
    }
    if let Some(token) = bearer {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let body = if let Some(body) = body {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
        Body::from(serde_json::to_vec(&body)?)
    } else {
        Body::empty()
    };
    let response = app(state, None).oneshot(builder.body(body)?).await?;
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
async fn organization_api_enforces_iam_and_inherited_scp_for_all_credentials()
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
    sqlx::raw_sql("DROP SCHEMA public CASCADE; CREATE SCHEMA public;")
        .execute(store.pool())
        .await?;
    store.migrate().await?;
    let owner = store
        .bootstrap_admin(BootstrapAdmin {
            email: "organization-api@example.test",
            display_name: "Owner",
            password_hash: "test-only",
            organization_slug: "organization-api",
            organization_name: "Organization API",
        })
        .await?;
    let management = owner
        .memberships
        .first()
        .ok_or("missing management account")?
        .organization_id;
    store
        .create_session(
            owner.user.id,
            &token_hash(TOKEN),
            Utc::now() + chrono::Duration::hours(1),
            None,
            "local",
        )
        .await?;
    let csrf_key = SecretString::from("test-csrf-key-at-least-32-bytes");
    let csrf = csrf_token(TOKEN, &csrf_key)?;
    let state = Arc::new(AppState {
        workload_identity: None,
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
    let base = format!("/api/v1/organizations/{management}");
    let (status, org) = request(
        state.clone(),
        "GET",
        &format!("{base}/organization"),
        None,
        None,
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    let root = org["root_id"].as_str().ok_or("root")?;
    let no_csrf = request(
        state.clone(),
        "POST",
        &format!("{base}/organization/units"),
        Some(json!({"name":"Denied", "parent_id":root})),
        None,
        None,
    )
    .await?;
    assert_eq!(no_csrf.0, StatusCode::FORBIDDEN);
    let blocked_policy = request(state.clone(), "POST", &format!("{base}/organization/policies"),
        Some(json!({"name":"Pending", "document":{"Statement":{"Effect":"Deny","Action":"*","Resource":"*"}}})),
        Some(csrf.expose_secret()), None).await?;
    assert_eq!(blocked_policy.0, StatusCode::SERVICE_UNAVAILABLE);
    let conditional_policy = json!({"name":"TimeLimited", "document":{
        "Statement":{"Effect":"Allow","Action":"project:List","Resource":"*",
            "Condition":{"DateLessThan":{"aws:CurrentTime":"2100-01-01T00:00:00Z"}}}}});
    assert_eq!(
        request(
            state.clone(),
            "POST",
            &format!("{base}/iam/policies"),
            Some(conditional_policy.clone()),
            Some(csrf.expose_secret()),
            None
        )
        .await?
        .0,
        StatusCode::BAD_REQUEST,
        "conditions cannot be accepted during a mixed rollout"
    );
    let blocked_group = request(
        state.clone(),
        "POST",
        &format!("{base}/iam/groups"),
        Some(json!({"name":"Pending"})),
        Some(csrf.expose_secret()),
        None,
    )
    .await?;
    assert_eq!(blocked_group.0, StatusCode::SERVICE_UNAVAILABLE);
    sqlx::query("UPDATE cloud_security_activation SET scp_enabled = true WHERE singleton = true")
        .execute(store.pool())
        .await?;
    sqlx::query(
        "UPDATE cloud_security_activation SET iam_conditions_enabled = true WHERE singleton = true",
    )
    .execute(store.pool())
    .await?;
    assert_eq!(
        request(
            state.clone(),
            "POST",
            &format!("{base}/iam/policies"),
            Some(conditional_policy),
            Some(csrf.expose_secret()),
            None
        )
        .await?
        .0,
        StatusCode::CREATED
    );
    let (status, unit) = request(
        state.clone(),
        "POST",
        &format!("{base}/organization/units"),
        Some(json!({"name":"Workloads", "parent_id":root})),
        Some(csrf.expose_secret()),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::CREATED);
    let unit_id = unit["id"].as_str().ok_or("unit id")?;
    let (status, member) = request(
        state.clone(),
        "POST",
        &format!("{base}/organization/accounts"),
        Some(json!({"name":"Member", "slug":"member-account", "parent_id":unit_id})),
        Some(csrf.expose_secret()),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::CREATED);
    let member_id = member["id"].as_str().ok_or("member id")?;
    let member_account = OrganizationId(uuid::Uuid::parse_str(member_id)?);
    let (quota_status, quota_view) = request(
        state.clone(),
        "GET",
        &format!("/api/v1/organizations/{member_id}/quotas"),
        None,
        None,
        None,
    )
    .await?;
    assert_eq!(quota_status, StatusCode::OK);
    assert_eq!(quota_view["cloud_organization"]["id"], org["id"]);
    assert_eq!(
        quota_view["cloud_organization"]["service_quotas_enabled"],
        false
    );
    assert_eq!(
        quota_view["cloud_organization"]["flash_weekly_quotas_enabled"],
        false
    );
    assert_eq!(
        quota_view["cloud_organization"]["effective_limits"],
        serde_json::to_value(heterocloud_domain::ResourceQuotaLimits::default())?
    );
    let membership = store
        .session_user(owner.user.id)
        .await?
        .ok_or("session user")?
        .memberships
        .into_iter()
        .find(|membership| membership.organization_id == member_account)
        .ok_or("member principal")?;
    assert!(
        heterocloud_api::flash_operations::allowed(
            &state,
            membership.principal_id,
            member_account,
            "project:List",
            "*"
        )
        .await
    );
    let dated_deny = json!({"name":"TimedDeny", "document":{"Statement":{
        "Effect":"Deny", "Action":"project:List", "Resource":"*",
        "Condition":{"DateLessThan":{"aws:CurrentTime":"2100-01-01T00:00:00Z"}}}}});
    let (status, timed_policy) = request(
        state.clone(),
        "POST",
        &format!("{base}/organization/policies"),
        Some(dated_deny),
        Some(csrf.expose_secret()),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::CREATED);
    let timed_id = timed_policy["id"].as_str().ok_or("timed policy id")?;
    let timed_attachment =
        format!("{base}/organization/policies/{timed_id}/targets/unit/{unit_id}");
    assert_eq!(
        request(
            state.clone(),
            "PUT",
            &timed_attachment,
            None,
            Some(csrf.expose_secret()),
            None
        )
        .await?
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(
            state.clone(),
            "GET",
            &format!("/api/v1/organizations/{member_id}/projects"),
            None,
            None,
            None
        )
        .await?
        .0,
        StatusCode::FORBIDDEN
    );
    assert!(
        !heterocloud_api::flash_operations::allowed(
            &state,
            membership.principal_id,
            member_account,
            "project:List",
            "*"
        )
        .await
    );
    assert_eq!(
        request(
            state.clone(),
            "DELETE",
            &timed_attachment,
            None,
            Some(csrf.expose_secret()),
            None
        )
        .await?
        .0,
        StatusCode::NO_CONTENT
    );
    let unsupported = request(state.clone(), "POST", &format!("{base}/organization/policies"), Some(json!({"name":"Unsupported", "document":{"Statement":{"Effect":"Deny", "Action":"*", "Resource":"*", "Condition":{"StringEquals":{"x":"y"}}}}})), Some(csrf.expose_secret()), None).await?;
    assert_eq!(unsupported.0, StatusCode::BAD_REQUEST);
    let oversized = request(state.clone(), "POST", &format!("{base}/organization/policies"),
        Some(json!({"name":"Oversized", "document":{"Statement":{"Sid":"x".repeat(10_300),"Effect":"Deny","Action":"*","Resource":"*"}}})),
        Some(csrf.expose_secret()), None).await?;
    assert_eq!(oversized.0, StatusCode::BAD_REQUEST);
    let (status, policy) = request(state.clone(), "POST", &format!("{base}/organization/policies"), Some(json!({"name":"DenyProjects", "document":{"Version":"2012-10-17", "Statement":{"Effect":"Deny", "Action":"project:List", "Resource":"*"}}})), Some(csrf.expose_secret()), None).await?;
    assert_eq!(status, StatusCode::CREATED);
    let policy_id = policy["id"].as_str().ok_or("policy id")?;
    let attach = format!("{base}/organization/policies/{policy_id}/targets/unit/{unit_id}");
    assert_eq!(
        request(
            state.clone(),
            "PUT",
            &attach,
            None,
            Some(csrf.expose_secret()),
            None
        )
        .await?
        .0,
        StatusCode::NO_CONTENT
    );
    let projects = format!("/api/v1/organizations/{member_id}/projects");
    assert_eq!(
        request(state.clone(), "GET", &projects, None, None, None)
            .await?
            .0,
        StatusCode::FORBIDDEN
    );
    assert!(
        !heterocloud_api::flash_operations::allowed(
            &state,
            membership.principal_id,
            member_account,
            "project:List",
            "*"
        )
        .await,
        "inherited SCP applies to an owner running background work"
    );
    assert_eq!(
        request(
            state.clone(),
            "GET",
            &format!("{base}/projects"),
            None,
            None,
            None
        )
        .await?
        .0,
        StatusCode::OK
    );

    let robot = store
        .create_service_account(member_account, "robot")
        .await?;
    let allow = PolicyDocument::from_aws_json(
        json!({"Statement":{"Effect":"Allow", "Action":"project:List", "Resource":"*"}}),
    )?;
    let allow = store
        .create_policy(member_account, "ProjectList", &allow, &semantics_digest())
        .await?;
    store
        .create_binding(member_account, robot.id, allow.id)
        .await?;
    let key = "hc_1234567890abcdef_0123456789abcdefghijklmnopqrstuvwxyzABCDEFG";
    store
        .create_api_key(
            member_account,
            robot.id,
            "robot",
            "1234567890abcdef",
            &token_hash(key),
            Some(Utc::now() + chrono::Duration::hours(1)),
        )
        .await?;
    assert_eq!(
        request(state.clone(), "GET", &projects, None, None, Some(key))
            .await?
            .0,
        StatusCode::FORBIDDEN
    );
    assert!(
        !heterocloud_api::flash_operations::allowed(
            &state,
            robot.id,
            member_account,
            "project:List",
            "*"
        )
        .await,
        "inherited SCP applies to a background service account"
    );
    assert_eq!(
        request(
            state.clone(),
            "PUT",
            &format!("{base}/organization/accounts/{member_id}/parent"),
            Some(json!({"parent_id":root})),
            Some(csrf.expose_secret()),
            None
        )
        .await?
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(state.clone(), "GET", &projects, None, None, Some(key))
            .await?
            .0,
        StatusCode::OK
    );
    assert!(
        heterocloud_api::flash_operations::allowed(
            &state,
            robot.id,
            member_account,
            "project:List",
            "*"
        )
        .await,
        "moving an account removes the inherited deny"
    );
    let deny = PolicyDocument::from_aws_json(
        json!({"Statement":{"Effect":"Deny", "Action":"project:List", "Resource":"*"}}),
    )?;
    let deny = store
        .create_policy(member_account, "OwnerDeny", &deny, &semantics_digest())
        .await?;
    let boundary_path = format!(
        "/api/v1/organizations/{member_id}/iam/principals/{}/permissions-boundary",
        robot.id
    );
    assert_eq!(
        request(
            state.clone(),
            "PUT",
            &boundary_path,
            Some(json!({"policy_id":deny.id})),
            Some(csrf.expose_secret()),
            None
        )
        .await?
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(state.clone(), "GET", &projects, None, None, Some(key))
            .await?
            .0,
        StatusCode::FORBIDDEN
    );
    assert!(
        !heterocloud_api::flash_operations::allowed(
            &state,
            robot.id,
            member_account,
            "project:List",
            "*"
        )
        .await,
        "permissions boundaries apply to background work"
    );
    assert_eq!(
        request(
            state.clone(),
            "DELETE",
            &boundary_path,
            None,
            Some(csrf.expose_secret()),
            None
        )
        .await?
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(state.clone(), "GET", &projects, None, None, Some(key))
            .await?
            .0,
        StatusCode::OK
    );
    let group_path = format!("/api/v1/organizations/{member_id}/iam/groups");
    let (status, group) = request(
        state.clone(),
        "POST",
        &group_path,
        Some(json!({"name":"Restricted"})),
        Some(csrf.expose_secret()),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::CREATED);
    let group_id = group["id"].as_str().ok_or("group id")?;
    let group_policy = format!("{group_path}/{group_id}/policies/{}", deny.id);
    assert_eq!(
        request(
            state.clone(),
            "PUT",
            &group_policy,
            None,
            Some(csrf.expose_secret()),
            None
        )
        .await?
        .0,
        StatusCode::NO_CONTENT
    );
    let group_robot = format!("{group_path}/{group_id}/users/{}", robot.id);
    assert_eq!(
        request(
            state.clone(),
            "PUT",
            &group_robot,
            None,
            Some(csrf.expose_secret()),
            None
        )
        .await?
        .0,
        StatusCode::NOT_FOUND
    );
    let group_member = format!("{group_path}/{group_id}/users/{}", membership.principal_id);
    assert_eq!(
        request(
            state.clone(),
            "PUT",
            &group_member,
            None,
            Some(csrf.expose_secret()),
            None
        )
        .await?
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(state.clone(), "GET", &projects, None, None, None)
            .await?
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            state.clone(),
            "DELETE",
            &group_member,
            None,
            Some(csrf.expose_secret()),
            None
        )
        .await?
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(state.clone(), "GET", &projects, None, None, None)
            .await?
            .0,
        StatusCode::OK
    );
    store
        .create_binding(member_account, membership.principal_id, deny.id)
        .await?;
    assert!(
        !heterocloud_api::flash_operations::allowed(
            &state,
            membership.principal_id,
            member_account,
            "project:List",
            "*"
        )
        .await,
        "an explicit identity deny overrides an owner"
    );
    assert_eq!(
        request(state.clone(), "GET", &projects, None, None, None)
            .await?
            .0,
        StatusCode::FORBIDDEN
    );

    // Role trust is independent from the source identity grant. This checks
    // real bearer credentials across two accounts and rejects privilege bypass.
    let source_number = org["current_account_number"]
        .as_str()
        .ok_or("source account number")?;
    let trust = json!({"Version":"2012-10-17","Statement":{"Effect":"Allow","Action":"sts:AssumeRole",
        "Principal":{"AWS":format!("arn:aws:iam::{source_number}:root")}}});
    let roles = format!("/api/v1/organizations/{member_id}/iam/roles");
    let role_body = json!({"name":"WorkspaceReader","trust_policy":trust});
    assert_eq!(
        request(
            state.clone(),
            "POST",
            &roles,
            Some(role_body.clone()),
            Some(csrf.expose_secret()),
            None
        )
        .await?
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    sqlx::query("UPDATE cloud_security_activation SET roles_enabled = true WHERE singleton = true")
        .execute(store.pool())
        .await?;
    let (status, role) = request(
        state.clone(),
        "POST",
        &roles,
        Some(role_body),
        Some(csrf.expose_secret()),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::CREATED);
    let role_arn = role["arn"].as_str().ok_or("role arn")?;
    let role_principal = heterocloud_domain::PrincipalId(uuid::Uuid::parse_str(
        role["principal_id"].as_str().ok_or("role principal")?,
    )?);
    assert!(
        !store
            .enabled_task_role(member_account, role_principal)
            .await?
    );
    assert!(
        store
            .create_api_key(
                member_account,
                role_principal,
                "must fail",
                "0123456789fedcba",
                &token_hash("not-issued"),
                None
            )
            .await
            .is_err()
    );
    let role_policy = PolicyDocument::from_aws_json(
        json!({"Statement":{"Effect":"Allow", "Action":"project:*", "Resource":"*"}}),
    )?;
    let role_policy = store
        .create_policy(
            member_account,
            "RoleProjects",
            &role_policy,
            &semantics_digest(),
        )
        .await?;
    let role_binding = store
        .create_binding(member_account, role_principal, role_policy.id)
        .await?;
    assert!(
        !heterocloud_api::flash_operations::allowed(
            &state,
            role_principal,
            member_account,
            "project:List",
            "*"
        )
        .await,
        "an assumed role session cannot become an unrestricted durable grant"
    );
    let source_robot = store
        .create_service_account(management, "source-robot")
        .await?;
    let source_key = "hc_abcdef0123456789_0123456789abcdefghijklmnopqrstuvwxyzABCDEFG";
    store
        .create_api_key(
            management,
            source_robot.id,
            "source",
            "abcdef0123456789",
            &token_hash(source_key),
            Some(Utc::now() + chrono::Duration::hours(1)),
        )
        .await?;
    let assume_path = format!("{base}/sts/assume-role");
    let assume_body = json!({"role_arn":role_arn,"session_name":"read-only","session_policy":{"Version":"2012-10-17",
        "Statement":{"Effect":"Allow","Action":"project:List","Resource":"*"}}});
    assert_eq!(
        request(
            state.clone(),
            "POST",
            &assume_path,
            Some(assume_body.clone()),
            None,
            Some(source_key)
        )
        .await?
        .0,
        StatusCode::FORBIDDEN
    );
    let source_policy = PolicyDocument::from_aws_json(
        json!({"Statement":{"Effect":"Allow", "Action":"sts:AssumeRole", "Resource":role_arn}}),
    )?;
    let source_policy = store
        .create_policy(
            management,
            "AssumeReader",
            &source_policy,
            &semantics_digest(),
        )
        .await?;
    store
        .create_binding(management, source_robot.id, source_policy.id)
        .await?;

    // A temporary role or workload session must not mint an independent
    // provider secret that survives the session's revocation or expiry.
    let provider_policy = PolicyDocument::from_aws_json(json!({"Statement":{
        "Effect":"Allow", "Action":["registry:CreateCredential", "syouyu:CreateCredential",
            "realtime:IssueAccessCredential", "flow:RoomJoin"], "Resource":"*"}}))?;
    let provider_policy = store
        .create_policy(
            member_account,
            "ProviderCredentials",
            &provider_policy,
            &semantics_digest(),
        )
        .await?;
    let role_provider_binding = store
        .create_binding(member_account, role_principal, provider_policy.id)
        .await?;
    store
        .create_binding(member_account, robot.id, provider_policy.id)
        .await?;
    let (status, provider_session) = request(
        state.clone(),
        "POST",
        &assume_path,
        Some(json!({"role_arn":role_arn,"session_name":"provider-credentials"})),
        None,
        Some(source_key),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    let provider_role_token = provider_session["access_token"]
        .as_str()
        .ok_or("provider role token")?;
    let provider_session_id = provider_session["session_id"]
        .as_str()
        .ok_or("provider session id")?;

    let credential_project = store
        .create_project(member_account, "provider-tests", "Provider Tests")
        .await?;
    let flash = store
        .create_service_instance(
            member_account,
            credential_project.id,
            membership.principal_id,
            "flash",
            "workload-test",
            json!({"region":"test", "image":"example/workspace:v1", "replicas":1,
                "cpu_millis":100, "memory_mib":256, "ephemeral_storage_gib":1,
                "ports":[], "exposure":{"type":"internal", "traffic_mode":"forwarded"},
                "env":{}, "command":[], "args":[], "metadata":{}, "task_role":robot.id.0}),
        )
        .await?;
    let workload_token = "hcw_test_provider_credential_session";
    store
        .mint_workload_token(
            flash.id.0,
            robot.id.0,
            uuid::Uuid::now_v7(),
            &token_hash(workload_token),
            Utc::now() + chrono::Duration::minutes(15),
        )
        .await?;
    let flow = store
        .create_service_instance(
            member_account,
            credential_project.id,
            membership.principal_id,
            "flow",
            "provider-test-flow",
            json!({"region":"global", "max_participants":100, "max_rooms":100,
            "rate_limit":{"requests_per_second":20,"burst":40}, "metadata":{}}),
        )
        .await?;
    assert!(
        store
            .mark_service_instance_ready(
                flow.id,
                "flow",
                flow.generation,
                uuid::Uuid::now_v7(),
                json!({"phase":"ready"}),
            )
            .await?
    );
    let registry_path = format!("/api/v1/organizations/{member_id}/registry/credentials");
    let syouyu_path = format!(
        "/api/v1/organizations/{member_id}/syouyu/buckets/{}/credentials",
        uuid::Uuid::now_v7()
    );
    let flow_path = format!(
        "/api/v1/organizations/{member_id}/realtime/services/{}/developer-credentials",
        flow.id
    );
    let flow_credential_body =
        json!({"name":"test", "permissions":["flow.room.join"], "expires_in_days":30});
    let (status, owner_flow_credential) = request(
        state.clone(),
        "POST",
        &flow_path,
        Some(flow_credential_body.clone()),
        Some(csrf.expose_secret()),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::CREATED);
    let flow_credential_id = owner_flow_credential["id"]
        .as_str()
        .ok_or("flow credential id")?;
    for token in [provider_role_token, workload_token] {
        for (path, body) in [
            (registry_path.clone(), json!({"name":"test"})),
            (
                syouyu_path.clone(),
                json!({"name":"test","permissions":["read"]}),
            ),
            (flow_path.clone(), flow_credential_body.clone()),
            (
                format!("{flow_path}/{flow_credential_id}/rotate"),
                json!({}),
            ),
        ] {
            let (status, response) =
                request(state.clone(), "POST", &path, Some(body), None, Some(token)).await?;
            assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {response}");
            assert_eq!(
                response["error"]["code"], "temporary_session_cannot_issue_durable_credential",
                "{path}"
            );
        }
    }
    assert_eq!(
        request(
            state.clone(),
            "POST",
            &registry_path,
            Some(json!({"name":"test"})),
            Some(csrf.expose_secret()),
            None
        )
        .await?
        .0,
        StatusCode::SERVICE_UNAVAILABLE,
        "regular user still reaches the provider"
    );
    assert_eq!(
        request(
            state.clone(),
            "POST",
            &syouyu_path,
            Some(json!({"name":"test","permissions":["read"]})),
            Some(csrf.expose_secret()),
            None
        )
        .await?
        .0,
        StatusCode::NOT_FOUND,
        "regular user still reaches the bucket lookup"
    );
    assert_eq!(
        request(
            state.clone(),
            "DELETE",
            &format!("{roles}/{role_principal}/sessions/{provider_session_id}"),
            None,
            Some(csrf.expose_secret()),
            None
        )
        .await?
        .0,
        StatusCode::NO_CONTENT
    );
    store
        .delete_binding(member_account, role_provider_binding)
        .await?;

    let timed_session = json!({"role_arn":role_arn,"session_name":"timed","session_policy":{
        "Version":"2012-10-17", "Statement":{"Effect":"Allow","Action":"project:List","Resource":"*",
        "Condition":{"DateLessThan":{"aws:CurrentTime":"2100-01-01T00:00:00Z"}}}}});
    sqlx::query("UPDATE cloud_security_activation SET iam_conditions_enabled = false WHERE singleton = true")
        .execute(store.pool()).await?;
    assert_eq!(
        request(
            state.clone(),
            "POST",
            &assume_path,
            Some(timed_session.clone()),
            None,
            Some(source_key)
        )
        .await?
        .0,
        StatusCode::BAD_REQUEST
    );
    sqlx::query(
        "UPDATE cloud_security_activation SET iam_conditions_enabled = true WHERE singleton = true",
    )
    .execute(store.pool())
    .await?;
    let (status, timed_credentials) = request(
        state.clone(),
        "POST",
        &assume_path,
        Some(timed_session),
        None,
        Some(source_key),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    let timed_session_id = timed_credentials["session_id"]
        .as_str()
        .ok_or("timed session")?;
    assert_eq!(
        request(
            state.clone(),
            "DELETE",
            &format!("{roles}/{role_principal}/sessions/{timed_session_id}"),
            None,
            Some(csrf.expose_secret()),
            None
        )
        .await?
        .0,
        StatusCode::NO_CONTENT
    );
    let (status, credentials) = request(
        state.clone(),
        "POST",
        &assume_path,
        Some(assume_body.clone()),
        None,
        Some(source_key),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    let role_token = credentials["access_token"].as_str().ok_or("role token")?;
    let session_id = credentials["session_id"].as_str().ok_or("role session")?;
    assert_eq!(
        request(state.clone(), "GET", &roles, None, None, Some(role_token))
            .await?
            .0,
        StatusCode::FORBIDDEN
    );
    let (_, expiring) = request(
        state.clone(),
        "POST",
        &assume_path,
        Some(assume_body.clone()),
        None,
        Some(source_key),
    )
    .await?;
    let expired_token = expiring["access_token"]
        .as_str()
        .ok_or("expiring role token")?;
    let expired_id = uuid::Uuid::parse_str(
        expiring["session_id"]
            .as_str()
            .ok_or("expiring role session")?,
    )?;
    sqlx::query(
        "UPDATE iam_role_sessions SET expires_at = now() - interval '1 second' WHERE id=$1",
    )
    .bind(expired_id)
    .execute(store.pool())
    .await?;
    assert_eq!(
        request(
            state.clone(),
            "GET",
            &projects,
            None,
            None,
            Some(expired_token)
        )
        .await?
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, sessions) = request(
        state.clone(),
        "GET",
        &format!("{roles}/{role_principal}/sessions"),
        None,
        None,
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert!(
        sessions["items"]
            .as_array()
            .ok_or("session list")?
            .iter()
            .any(|item| item["id"] == session_id)
    );
    assert!(!sessions.to_string().contains(role_token));
    let switch_body = json!({"role_arn":role_arn, "session_name":"browser-role", "switch_console":true,
        "session_policy":{"Version":"2012-10-17","Statement":{"Effect":"Allow","Action":"project:List","Resource":"*"}}});
    assert_eq!(
        request(
            state.clone(),
            "POST",
            &assume_path,
            Some(switch_body.clone()),
            Some(csrf.expose_secret()),
            None
        )
        .await?
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    sqlx::query(
        "UPDATE cloud_security_activation SET browser_roles_enabled=true WHERE singleton=true",
    )
    .execute(store.pool())
    .await?;
    let (status, switched) = request(
        state.clone(),
        "POST",
        &assume_path,
        Some(switch_body),
        Some(csrf.expose_secret()),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(switched["switched"], true);
    assert!(switched.get("access_token").is_none());
    let browser_role_id = uuid::Uuid::parse_str(
        switched["session_id"]
            .as_str()
            .ok_or("browser role session")?,
    )?;
    let (_, browser_session) = request(
        state.clone(),
        "GET",
        "/api/v1/auth/session",
        None,
        None,
        None,
    )
    .await?;
    assert_eq!(browser_session["active_role"]["organization_id"], member_id);
    assert_eq!(browser_session["active_role"]["active"], true);
    assert_eq!(
        request(state.clone(), "GET", &projects, None, None, None)
            .await?
            .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            state.clone(),
            "GET",
            &format!("{base}/projects"),
            None,
            None,
            None
        )
        .await?
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            state.clone(),
            "GET",
            &format!("/api/v1/organizations/{member_id}/audit-events"),
            None,
            None,
            None
        )
        .await?
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            state.clone(),
            "POST",
            &projects,
            Some(json!({"name":"NoCsrf","slug":"no-csrf"})),
            None,
            None
        )
        .await?
        .0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("UPDATE iam_role_sessions SET expires_at=now() - interval '1 second' WHERE id=$1")
        .bind(browser_role_id)
        .execute(store.pool())
        .await?;
    assert_eq!(
        request(
            state.clone(),
            "GET",
            &format!("{base}/projects"),
            None,
            None,
            None
        )
        .await?
        .0,
        StatusCode::FORBIDDEN
    );
    let (_, expired_browser) = request(
        state.clone(),
        "GET",
        "/api/v1/auth/session",
        None,
        None,
        None,
    )
    .await?;
    assert_eq!(expired_browser["active_role"]["active"], false);
    assert_eq!(
        request(
            state.clone(),
            "POST",
            "/api/v1/auth/role/exit",
            None,
            None,
            None
        )
        .await?
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            state.clone(),
            "POST",
            "/api/v1/auth/role/exit",
            None,
            Some(csrf.expose_secret()),
            None
        )
        .await?
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(
            state.clone(),
            "GET",
            &format!("{base}/projects"),
            None,
            None,
            None
        )
        .await?
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            state.clone(),
            "GET",
            &projects,
            None,
            None,
            Some(role_token)
        )
        .await?
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            state.clone(),
            "POST",
            &projects,
            Some(json!({"name":"Denied","slug":"denied"})),
            None,
            Some(role_token)
        )
        .await?
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            state.clone(),
            "GET",
            &format!("/api/v1/organizations/{member_id}/audit-events"),
            None,
            None,
            Some(role_token)
        )
        .await?
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            state.clone(),
            "GET",
            &format!("{base}/projects"),
            None,
            None,
            Some(role_token)
        )
        .await?
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            state.clone(),
            "POST",
            &format!("/api/v1/organizations/{member_id}/sts/assume-role"),
            Some(json!({"role_arn":role_arn,"session_name":"chain","duration_seconds":7200})),
            None,
            Some(role_token)
        )
        .await?
        .0,
        StatusCode::BAD_REQUEST
    );
    let denied_trust = json!({"Version":"2012-10-17","Statement":[
        {"Effect":"Allow","Action":"sts:AssumeRole","Principal":{"AWS":format!("arn:aws:iam::{source_number}:root")}},
        {"Effect":"Deny","Action":"sts:AssumeRole","Principal":{"AWS":format!("arn:aws:iam::{source_number}:user/{}",source_robot.id)}}]});
    assert_eq!(
        request(
            state.clone(),
            "PUT",
            &format!("{roles}/{role_principal}/trust"),
            Some(json!({"trust_policy":denied_trust})),
            Some(csrf.expose_secret()),
            None
        )
        .await?
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(
            state.clone(),
            "POST",
            &assume_path,
            Some(assume_body),
            None,
            Some(source_key)
        )
        .await?
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            state.clone(),
            "GET",
            &projects,
            None,
            None,
            Some(role_token)
        )
        .await?
        .0,
        StatusCode::OK
    );
    store.delete_binding(member_account, role_binding).await?;
    assert_eq!(
        request(
            state.clone(),
            "DELETE",
            &format!("{roles}/{role_principal}"),
            None,
            Some(csrf.expose_secret()),
            None
        )
        .await?
        .0,
        StatusCode::BAD_REQUEST
    );
    let role_binding = store
        .create_binding(member_account, role_principal, role_policy.id)
        .await?;
    assert_eq!(
        request(
            state.clone(),
            "DELETE",
            &format!("{roles}/{role_principal}/sessions/{session_id}"),
            None,
            Some(csrf.expose_secret()),
            None
        )
        .await?
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(
            state.clone(),
            "DELETE",
            &format!("{roles}/{role_principal}"),
            None,
            Some(csrf.expose_secret()),
            None
        )
        .await?
        .0,
        StatusCode::BAD_REQUEST
    );
    store.delete_binding(member_account, role_binding).await?;
    assert_eq!(
        request(
            state.clone(),
            "DELETE",
            &format!("{roles}/{role_principal}"),
            None,
            Some(csrf.expose_secret()),
            None
        )
        .await?
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(
            state.clone(),
            "GET",
            &projects,
            None,
            None,
            Some(role_token)
        )
        .await?
        .0,
        StatusCode::UNAUTHORIZED
    );
    let management_principal = owner
        .memberships
        .first()
        .ok_or("management membership")?
        .principal_id;
    let management_path = format!("/api/v1/organizations/{management}/projects");
    let cli_hash = [9_u8; 32];
    sqlx::query("INSERT INTO cli_access_tokens(id,user_id,organization_id,prefix,token_hash,expires_at) VALUES ($1,$2,$3,'0123456789abcdef',$4,now()+interval '1 hour')")
        .bind(uuid::Uuid::now_v7()).bind(owner.user.id.0).bind(management.0).bind(cli_hash.as_slice())
        .execute(store.pool()).await?;
    assert!(
        store
            .authenticate_cli_access_token("0123456789abcdef", &cli_hash)
            .await?
            .is_some()
    );
    sqlx::query("UPDATE principals SET enabled=false WHERE id=$1")
        .bind(management_principal.0)
        .execute(store.pool())
        .await?;
    assert_eq!(
        request(state.clone(), "GET", &management_path, None, None, None)
            .await?
            .0,
        StatusCode::FORBIDDEN
    );
    assert!(
        store
            .authenticate_cli_access_token("0123456789abcdef", &cli_hash)
            .await?
            .is_none()
    );
    let (_, session_without_disabled_membership) = request(
        state.clone(),
        "GET",
        "/api/v1/auth/session",
        None,
        None,
        None,
    )
    .await?;
    assert!(
        !session_without_disabled_membership["memberships"]
            .as_array()
            .ok_or("memberships")?
            .iter()
            .any(|item| item["organization_id"] == management.to_string())
    );
    sqlx::query("UPDATE principals SET enabled=true WHERE id=$1")
        .bind(management_principal.0)
        .execute(store.pool())
        .await?;
    assert_eq!(
        request(state.clone(), "GET", &management_path, None, None, None)
            .await?
            .0,
        StatusCode::OK
    );
    assert!(
        store
            .authenticate_cli_access_token("0123456789abcdef", &cli_hash)
            .await?
            .is_some()
    );
    sqlx::query("UPDATE users SET status='suspended' WHERE id=$1")
        .bind(owner.user.id.0)
        .execute(store.pool())
        .await?;
    assert_eq!(
        request(
            state.clone(),
            "GET",
            "/api/v1/auth/session",
            None,
            None,
            None
        )
        .await?
        .0,
        StatusCode::UNAUTHORIZED
    );
    Ok(())
}
