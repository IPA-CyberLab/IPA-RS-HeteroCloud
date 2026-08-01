use std::{
    collections::BTreeSet,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use chrono::{Duration as ChronoDuration, Utc};
use email_address::EmailAddress;
use heterocloud_auth::{
    constant_time_token_eq, csrf_token, generate_token, hash_password, token_hash, verify_password,
};
use heterocloud_domain::{
    FlowSpec, OrganizationId, PolicyDocument, PolicyId, PrincipalId, ProjectId, ServiceInstance,
    ServiceInstanceId, ServiceState, UserStatus,
};
use heterocloud_iam::{AuthorizationRequest, Decision, authorize, semantics_digest};
use heterocloud_store::{
    AuditEvent, AuthorizationContext, OidcUser, RegisterWithInvitation, SessionUser, Store,
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use time::Duration as CookieDuration;
use tokio::sync::Semaphore;
use url::Url;
use uuid::Uuid;

use crate::{
    config::RuntimeConfig,
    error::ApiError,
    flow_access::{FlowAccessInput, SignedFlowAccessContext},
    oidc::{
        OIDC_TRANSACTION_COOKIE, OidcCallbackQuery, OidcError, OidcLoginIntent,
        clear_transaction_cookie,
    },
};

const SESSION_COOKIE: &str = "hc_session";
const CSRF_HEADER: &str = "x-heterocloud-csrf";

#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    pub config: RuntimeConfig,
    pub flow_client: reqwest::Client,
    pub registration_limiter: Arc<Semaphore>,
}

pub fn api_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health/live", get(live))
        .route("/health/ready", get(ready))
        .route("/auth/login", post(login))
        .route("/auth/register", post(register))
        .route("/auth/oidc/start", get(oidc_start))
        .route("/auth/oidc/callback", get(oidc_callback))
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
            "/organizations/{organization_id}/realtime/services",
            get(list_realtime_services).post(create_realtime_service),
        )
        .route(
            "/organizations/{organization_id}/realtime/services/{service_instance_id}",
            get(get_realtime_service)
                .patch(update_realtime_service)
                .delete(delete_realtime_service),
        )
        .route(
            "/organizations/{organization_id}/realtime/services/{service_instance_id}/access-credentials",
            post(create_realtime_access_credential),
        )
        .route(
            "/organizations/{organization_id}/realtime/services/{service_instance_id}/metrics",
            get(get_realtime_service_metrics),
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
    if !EmailAddress::is_valid(&request.email) {
        return Err(ApiError::BadRequest("Invalid email address.".into()));
    }
    validate_name(&request.display_name)?;
    let invitation_hash = token_hash(request.invitation_code.expose_secret());
    if !state
        .store
        .invitation_available(&invitation_hash)
        .await
        .map_err(ApiError::from_store)?
    {
        return Err(ApiError::from_store(
            heterocloud_store::StoreError::InvitationUnavailable,
        ));
    }
    let _permit = state
        .registration_limiter
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError::TooManyRequests)?;
    let password_hash = hash_password(&request.password)
        .map_err(|error| ApiError::BadRequest(error.to_string()))?;
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

