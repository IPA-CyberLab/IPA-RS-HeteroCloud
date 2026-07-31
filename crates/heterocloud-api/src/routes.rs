use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
    routing::{get, post},
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use chrono::{Duration as ChronoDuration, Utc};
use email_address::EmailAddress;
use heterocloud_auth::{
    constant_time_token_eq, csrf_token, generate_token, hash_password, token_hash, verify_password,
};
use heterocloud_domain::{
    FlowSpec, OrganizationId, PolicyDocument, PolicyId, PrincipalId, ProjectId, ServiceInstanceId,
    UserStatus,
};
use heterocloud_iam::{AuthorizationRequest, Decision, authorize, semantics_digest};
use heterocloud_store::{
    AuditEvent, AuthorizationContext, RegisterWithInvitation, SessionUser, Store,
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use time::Duration as CookieDuration;
use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::{config::RuntimeConfig, error::ApiError};

const SESSION_COOKIE: &str = "hc_session";
const CSRF_HEADER: &str = "x-heterocloud-csrf";

#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    pub config: RuntimeConfig,
    pub registration_limiter: Arc<Semaphore>,
}

pub fn api_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health/live", get(live))
        .route("/health/ready", get(ready))
        .route("/auth/login", post(login))
        .route("/auth/register", post(register))
        .route("/auth/session", get(session))
        .route("/auth/logout", post(logout))
        .route("/organizations", get(list_organizations))
        .route(
            "/organizations/{organization_id}/projects",
            get(list_projects).post(create_project),
        )
        .route(
            "/organizations/{organization_id}/iam/principals",
            get(list_principals).post(create_service_account),
        )
        .route(
            "/organizations/{organization_id}/iam/policies",
            get(list_policies).post(create_policy),
        )
        .route(
            "/organizations/{organization_id}/iam/bindings",
            post(create_binding),
        )
        .route(
            "/organizations/{organization_id}/iam/principals/{principal_id}/api-keys",
            get(list_api_keys).post(create_api_key),
        )
        .route(
            "/organizations/{organization_id}/invitations",
            post(create_invitation),
        )
        .route(
            "/organizations/{organization_id}/flow/instances",
            get(list_flow_instances).post(create_flow_instance),
        )
        .route(
            "/organizations/{organization_id}/audit-events",
            get(list_audit_events),
        )
        .with_state(state)
}

async fn live() -> impl IntoResponse {
    (StatusCode::OK, Json(json!({ "status": "ok" })))
}

async fn ready(State(state): State<Arc<AppState>>) -> Result<impl IntoResponse, ApiError> {
    state.store.ping().await.map_err(ApiError::from_store)?;
    Ok((StatusCode::OK, Json(json!({ "status": "ready" }))))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginRequest {
    email: String,
    password: SecretString,
}

async fn login(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(request): Json<LoginRequest>,
) -> Result<impl IntoResponse, ApiError> {
    require_same_origin(&state.config, &headers)?;
    if !EmailAddress::is_valid(&request.email) {
        return Err(ApiError::BadRequest("Invalid email address.".into()));
    }
    let password_user = state
        .store
        .password_user_by_email(&request.email)
        .await
        .map_err(ApiError::from_store)?;
    let Some(password_user) = password_user else {
        return Err(ApiError::Unauthorized);
    };
    if password_user.user.status != UserStatus::Active
        || !verify_password(&request.password, &password_user.password_hash)
    {
        return Err(ApiError::Unauthorized);
    }

    let token = generate_token().map_err(|_| ApiError::Internal)?;
    let token_digest = token_hash(token.expose_secret());
    let expires_at = Utc::now()
        + ChronoDuration::from_std(state.config.session_ttl).map_err(|_| ApiError::Internal)?;
    state
        .store
        .create_session(password_user.user.id, &token_digest, expires_at)
        .await
        .map_err(ApiError::from_store)?;
    let session_user = state
        .store
        .session_user(password_user.user.id)
        .await
        .map_err(ApiError::from_store)?
        .ok_or(ApiError::Internal)?;
    let csrf = csrf_token(token.expose_secret(), &state.config.csrf_key)
        .map_err(|_| ApiError::Internal)?;
    let cookie = session_cookie(
        token.expose_secret().to_owned(),
        state.config.secure_cookie,
        state.config.session_ttl.as_secs(),
    );
    Ok((
        jar.add(cookie),
        Json(SessionResponse::new(session_user, csrf)),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegisterRequest {
    invitation_code: SecretString,
    email: String,
    display_name: String,
    password: SecretString,
}

async fn register(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(request): Json<RegisterRequest>,
) -> Result<impl IntoResponse, ApiError> {
    require_same_origin(&state.config, &headers)?;
    let _permit = state
        .registration_limiter
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError::TooManyRequests)?;
    if !EmailAddress::is_valid(&request.email) {
        return Err(ApiError::BadRequest("Invalid email address.".into()));
    }
    validate_name(&request.display_name)?;
    let password_hash = hash_password(&request.password)
        .map_err(|error| ApiError::BadRequest(error.to_string()))?;
    let invitation_hash = token_hash(request.invitation_code.expose_secret());
    let session_user = state
        .store
        .register_with_invitation(RegisterWithInvitation {
            code_hash: &invitation_hash,
            email: &request.email,
            display_name: &request.display_name,
            password_hash: &password_hash,
        })
        .await
        .map_err(ApiError::from_store)?;
    issue_session(&state, jar, session_user).await
}

async fn session(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
) -> Result<Json<SessionResponse>, ApiError> {
    let authenticated = authenticated_session(&state, &jar).await?;
    Ok(Json(SessionResponse::new(
        authenticated.user,
        authenticated.csrf,
    )))
}

async fn logout(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<impl IntoResponse, ApiError> {
    require_same_origin(&state.config, &headers)?;
    let authenticated = authenticated_session(&state, &jar).await?;
    require_csrf(&headers, &authenticated.csrf)?;
    state
        .store
        .delete_session(&authenticated.token_hash)
        .await
        .map_err(ApiError::from_store)?;
    let removal = Cookie::build((SESSION_COOKIE, ""))
        .path("/")
        .http_only(true)
        .secure(state.config.secure_cookie)
        .same_site(SameSite::Lax)
        .max_age(CookieDuration::ZERO)
        .build();
    Ok((jar.remove(removal), StatusCode::NO_CONTENT))
}

async fn list_organizations(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
) -> Result<Json<Value>, ApiError> {
    let authenticated = authenticated_session(&state, &jar).await?;
    let organizations = state
        .store
        .list_organizations(authenticated.user.user.id)
        .await
        .map_err(ApiError::from_store)?;
    Ok(Json(json!({ "items": organizations })))
}

async fn list_projects(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<Uuid>,
    jar: CookieJar,
) -> Result<Json<Value>, ApiError> {
    let authenticated = authenticated_session(&state, &jar).await?;
    authorize_organization(
        &state,
        &authenticated.user,
        OrganizationId(organization_id),
        "project:List",
        &organization_resource(organization_id, "project/*"),
    )
    .await?;
    let projects = state
        .store
        .list_projects(OrganizationId(organization_id))
        .await
        .map_err(ApiError::from_store)?;
    Ok(Json(json!({ "items": projects })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateProject {
    slug: String,
    name: String,
}

async fn create_project(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(request): Json<CreateProject>,
) -> Result<impl IntoResponse, ApiError> {
    let authenticated = authenticated_mutation(&state, &headers, &jar).await?;
    validate_slug(&request.slug)?;
    validate_name(&request.name)?;
    authorize_organization(
        &state,
        &authenticated.user,
        OrganizationId(organization_id),
        "project:Create",
        &organization_resource(organization_id, "project/*"),
    )
    .await?;
    let project = state
        .store
        .create_project(
            OrganizationId(organization_id),
            &request.slug,
            &request.name,
        )
        .await
        .map_err(ApiError::from_store)?;
    Ok((StatusCode::CREATED, Json(project)))
}

async fn list_principals(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<Uuid>,
    jar: CookieJar,
) -> Result<Json<Value>, ApiError> {
    let authenticated = authenticated_session(&state, &jar).await?;
    authorize_organization(
        &state,
        &authenticated.user,
        OrganizationId(organization_id),
        "iam:ListPrincipals",
        &organization_resource(organization_id, "iam/principal/*"),
    )
    .await?;
    let items = state
        .store
        .list_principals(OrganizationId(organization_id))
        .await
        .map_err(ApiError::from_store)?;
    Ok(Json(json!({ "items": items })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateServiceAccount {
    name: String,
}

async fn create_service_account(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(request): Json<CreateServiceAccount>,
) -> Result<impl IntoResponse, ApiError> {
    let authenticated = authenticated_mutation(&state, &headers, &jar).await?;
    validate_name(&request.name)?;
    authorize_organization(
        &state,
        &authenticated.user,
        OrganizationId(organization_id),
        "iam:CreatePrincipal",
        &organization_resource(organization_id, "iam/principal/*"),
    )
    .await?;
    let principal = state
        .store
        .create_service_account(OrganizationId(organization_id), &request.name)
        .await
        .map_err(ApiError::from_store)?;
    Ok((StatusCode::CREATED, Json(principal)))
}

async fn list_policies(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<Uuid>,
    jar: CookieJar,
) -> Result<Json<Value>, ApiError> {
    let authenticated = authenticated_session(&state, &jar).await?;
    authorize_organization(
        &state,
        &authenticated.user,
        OrganizationId(organization_id),
        "iam:ListPolicies",
        &organization_resource(organization_id, "iam/policy/*"),
    )
    .await?;
    let items = state
        .store
        .list_policies(OrganizationId(organization_id))
        .await
        .map_err(ApiError::from_store)?;
    Ok(Json(json!({ "items": items })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreatePolicy {
    name: String,
    document: PolicyDocument,
}

async fn create_policy(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(request): Json<CreatePolicy>,
) -> Result<impl IntoResponse, ApiError> {
    let authenticated = authenticated_mutation(&state, &headers, &jar).await?;
    validate_name(&request.name)?;
    request
        .document
        .validate()
        .map_err(|error| ApiError::BadRequest(error.to_string()))?;
    authorize_organization(
        &state,
        &authenticated.user,
        OrganizationId(organization_id),
        "iam:CreatePolicy",
        &organization_resource(organization_id, "iam/policy/*"),
    )
    .await?;
    let policy = state
        .store
        .create_policy(
            OrganizationId(organization_id),
            &request.name,
            &request.document,
            &semantics_digest(),
        )
        .await
        .map_err(ApiError::from_store)?;
    Ok((StatusCode::CREATED, Json(policy)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateBinding {
    principal_id: Uuid,
    policy_id: Uuid,
}

async fn create_binding(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(request): Json<CreateBinding>,
) -> Result<impl IntoResponse, ApiError> {
    let authenticated = authenticated_mutation(&state, &headers, &jar).await?;
    authorize_organization(
        &state,
        &authenticated.user,
        OrganizationId(organization_id),
        "iam:CreateBinding",
        &organization_resource(organization_id, "iam/binding/*"),
    )
    .await?;
    let id = state
        .store
        .create_binding(
            OrganizationId(organization_id),
            PrincipalId(request.principal_id),
            PolicyId(request.policy_id),
        )
        .await
        .map_err(ApiError::from_store)?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

async fn list_api_keys(
    State(state): State<Arc<AppState>>,
    Path((organization_id, principal_id)): Path<(Uuid, Uuid)>,
    jar: CookieJar,
) -> Result<Json<Value>, ApiError> {
    let authenticated = authenticated_session(&state, &jar).await?;
    authorize_organization(
        &state,
        &authenticated.user,
        OrganizationId(organization_id),
        "iam:ListApiKeys",
        &organization_resource(organization_id, "iam/api-key/*"),
    )
    .await?;
    let items = state
        .store
        .list_api_keys(OrganizationId(organization_id), PrincipalId(principal_id))
        .await
        .map_err(ApiError::from_store)?;
    Ok(Json(json!({ "items": items })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateApiKey {
    name: String,
    expires_in_days: Option<i64>,
}

async fn create_api_key(
    State(state): State<Arc<AppState>>,
    Path((organization_id, principal_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(request): Json<CreateApiKey>,
) -> Result<impl IntoResponse, ApiError> {
    let authenticated = authenticated_mutation(&state, &headers, &jar).await?;
    validate_name(&request.name)?;
    if request
        .expires_in_days
        .is_some_and(|days| !(1..=365).contains(&days))
    {
        return Err(ApiError::BadRequest(
            "expires_in_days must be between 1 and 365".into(),
        ));
    }
    authorize_organization(
        &state,
        &authenticated.user,
        OrganizationId(organization_id),
        "iam:CreateApiKey",
        &organization_resource(organization_id, "iam/api-key/*"),
    )
    .await?;
    let prefix = Uuid::now_v7().simple().to_string()[..16].to_owned();
    let secret = generate_token().map_err(|_| ApiError::Internal)?;
    let api_key = format!("hc_{prefix}_{}", secret.expose_secret());
    let api_key_hash = token_hash(&api_key);
    let expires_at = request
        .expires_in_days
        .map(|days| Utc::now() + ChronoDuration::days(days));
    let id = state
        .store
        .create_api_key(
            OrganizationId(organization_id),
            PrincipalId(principal_id),
            &request.name,
            &prefix,
            &api_key_hash,
            expires_at,
        )
        .await
        .map_err(ApiError::from_store)?;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "id": id,
            "name": request.name,
            "prefix": prefix,
            "api_key": api_key,
            "expires_at": expires_at,
        })),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateInvitation {
    #[serde(default = "default_invitation_max_uses")]
    max_uses: i32,
    #[serde(default = "default_invitation_ttl_hours")]
    expires_in_hours: i64,
}

async fn create_invitation(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(request): Json<CreateInvitation>,
) -> Result<impl IntoResponse, ApiError> {
    let authenticated = authenticated_mutation(&state, &headers, &jar).await?;
    if !(1..=100).contains(&request.max_uses) || !(1..=168).contains(&request.expires_in_hours) {
        return Err(ApiError::BadRequest(
            "max_uses must be 1..100 and expires_in_hours must be 1..168".into(),
        ));
    }
    authorize_organization(
        &state,
        &authenticated.user,
        OrganizationId(organization_id),
        "identity:CreateInvitation",
        &organization_resource(organization_id, "identity/invitation/*"),
    )
    .await?;
    let code = generate_token().map_err(|_| ApiError::Internal)?;
    let code_hash = token_hash(code.expose_secret());
    let expires_at = Utc::now() + ChronoDuration::hours(request.expires_in_hours);
    let id = state
        .store
        .create_invitation(
            OrganizationId(organization_id),
            authenticated.user.user.id,
            &code_hash,
            request.max_uses,
            expires_at,
        )
        .await
        .map_err(ApiError::from_store)?;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "id": id,
            "code": code.expose_secret(),
            "max_uses": request.max_uses,
            "expires_at": expires_at,
        })),
    ))
}

#[derive(Default, Deserialize)]
struct FlowListQuery {
    project_id: Option<Uuid>,
}

async fn list_flow_instances(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<Uuid>,
    Query(query): Query<FlowListQuery>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Json<Value>, ApiError> {
    let actor = authenticated_actor(&state, &headers, &jar).await?;
    authorize_actor(
        &state,
        &actor,
        OrganizationId(organization_id),
        "flow:ListInstances",
        &organization_resource(organization_id, "flow/*"),
    )
    .await?;
    let items = state
        .store
        .list_service_instances(
            OrganizationId(organization_id),
            query.project_id.map(ProjectId),
            Some("flow"),
        )
        .await
        .map_err(ApiError::from_store)?;
    Ok(Json(json!({ "items": items })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateFlowInstance {
    project_id: Uuid,
    name: String,
    spec: FlowSpec,
}

async fn create_flow_instance(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(request): Json<CreateFlowInstance>,
) -> Result<impl IntoResponse, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    validate_name(&request.name)?;
    if request.spec.max_participants == 0 || request.spec.max_participants > 100_000 {
        return Err(ApiError::BadRequest(
            "max_participants must be between 1 and 100000".into(),
        ));
    }
    let authorization = authorize_actor(
        &state,
        &actor,
        OrganizationId(organization_id),
        "flow:CreateInstance",
        &organization_resource(organization_id, "flow/*"),
    )
    .await?;
    let instance = state
        .store
        .create_service_instance(
            OrganizationId(organization_id),
            ProjectId(request.project_id),
            authorization.principal_id,
            "flow",
            &request.name,
            serde_json::to_value(request.spec).map_err(|_| ApiError::Internal)?,
        )
        .await
        .map_err(ApiError::from_store)?;
    Ok((StatusCode::ACCEPTED, Json(instance)))
}

#[derive(Default, Deserialize)]
struct AuditQuery {
    limit: Option<i64>,
}

async fn list_audit_events(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<Uuid>,
    Query(query): Query<AuditQuery>,
    jar: CookieJar,
) -> Result<Json<Value>, ApiError> {
    let authenticated = authenticated_session(&state, &jar).await?;
    authorize_organization(
        &state,
        &authenticated.user,
        OrganizationId(organization_id),
        "audit:ListEvents",
        &organization_resource(organization_id, "audit/*"),
    )
    .await?;
    let items = state
        .store
        .list_audit_events(OrganizationId(organization_id), query.limit.unwrap_or(100))
        .await
        .map_err(ApiError::from_store)?;
    Ok(Json(json!({ "items": items })))
}

struct AuthenticatedSession {
    user: SessionUser,
    csrf: SecretString,
    token_hash: [u8; 32],
}

enum AuthenticatedActor {
    User(AuthenticatedSession),
    ApiKey {
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        api_key_id: Uuid,
    },
}

async fn issue_session(
    state: &AppState,
    jar: CookieJar,
    session_user: SessionUser,
) -> Result<(CookieJar, Json<SessionResponse>), ApiError> {
    let token = generate_token().map_err(|_| ApiError::Internal)?;
    let digest = token_hash(token.expose_secret());
    let expires_at = Utc::now()
        + ChronoDuration::from_std(state.config.session_ttl).map_err(|_| ApiError::Internal)?;
    state
        .store
        .create_session(session_user.user.id, &digest, expires_at)
        .await
        .map_err(ApiError::from_store)?;
    let csrf = csrf_token(token.expose_secret(), &state.config.csrf_key)
        .map_err(|_| ApiError::Internal)?;
    let cookie = session_cookie(
        token.expose_secret().to_owned(),
        state.config.secure_cookie,
        state.config.session_ttl.as_secs(),
    );
    Ok((
        jar.add(cookie),
        Json(SessionResponse::new(session_user, csrf)),
    ))
}

async fn authenticated_session(
    state: &AppState,
    jar: &CookieJar,
) -> Result<AuthenticatedSession, ApiError> {
    let raw_token = jar
        .get(SESSION_COOKIE)
        .map(Cookie::value)
        .ok_or(ApiError::Unauthorized)?;
    let digest = token_hash(raw_token);
    let user = state
        .store
        .session_user_by_token_hash(&digest)
        .await
        .map_err(ApiError::from_store)?
        .ok_or(ApiError::Unauthorized)?;
    let csrf = csrf_token(raw_token, &state.config.csrf_key).map_err(|_| ApiError::Internal)?;
    Ok(AuthenticatedSession {
        user,
        csrf,
        token_hash: digest,
    })
}

async fn authenticated_mutation(
    state: &AppState,
    headers: &HeaderMap,
    jar: &CookieJar,
) -> Result<AuthenticatedSession, ApiError> {
    require_same_origin(&state.config, headers)?;
    let authenticated = authenticated_session(state, jar).await?;
    require_csrf(headers, &authenticated.csrf)?;
    Ok(authenticated)
}

async fn authenticated_actor(
    state: &AppState,
    headers: &HeaderMap,
    jar: &CookieJar,
) -> Result<AuthenticatedActor, ApiError> {
    if let Some(value) = headers.get(header::AUTHORIZATION) {
        let authorization = value.to_str().map_err(|_| ApiError::Unauthorized)?;
        let token = authorization
            .strip_prefix("Bearer ")
            .ok_or(ApiError::Unauthorized)?;
        let prefix = parse_api_key_prefix(token)?;
        let digest = token_hash(token);
        let principal = state
            .store
            .authenticate_api_key(prefix, &digest)
            .await
            .map_err(ApiError::from_store)?
            .ok_or(ApiError::Unauthorized)?;
        return Ok(AuthenticatedActor::ApiKey {
            organization_id: OrganizationId(principal.organization_id),
            principal_id: PrincipalId(principal.principal_id),
            api_key_id: principal.api_key_id,
        });
    }
    Ok(AuthenticatedActor::User(
        authenticated_session(state, jar).await?,
    ))
}

async fn authenticated_actor_mutation(
    state: &AppState,
    headers: &HeaderMap,
    jar: &CookieJar,
) -> Result<AuthenticatedActor, ApiError> {
    let actor = authenticated_actor(state, headers, jar).await?;
    if let AuthenticatedActor::User(session) = &actor {
        require_same_origin(&state.config, headers)?;
        require_csrf(headers, &session.csrf)?;
    }
    Ok(actor)
}

async fn authorize_organization(
    state: &AppState,
    session: &SessionUser,
    organization_id: OrganizationId,
    action: &str,
    resource: &str,
) -> Result<AuthorizationContext, ApiError> {
    let context = state
        .store
        .authorization_context(session.user.id, organization_id)
        .await
        .map_err(ApiError::from_store)?
        .ok_or(ApiError::Forbidden)?;
    let (decision, reason) = if context.role == "owner" {
        (Decision::Allow, "organization_owner")
    } else {
        let evaluation = authorize(
            &AuthorizationRequest {
                principal_organization_id: organization_id,
                resource_organization_id: organization_id,
                action,
                resource,
            },
            &context.policies,
        )
        .map_err(|_| ApiError::Internal)?;
        (evaluation.decision, evaluation.reason)
    };
    let request_id = Uuid::now_v7().to_string();
    state
        .store
        .append_audit(&AuditEvent {
            organization_id: Some(organization_id),
            principal_id: Some(context.principal_id),
            user_id: Some(session.user.id),
            request_id: &request_id,
            source_ip: None,
            action,
            resource,
            decision: match decision {
                Decision::Allow => "allow",
                Decision::Deny => "deny",
            },
            reason,
            metadata: json!({ "semantics_digest": semantics_digest() }),
        })
        .await
        .map_err(ApiError::from_store)?;
    if decision == Decision::Deny {
        return Err(ApiError::Forbidden);
    }
    Ok(context)
}

async fn authorize_actor(
    state: &AppState,
    actor: &AuthenticatedActor,
    organization_id: OrganizationId,
    action: &str,
    resource: &str,
) -> Result<AuthorizationContext, ApiError> {
    let (context, user_id, metadata) = match actor {
        AuthenticatedActor::User(session) => {
            let context = state
                .store
                .authorization_context(session.user.user.id, organization_id)
                .await
                .map_err(ApiError::from_store)?
                .ok_or(ApiError::Forbidden)?;
            (
                context,
                Some(session.user.user.id),
                json!({ "actor": "user" }),
            )
        }
        AuthenticatedActor::ApiKey {
            organization_id: key_organization_id,
            principal_id,
            api_key_id,
        } => {
            if *key_organization_id != organization_id {
                return Err(ApiError::Forbidden);
            }
            let context = state
                .store
                .authorization_context_for_principal(*principal_id, organization_id)
                .await
                .map_err(ApiError::from_store)?
                .ok_or(ApiError::Forbidden)?;
            (
                context,
                None,
                json!({ "actor": "api_key", "api_key_id": api_key_id }),
            )
        }
    };
    let (decision, reason) = if context.role == "owner" {
        (Decision::Allow, "organization_owner")
    } else {
        let evaluation = authorize(
            &AuthorizationRequest {
                principal_organization_id: organization_id,
                resource_organization_id: organization_id,
                action,
                resource,
            },
            &context.policies,
        )
        .map_err(|_| ApiError::Internal)?;
        (evaluation.decision, evaluation.reason)
    };
    let request_id = Uuid::now_v7().to_string();
    state
        .store
        .append_audit(&AuditEvent {
            organization_id: Some(organization_id),
            principal_id: Some(context.principal_id),
            user_id,
            request_id: &request_id,
            source_ip: None,
            action,
            resource,
            decision: match decision {
                Decision::Allow => "allow",
                Decision::Deny => "deny",
            },
            reason,
            metadata: json!({
                "semantics_digest": semantics_digest(),
                "authentication": metadata,
            }),
        })
        .await
        .map_err(ApiError::from_store)?;
    if decision == Decision::Deny {
        return Err(ApiError::Forbidden);
    }
    Ok(context)
}

fn require_same_origin(config: &RuntimeConfig, headers: &HeaderMap) -> Result<(), ApiError> {
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .ok_or(ApiError::Forbidden)?;
    if !config
        .allowed_origins
        .iter()
        .any(|allowed| allowed == origin)
    {
        return Err(ApiError::Forbidden);
    }
    Ok(())
}

fn require_csrf(headers: &HeaderMap, expected: &SecretString) -> Result<(), ApiError> {
    let supplied = headers
        .get(CSRF_HEADER)
        .and_then(|value| value.to_str().ok())
        .ok_or(ApiError::Forbidden)?;
    if !constant_time_token_eq(supplied, expected.expose_secret()) {
        return Err(ApiError::Forbidden);
    }
    Ok(())
}

fn parse_api_key_prefix(token: &str) -> Result<&str, ApiError> {
    let mut segments = token.splitn(3, '_');
    if segments.next() != Some("hc") {
        return Err(ApiError::Unauthorized);
    }
    let prefix = segments.next().ok_or(ApiError::Unauthorized)?;
    let secret = segments.next().ok_or(ApiError::Unauthorized)?;
    if prefix.len() != 16 || secret.len() < 32 {
        return Err(ApiError::Unauthorized);
    }
    Ok(prefix)
}

fn session_cookie(value: String, secure: bool, ttl_seconds: u64) -> Cookie<'static> {
    Cookie::build((SESSION_COOKIE, value))
        .path("/")
        .http_only(true)
        .secure(secure)
        .same_site(SameSite::Lax)
        .max_age(CookieDuration::seconds(
            i64::try_from(ttl_seconds).unwrap_or(i64::MAX),
        ))
        .build()
}

fn validate_name(name: &str) -> Result<(), ApiError> {
    let length = name.trim().chars().count();
    if !(1..=120).contains(&length) {
        return Err(ApiError::BadRequest(
            "name must contain between 1 and 120 characters".into(),
        ));
    }
    Ok(())
}

fn validate_slug(slug: &str) -> Result<(), ApiError> {
    let bytes = slug.as_bytes();
    let valid_length = (3..=63).contains(&bytes.len());
    let valid_start = bytes.first().is_some_and(u8::is_ascii_lowercase);
    let valid_end = bytes
        .last()
        .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit());
    let valid_chars = bytes
        .iter()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-');
    if !valid_length || !valid_start || !valid_end || !valid_chars {
        return Err(ApiError::BadRequest(
            "slug must be a 3..63 character lowercase DNS label".into(),
        ));
    }
    Ok(())
}

fn organization_resource(organization_id: Uuid, suffix: &str) -> String {
    format!("hc:org:{organization_id}:{suffix}")
}

const fn default_invitation_max_uses() -> i32 {
    1
}

const fn default_invitation_ttl_hours() -> i64 {
    24
}

#[derive(Serialize)]
struct SessionResponse {
    user: heterocloud_domain::User,
    memberships: Vec<heterocloud_store::Membership>,
    csrf_token: String,
}

impl SessionResponse {
    fn new(session: SessionUser, csrf_token: SecretString) -> Self {
        Self {
            user: session.user,
            memberships: session.memberships,
            csrf_token: csrf_token.expose_secret().to_owned(),
        }
    }
}

#[allow(dead_code)]
fn _service_id_type_guard(id: Uuid) -> ServiceInstanceId {
    ServiceInstanceId(id)
}

#[cfg(test)]
mod tests {
    use super::{CSRF_HEADER, SESSION_COOKIE, parse_api_key_prefix, validate_slug};

    #[test]
    fn public_security_names_are_stable() {
        assert_eq!(SESSION_COOKIE, "hc_session");
        assert_eq!(CSRF_HEADER, "x-heterocloud-csrf");
    }

    #[test]
    fn validates_dns_slugs() {
        assert!(validate_slug("realtime-prod").is_ok());
        assert!(validate_slug("-invalid").is_err());
        assert!(validate_slug("Invalid").is_err());
    }

    #[test]
    fn api_key_prefix_is_strictly_parsed() {
        let token = "hc_0123456789abcdef_0123456789abcdefghijklmnopqrstuvwxyzABCDEFG";
        assert_eq!(parse_api_key_prefix(token).ok(), Some("0123456789abcdef"));
        assert!(parse_api_key_prefix("not-a-key").is_err());
    }
}