async fn oidc_start(
    State(state): State<Arc<AppState>>,
    Query(query): Query<OidcStartQuery>,
    jar: CookieJar,
) -> Result<impl IntoResponse, ApiError> {
    let oidc = state.config.oidc.as_ref().ok_or(ApiError::NotFound)?;
    let start = oidc
        .begin_login(
            &state.config.csrf_key,
            state.config.secure_cookie,
            query.intent.unwrap_or(OidcLoginIntent::Authenticate),
        )
        .await
        .map_err(oidc_api_error)?;
    Ok((
        jar.add(start.transaction_cookie),
        Redirect::to(start.authorization_url.as_str()),
    ))
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct OidcStartQuery {
    intent: Option<OidcLoginIntent>,
}

async fn oidc_callback(
    State(state): State<Arc<AppState>>,
    Query(query): Query<OidcCallbackQuery>,
    jar: CookieJar,
) -> Response {
    let transaction_cookie = jar
        .get(OIDC_TRANSACTION_COOKIE)
        .map(|cookie| cookie.value().to_owned());
    let jar = jar.remove(clear_transaction_cookie(state.config.secure_cookie));
    let result = async {
        let oidc = state.config.oidc.as_ref().ok_or(ApiError::NotFound)?;
        let identity = oidc
            .complete_login(
                &query,
                transaction_cookie.as_deref(),
                &state.config.csrf_key,
            )
            .await
            .map_err(oidc_api_error)?;
        if !EmailAddress::is_valid(&identity.email) {
            return Err(ApiError::Unauthorized);
        }
        validate_name(&identity.display_name)?;
        let session_user = state
            .store
            .find_or_create_oidc_user(OidcUser {
                issuer: &identity.issuer,
                subject: &identity.subject,
                email: &identity.email,
                display_name: &identity.display_name,
            })
            .await
            .map_err(ApiError::from_store)?;
        if session_user.user.status != UserStatus::Active {
            return Err(ApiError::Unauthorized);
        }
        let (cookie, _) = create_session_cookie(&state, session_user.user.id).await?;
        Ok::<_, ApiError>((jar.clone().add(cookie), Redirect::to("/")).into_response())
    }
    .await;
    match result {
        Ok(response) => response,
        Err(error) => (jar, error).into_response(),
    }
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
    validate_invitation_ttl(request.expires_in_hours)?;
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
            expires_at,
        )
        .await
        .map_err(ApiError::from_store)?;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "id": id,
            "code": code.expose_secret(),
            "max_uses": 1,
            "expires_at": expires_at,
        })),
    ))
}

#[derive(Default, Deserialize)]
struct FlowListQuery {
    project_id: Option<Uuid>,
}

async fn list_realtime_services(
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
        "realtime:ListServices",
        &organization_resource(organization_id, "realtime/*"),
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
struct CreateRealtimeService {
    project_id: Uuid,
    name: String,
    spec: FlowSpec,
}

async fn create_realtime_service(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(request): Json<CreateRealtimeService>,
) -> Result<impl IntoResponse, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    validate_name(&request.name)?;
    validate_flow_spec(&request.spec)?;
    let authorization = authorize_actor(
        &state,
        &actor,
        OrganizationId(organization_id),
        "realtime:CreateService",
        &organization_resource(organization_id, "realtime/*"),
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

async fn get_realtime_service(
    State(state): State<Arc<AppState>>,
    Path((organization_id, service_instance_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Json<ServiceInstance>, ApiError> {
    let actor = authenticated_actor(&state, &headers, &jar).await?;
    authorize_actor(
        &state,
        &actor,
        OrganizationId(organization_id),
        "realtime:GetService",
        &realtime_service_resource(organization_id, service_instance_id),
    )
    .await?;
    Ok(Json(
        realtime_service(&state, organization_id, service_instance_id).await?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateRealtimeService {
    name: Option<String>,
    spec: Option<FlowSpec>,
}

async fn update_realtime_service(
    State(state): State<Arc<AppState>>,
    Path((organization_id, service_instance_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(request): Json<UpdateRealtimeService>,
) -> Result<impl IntoResponse, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    if request.name.is_none() && request.spec.is_none() {
        return Err(ApiError::BadRequest("name or spec must be supplied".into()));
    }
    let current = realtime_service(&state, organization_id, service_instance_id).await?;
    let name = request.name.unwrap_or(current.name);
    validate_name(&name)?;
    let spec = match request.spec {
        Some(spec) => spec,
        None => serde_json::from_value(current.spec).map_err(|_| ApiError::Internal)?,
    };
    validate_flow_spec(&spec)?;
    let authorization = authorize_actor(
        &state,
        &actor,
        OrganizationId(organization_id),
        "realtime:UpdateService",
        &realtime_service_resource(organization_id, service_instance_id),
    )
    .await?;
    let service = state
        .store
        .update_service_instance(
            OrganizationId(organization_id),
            ServiceInstanceId(service_instance_id),
            authorization.principal_id,
            &name,
            serde_json::to_value(spec).map_err(|_| ApiError::Internal)?,
        )
        .await
        .map_err(ApiError::from_store)?;
    Ok((StatusCode::ACCEPTED, Json(service)))
}

async fn delete_realtime_service(
    State(state): State<Arc<AppState>>,
    Path((organization_id, service_instance_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<impl IntoResponse, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    let authorization = authorize_actor(
        &state,
        &actor,
        OrganizationId(organization_id),
        "realtime:DeleteService",
        &realtime_service_resource(organization_id, service_instance_id),
    )
    .await?;
    let service = state
        .store
        .begin_delete_service_instance(
            OrganizationId(organization_id),
            ServiceInstanceId(service_instance_id),
            authorization.principal_id,
        )
        .await
        .map_err(ApiError::from_store)?;
    Ok((StatusCode::ACCEPTED, Json(service)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateRealtimeAccessCredential {
    permissions: BTreeSet<String>,
    expires_in_seconds: Option<u64>,
}

#[derive(Serialize)]
struct FlowAccessHeaders {
    #[serde(rename = "x-flow-principal")]
    principal: String,
    #[serde(rename = "x-flow-timestamp")]
    timestamp: String,
    #[serde(rename = "x-flow-signature")]
    signature: String,
}

#[derive(Serialize)]
struct FlowAccessContextResponse {
    headers: FlowAccessHeaders,
    endpoints: Vec<Url>,
    issued_at: u64,
    expires_at: u64,
    context_id: Uuid,
    organization_id: OrganizationId,
    project_id: ProjectId,
    service_instance_id: ServiceInstanceId,
    principal_id: PrincipalId,
}

async fn create_realtime_access_credential(
    State(state): State<Arc<AppState>>,
    Path((organization_id, service_instance_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(request): Json<CreateRealtimeAccessCredential>,
) -> Result<impl IntoResponse, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    validate_flow_permissions(&request.permissions)?;
    let expires_in_seconds = validate_flow_access_ttl(request.expires_in_seconds)?;

    let instance = state
        .store
        .service_instance(ServiceInstanceId(service_instance_id))
        .await
        .map_err(ApiError::from_store)?;
    let instance = validate_flow_access_target(
        instance,
        OrganizationId(organization_id),
        ServiceInstanceId(service_instance_id),
    )?;
    let resource = realtime_service_resource(organization_id, service_instance_id);
    let authorization = authorize_actor(
        &state,
        &actor,
        OrganizationId(organization_id),
        "realtime:IssueAccessCredential",
        &resource,
    )
    .await?;
    for permission in &request.permissions {
        let action = flow_permission_iam_action(permission).ok_or(ApiError::Internal)?;
        authorize_actor(
            &state,
            &actor,
            OrganizationId(organization_id),
            action,
            &resource,
        )
        .await?;
    }

    let issued_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ApiError::Internal)?
        .as_secs();
    let expires_at = issued_at
        .checked_add(expires_in_seconds)
        .ok_or(ApiError::Internal)?;
    let signed = state
        .config
        .flow_access_signer
        .sign(
            FlowAccessInput {
                organization_id: instance.organization_id,
                project_id: instance.project_id,
                service_instance_id: instance.id,
                principal_id: authorization.principal_id,
                permissions: request.permissions,
            },
            issued_at,
            expires_at,
            Uuid::now_v7(),
        )
        .map_err(|_| ApiError::Internal)?;
    let response = flow_access_response(signed, &state.config.flow_public_endpoints);
    Ok((
        StatusCode::CREATED,
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::PRAGMA, "no-cache"),
        ],
        Json(response),
    ))
}

async fn get_realtime_service_metrics(
    State(state): State<Arc<AppState>>,
    Path((organization_id, service_instance_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Json<Value>, ApiError> {
    let actor = authenticated_actor(&state, &headers, &jar).await?;
    let instance = validate_flow_access_target(
        state
            .store
            .service_instance(ServiceInstanceId(service_instance_id))
            .await
            .map_err(ApiError::from_store)?,
        OrganizationId(organization_id),
        ServiceInstanceId(service_instance_id),
    )?;
    let authorization = authorize_actor(
        &state,
        &actor,
        OrganizationId(organization_id),
        "realtime:GetMetrics",
        &realtime_service_resource(organization_id, service_instance_id),
    )
    .await?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ApiError::Internal)?
        .as_secs();
    let signed = state
        .config
        .flow_access_signer
        .sign(
            FlowAccessInput {
                organization_id: instance.organization_id,
                project_id: instance.project_id,
                service_instance_id: instance.id,
                principal_id: authorization.principal_id,
                permissions: BTreeSet::from(["flow.metrics.read".to_owned()]),
            },
            now,
            now + 30,
            Uuid::now_v7(),
        )
        .map_err(|_| ApiError::Internal)?;
    let url = state
        .config
        .flow_internal_endpoint
        .join("v1/service-overview")
        .map_err(|_| ApiError::Internal)?;
    let response = state
        .flow_client
        .get(url)
        .header("x-flow-principal", signed.encoded)
        .header("x-flow-timestamp", signed.timestamp)
        .header("x-flow-signature", signed.signature)
        .send()
        .await
        .map_err(|error| {
            tracing::warn!(error = %error, "Flow metrics request failed");
            ApiError::RealtimeProviderUnavailable
        })?;
    if !response.status().is_success() {
        tracing::warn!(status = %response.status(), "Flow metrics request was rejected");
        return Err(ApiError::RealtimeProviderUnavailable);
    }
    let metrics = response.json().await.map_err(|error| {
        tracing::warn!(error = %error, "Flow metrics response was invalid");
        ApiError::RealtimeProviderUnavailable
    })?;
    Ok(Json(metrics))
}

fn flow_access_response(
    signed: SignedFlowAccessContext,
    flow_public_endpoints: &[Url],
) -> FlowAccessContextResponse {
    FlowAccessContextResponse {
        endpoints: flow_public_endpoints.to_vec(),
        issued_at: signed.context.issued_at,
        expires_at: signed.context.expires_at,
        context_id: signed.context.context_id,
        organization_id: signed.context.organization_id,
        project_id: signed.context.project_id,
        service_instance_id: signed.context.service_instance_id,
        principal_id: signed.context.principal_id,
        headers: FlowAccessHeaders {
            principal: signed.encoded,
            timestamp: signed.timestamp,
            signature: signed.signature,
        },
    }
}

fn validate_flow_permissions(permissions: &BTreeSet<String>) -> Result<(), ApiError> {
    if permissions.is_empty() {
        return Err(ApiError::BadRequest(
            "permissions must contain at least one permission".into(),
        ));
    }
    if permissions
        .iter()
        .any(|permission| permission.contains('*'))
    {
        return Err(ApiError::BadRequest(
            "wildcard Flow permissions are not allowed".into(),
        ));
    }
    if permissions
        .iter()
        .any(|permission| flow_permission_iam_action(permission).is_none())
    {
        return Err(ApiError::BadRequest(
            "permissions contains an unsupported Flow permission".into(),
        ));
    }
    Ok(())
}

fn flow_permission_iam_action(permission: &str) -> Option<&'static str> {
    match permission {
        "flow.queue.read" => Some("flow:QueueRead"),
        "flow.queue.write" => Some("flow:QueueWrite"),
        "flow.room.create" => Some("flow:RoomCreate"),
        "flow.room.read" => Some("flow:RoomRead"),
        "flow.room.join" => Some("flow:RoomJoin"),
        "flow.turn.issue" => Some("flow:TurnIssue"),
        "flow.signal.connect" => Some("flow:SignalConnect"),
        "flow.metrics.read" => Some("realtime:GetMetrics"),
        _ => None,
    }
}

fn validate_flow_access_ttl(expires_in_seconds: Option<u64>) -> Result<u64, ApiError> {
    let expires_in_seconds = expires_in_seconds.unwrap_or(FLOW_ACCESS_DEFAULT_TTL_SECONDS);
    if !(FLOW_ACCESS_MIN_TTL_SECONDS..=FLOW_ACCESS_MAX_TTL_SECONDS).contains(&expires_in_seconds) {
        return Err(ApiError::BadRequest(format!(
            "expires_in_seconds must be {FLOW_ACCESS_MIN_TTL_SECONDS}..{FLOW_ACCESS_MAX_TTL_SECONDS}"
        )));
    }
    Ok(expires_in_seconds)
}

fn validate_flow_access_target(
    instance: Option<ServiceInstance>,
    organization_id: OrganizationId,
    service_instance_id: ServiceInstanceId,
) -> Result<ServiceInstance, ApiError> {
    let instance = instance
        .filter(|instance| {
            instance.id == service_instance_id
                && instance.organization_id == organization_id
                && instance.provider == "flow"
        })
        .ok_or(ApiError::NotFound)?;
    if instance.state != ServiceState::Ready {
        return Err(ApiError::ServiceInstanceNotReady);
    }
    Ok(instance)
}

async fn realtime_service(
    state: &AppState,
    organization_id: Uuid,
    service_instance_id: Uuid,
) -> Result<ServiceInstance, ApiError> {
    state
        .store
        .service_instance(ServiceInstanceId(service_instance_id))
        .await
        .map_err(ApiError::from_store)?
        .filter(|service| {
            service.organization_id == OrganizationId(organization_id) && service.provider == "flow"
        })
        .ok_or(ApiError::NotFound)
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
    let (cookie, csrf) = create_session_cookie(state, session_user.user.id).await?;
    Ok((
        jar.add(cookie),
        Json(SessionResponse::new(session_user, csrf)),
    ))
}

async fn create_session_cookie(
    state: &AppState,
    user_id: heterocloud_domain::UserId,
) -> Result<(Cookie<'static>, SecretString), ApiError> {
    let token = generate_token().map_err(|_| ApiError::Internal)?;
    let digest = token_hash(token.expose_secret());
    let expires_at = Utc::now()
        + ChronoDuration::from_std(state.config.session_ttl).map_err(|_| ApiError::Internal)?;
    state
        .store
        .create_session(user_id, &digest, expires_at)
        .await
        .map_err(ApiError::from_store)?;
    let csrf = csrf_token(token.expose_secret(), &state.config.csrf_key)
        .map_err(|_| ApiError::Internal)?;
    let cookie = session_cookie(
        token.expose_secret().to_owned(),
        state.config.secure_cookie,
        state.config.session_ttl.as_secs(),
    );
    Ok((cookie, csrf))
}

fn oidc_api_error(error: OidcError) -> ApiError {
    match error {
        OidcError::InvalidRequest => {
            ApiError::BadRequest("Invalid or expired OIDC login transaction.".into())
        }
        OidcError::AuthorizationRejected | OidcError::InvalidToken => ApiError::Unauthorized,
        OidcError::ProviderUnavailable => ApiError::IdentityProviderUnavailable,
        OidcError::Internal => ApiError::Internal,
    }
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

fn realtime_service_resource(organization_id: Uuid, service_instance_id: Uuid) -> String {
    organization_resource(
        organization_id,
        &format!("realtime/service/{service_instance_id}"),
    )
}

fn validate_flow_spec(spec: &FlowSpec) -> Result<(), ApiError> {
    if spec.max_participants == 0 || spec.max_participants > 100_000 {
        return Err(ApiError::BadRequest(
            "max_participants must be between 1 and 100000".into(),
        ));
    }
    if spec.region.trim().is_empty() || spec.region.len() > 64 {
        return Err(ApiError::BadRequest(
            "region must contain between 1 and 64 characters".into(),
        ));
    }
    if !spec.metadata.is_object() {
        return Err(ApiError::BadRequest("metadata must be an object".into()));
    }
    Ok(())
}

fn validate_invitation_ttl(expires_in_hours: i64) -> Result<(), ApiError> {
    if !(1..=INVITATION_MAX_TTL_HOURS).contains(&expires_in_hours) {
        return Err(ApiError::BadRequest(format!(
            "expires_in_hours must be 1..{INVITATION_MAX_TTL_HOURS}"
        )));
    }
    Ok(())
}

const fn default_invitation_ttl_hours() -> i64 {
    INVITATION_MAX_TTL_HOURS
}

const INVITATION_MAX_TTL_HOURS: i64 = 24;
const FLOW_ACCESS_DEFAULT_TTL_SECONDS: u64 = 300;
const FLOW_ACCESS_MIN_TTL_SECONDS: u64 = 30;
const FLOW_ACCESS_MAX_TTL_SECONDS: u64 = 300;

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
    use std::collections::BTreeSet;

    use chrono::Utc;
    use heterocloud_domain::{
        OrganizationId, ProjectId, ServiceInstance, ServiceInstanceId, ServiceState,
    };
    use serde_json::{Value, json};
    use uuid::Uuid;

    use crate::error::ApiError;

    use super::{
        CSRF_HEADER, CreateInvitation, INVITATION_MAX_TTL_HOURS, SESSION_COOKIE,
        flow_permission_iam_action, parse_api_key_prefix, validate_flow_access_target,
        validate_flow_access_ttl, validate_flow_permissions, validate_invitation_ttl,
        validate_slug,
    };

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

    #[test]
    fn invitation_creation_is_single_use_and_short_lived() {
        let default_request = serde_json::from_value::<CreateInvitation>(json!({}))
            .map_err(|error| format!("default invitation request should deserialize: {error}"));
        assert_eq!(
            default_request.ok().map(|request| request.expires_in_hours),
            Some(INVITATION_MAX_TTL_HOURS)
        );
        assert!(
            serde_json::from_value::<CreateInvitation>(
                json!({"max_uses": 2, "expires_in_hours": 1})
            )
            .is_err()
        );
        assert!(validate_invitation_ttl(1).is_ok());
        assert!(validate_invitation_ttl(INVITATION_MAX_TTL_HOURS).is_ok());
        assert!(validate_invitation_ttl(0).is_err());
        assert!(validate_invitation_ttl(INVITATION_MAX_TTL_HOURS + 1).is_err());
    }

    #[test]
    fn flow_permissions_are_exact_and_never_wildcarded() {
        assert!(
            validate_flow_permissions(&BTreeSet::from([
                "flow.room.join".to_owned(),
                "flow.turn.issue".to_owned(),
            ]))
            .is_ok()
        );
        assert!(validate_flow_permissions(&BTreeSet::new()).is_err());
        assert!(validate_flow_permissions(&BTreeSet::from(["flow.room.*".to_owned()])).is_err());
        assert!(
            validate_flow_permissions(&BTreeSet::from(["flow.room.delete".to_owned()])).is_err()
        );
    }

    #[test]
    fn every_flow_permission_maps_to_one_least_privilege_iam_action() {
        assert_eq!(
            [
                "flow.queue.read",
                "flow.queue.write",
                "flow.room.create",
                "flow.room.read",
                "flow.room.join",
                "flow.turn.issue",
                "flow.signal.connect",
            ]
            .map(flow_permission_iam_action),
            [
                Some("flow:QueueRead"),
                Some("flow:QueueWrite"),
                Some("flow:RoomCreate"),
                Some("flow:RoomRead"),
                Some("flow:RoomJoin"),
                Some("flow:TurnIssue"),
                Some("flow:SignalConnect"),
            ]
        );
        assert_eq!(flow_permission_iam_action("flow.room.*"), None);
    }

    #[test]
    fn flow_access_lifetime_defaults_to_five_minutes_and_is_bounded() {
        assert_eq!(validate_flow_access_ttl(None).ok(), Some(300));
        assert_eq!(validate_flow_access_ttl(Some(30)).ok(), Some(30));
        assert_eq!(validate_flow_access_ttl(Some(300)).ok(), Some(300));
        assert!(validate_flow_access_ttl(Some(29)).is_err());
        assert!(validate_flow_access_ttl(Some(301)).is_err());
    }

    #[test]
    fn flow_access_target_requires_ready_same_organization_flow_instance() {
        let organization_id = OrganizationId(Uuid::from_u128(1));
        let service_instance_id = ServiceInstanceId(Uuid::from_u128(2));
        let instance = ServiceInstance {
            id: service_instance_id,
            organization_id,
            project_id: ProjectId(Uuid::from_u128(3)),
            provider: "flow".into(),
            name: "flow-test".into(),
            generation: 1,
            state: ServiceState::Ready,
            spec: Value::Null,
            status: Value::Null,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        assert!(
            validate_flow_access_target(
                Some(instance.clone()),
                organization_id,
                service_instance_id,
            )
            .is_ok()
        );
        assert!(
            validate_flow_access_target(
                Some(instance.clone()),
                OrganizationId(Uuid::from_u128(4)),
                service_instance_id,
            )
            .is_err()
        );
        assert!(matches!(
            validate_flow_access_target(
                Some(ServiceInstance {
                    state: ServiceState::Provisioning,
                    ..instance.clone()
                }),
                organization_id,
                service_instance_id,
            ),
            Err(ApiError::ServiceInstanceNotReady)
        ));
        assert!(
            validate_flow_access_target(
                Some(ServiceInstance {
                    provider: "other".into(),
                    ..instance
                }),
                organization_id,
                service_instance_id,
            )
            .is_err()
        );
    }
}
