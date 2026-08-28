use std::{
    collections::BTreeSet,
    convert::Infallible,
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

use axum::{
    Json, Router,
    extract::{
        ConnectInfo, DefaultBodyLimit, FromRequestParts, Path, Query, State, ws::WebSocketUpgrade,
    },
    http::{HeaderMap, StatusCode, header, request::Parts},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use email_address::EmailAddress;
use heterocloud_auth::{
    constant_time_token_eq, csrf_token, generate_token, hash_password, token_hash, verify_password,
};
use heterocloud_domain::{
    DEFAULT_FLOW_MAX_ROOMS, DEFAULT_FLOW_RATE_LIMIT_BURST,
    DEFAULT_FLOW_RATE_LIMIT_REQUESTS_PER_SECOND, FlashSpec, FlowRateLimit, FlowSpec,
    MAX_FLOW_RATE_LIMIT_BURST, MAX_FLOW_RATE_LIMIT_REQUESTS_PER_SECOND, MAX_FLOW_ROOMS,
    OrganizationId, PolicyDocument, PolicyId, PrincipalId, ProjectId, ResourceQuotaLimits,
    ServiceInstance, ServiceInstanceId, ServiceState, UserStatus,
};
use heterocloud_iam::{AuthorizationRequest, Decision, authorize, semantics_digest};
use heterocloud_store::{
    AuditEvent, AuthorizationContext, DeveloperCredentialMint, DeveloperCredentialMintOutcome,
    FlowDeveloperCredentialRecord, MAX_FLOW_ACCESS_CONTEXT_LIST_SIZE,
    MAX_FLOW_DEVELOPER_CREDENTIAL_LIST_SIZE, MAX_REALTIME_METRIC_HISTORY_SAMPLES,
    MAX_USER_LOGIN_EVENTS_PER_USER, NewFlowAccessContext, NewFlowDeveloperCredential, OidcUser,
    RealtimeMetricCollectionTarget, RegisterWithInvitation, SessionUser, Store,
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
    flash_provider::{
        FlashContainerList, FlashProviderContext, FlashProviderProxy, bridge_websockets,
    },
    flow_access::{FlowAccessInput, SignedFlowAccessContext},
    metrics::fetch_and_record_realtime_metrics,
    oidc::{
        OIDC_TRANSACTION_COOKIE, OidcCallbackQuery, OidcError, OidcLoginIntent,
        clear_transaction_cookie,
    },
    registry::RegistryClient,
};

const SESSION_COOKIE: &str = "hc_session";
const CSRF_HEADER: &str = "x-heterocloud-csrf";

struct PeerAddress(Option<SocketAddr>);

impl<S> FromRequestParts<S> for PeerAddress
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(
            parts
                .extensions
                .get::<ConnectInfo<SocketAddr>>()
                .map(|ConnectInfo(address)| *address),
        ))
    }
}

#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    pub config: RuntimeConfig,
    pub flow_client: reqwest::Client,
    pub flash_provider: Option<Arc<FlashProviderProxy>>,
    pub registry: Option<Arc<RegistryClient>>,
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
        .route("/owner/quotas", get(owner_quota_overview))
        .route("/owner/accounts", get(list_owner_accounts))
        .route(
            "/owner/accounts/{user_id}/logins",
            get(list_owner_account_logins),
        )
        .route(
            "/owner/quotas/defaults",
            axum::routing::put(update_owner_quota_defaults),
        )
        .route(
            "/owner/quotas/organizations/{organization_id}",
            axum::routing::put(update_owner_organization_quota)
                .delete(clear_owner_organization_quota),
        )
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
            "/organizations/{organization_id}/flash/services",
            get(list_flash_services).post(create_flash_service),
        )
        .route(
            "/organizations/{organization_id}/flash/services/{service_instance_id}",
            get(get_flash_service)
                .put(update_flash_service)
                .delete(delete_flash_service),
        )
        .route(
            "/organizations/{organization_id}/flash/services/{service_instance_id}/containers",
            get(list_flash_containers),
        )
        .route(
            "/organizations/{organization_id}/flash/services/{service_instance_id}/exec",
            get(exec_flash_container),
        )
        .route(
            "/organizations/{organization_id}/registry",
            get(get_registry),
        )
        .route(
            "/organizations/{organization_id}/registry/images",
            get(list_registry_images),
        )
        .route(
            "/organizations/{organization_id}/registry/images/{digest}",
            axum::routing::delete(delete_registry_image),
        )
        .route(
            "/organizations/{organization_id}/registry/credentials",
            post(create_registry_credential),
        )
        .route(
            "/organizations/{organization_id}/registry/credentials/{credential_id}",
            axum::routing::delete(delete_registry_credential),
        )
        .route(
            "/organizations/{organization_id}/realtime/services/{service_instance_id}/access-credentials",
            post(create_realtime_access_credential)
                .layer(DefaultBodyLimit::max(FLOW_CREDENTIAL_BODY_LIMIT_BYTES)),
        )
        .route(
            "/organizations/{organization_id}/realtime/services/{service_instance_id}/developer-credentials",
            get(list_realtime_developer_credentials)
                .post(create_realtime_developer_credential)
                .layer(DefaultBodyLimit::max(FLOW_CREDENTIAL_BODY_LIMIT_BYTES)),
        )
        .route(
            "/organizations/{organization_id}/realtime/services/{service_instance_id}/developer-credentials/{credential_id}",
            axum::routing::delete(revoke_realtime_developer_credential),
        )
        .route(
            "/organizations/{organization_id}/realtime/services/{service_instance_id}/developer-credentials/{credential_id}/rotate",
            post(rotate_realtime_developer_credential)
                .layer(DefaultBodyLimit::max(FLOW_CREDENTIAL_BODY_LIMIT_BYTES)),
        )
        .route(
            "/organizations/{organization_id}/realtime/services/{service_instance_id}/access-contexts",
            get(list_realtime_access_contexts),
        )
        .route(
            "/organizations/{organization_id}/realtime/services/{service_instance_id}/access-contexts/{context_id}",
            axum::routing::delete(revoke_realtime_access_context),
        )
        .route(
            "/flow/v1/access-credentials",
            get(list_developer_access_contexts)
                .post(create_developer_access_credential)
                .layer(DefaultBodyLimit::max(FLOW_CREDENTIAL_BODY_LIMIT_BYTES)),
        )
        .route(
            "/flow/v1/access-credentials/{context_id}",
            axum::routing::delete(revoke_developer_access_context),
        )
        .route(
            "/organizations/{organization_id}/realtime/services/{service_instance_id}/metrics",
            get(get_realtime_service_metrics),
        )
        .route(
            "/organizations/{organization_id}/projects/{project_id}/realtime/services/{service_instance_id}/metrics/history",
            get(get_realtime_service_metrics_history),
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

async fn owner_quota_overview(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    jar: CookieJar,
    PeerAddress(peer): PeerAddress,
) -> Result<Json<Value>, ApiError> {
    require_owner(&state, &headers, &jar, peer, false).await?;
    let defaults = state
        .store
        .resource_quota_defaults()
        .await
        .map_err(ApiError::from_store)?;
    let tenants = state
        .store
        .list_resource_quota_tenants()
        .await
        .map_err(ApiError::from_store)?;
    Ok(Json(json!({ "defaults": defaults, "tenants": tenants })))
}

async fn list_owner_accounts(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    jar: CookieJar,
    PeerAddress(peer): PeerAddress,
) -> Result<Json<Value>, ApiError> {
    require_owner(&state, &headers, &jar, peer, false).await?;
    let items = state
        .store
        .list_owner_accounts()
        .await
        .map_err(ApiError::from_store)?;
    Ok(Json(json!({ "items": items })))
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnerLoginHistoryQuery {
    limit: Option<i64>,
}

async fn list_owner_account_logins(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<Uuid>,
    Query(query): Query<OwnerLoginHistoryQuery>,
    headers: HeaderMap,
    jar: CookieJar,
    PeerAddress(peer): PeerAddress,
) -> Result<Json<Value>, ApiError> {
    require_owner(&state, &headers, &jar, peer, false).await?;
    let limit = validate_list_limit(query.limit, MAX_USER_LOGIN_EVENTS_PER_USER)?;
    let items = state
        .store
        .list_user_login_events(heterocloud_domain::UserId(user_id), limit)
        .await
        .map_err(ApiError::from_store)?;
    Ok(Json(json!({ "items": items })))
}

async fn update_owner_quota_defaults(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    jar: CookieJar,
    PeerAddress(peer): PeerAddress,
    Json(limits): Json<ResourceQuotaLimits>,
) -> Result<Json<ResourceQuotaLimits>, ApiError> {
    require_owner(&state, &headers, &jar, peer, true).await?;
    let limits = state
        .store
        .update_resource_quota_defaults(&limits)
        .await
        .map_err(ApiError::from_store)?;
    schedule_registry_quota_reconcile(Arc::clone(&state), None);
    Ok(Json(limits))
}

async fn update_owner_organization_quota(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
    PeerAddress(peer): PeerAddress,
    Json(limits): Json<ResourceQuotaLimits>,
) -> Result<Json<ResourceQuotaLimits>, ApiError> {
    require_owner(&state, &headers, &jar, peer, true).await?;
    let limits = state
        .store
        .set_organization_resource_quota(OrganizationId(organization_id), &limits)
        .await
        .map_err(ApiError::from_store)?;
    schedule_registry_quota_reconcile(
        Arc::clone(&state),
        Some((OrganizationId(organization_id), limits.clone())),
    );
    Ok(Json(limits))
}

async fn clear_owner_organization_quota(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
    PeerAddress(peer): PeerAddress,
) -> Result<Json<ResourceQuotaLimits>, ApiError> {
    require_owner(&state, &headers, &jar, peer, true).await?;
    let limits = state
        .store
        .clear_organization_resource_quota(OrganizationId(organization_id))
        .await
        .map_err(ApiError::from_store)?;
    schedule_registry_quota_reconcile(
        Arc::clone(&state),
        Some((OrganizationId(organization_id), limits.clone())),
    );
    Ok(Json(limits))
}

fn schedule_registry_quota_reconcile(
    state: Arc<AppState>,
    target: Option<(OrganizationId, ResourceQuotaLimits)>,
) {
    let Some(registry) = state.registry.clone() else {
        return;
    };
    tokio::spawn(async move {
        let targets = match target {
            Some(target) => vec![target],
            None => match state.store.list_resource_quota_tenants().await {
                Ok(tenants) => tenants
                    .into_iter()
                    .map(|tenant| (tenant.organization.id, tenant.effective_limits))
                    .collect(),
                Err(error) => {
                    tracing::warn!(error = %error, "failed to list registry quota reconciliation targets");
                    return;
                }
            },
        };
        for (organization_id, limits) in targets {
            if let Err(error) = registry.ensure_project(organization_id, &limits).await {
                tracing::warn!(
                    %organization_id,
                    error = %error,
                    "registry quota reconciliation failed"
                );
            }
        }
    });
}

async fn get_registry(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Json<Value>, ApiError> {
    let actor = authenticated_actor(&state, &headers, &jar).await?;
    authorize_actor(
        &state,
        &actor,
        OrganizationId(organization_id),
        "registry:GetRegistry",
        &organization_resource(organization_id, "registry/*"),
    )
    .await?;
    let limits = state
        .store
        .effective_resource_quota(OrganizationId(organization_id))
        .await
        .map_err(ApiError::from_store)?;
    let registry = state
        .registry
        .as_deref()
        .ok_or(ApiError::RegistryProviderUnavailable)?;
    let project = registry
        .ensure_project(OrganizationId(organization_id), &limits)
        .await
        .map_err(|error| {
            tracing::warn!(%organization_id, error = %error, "registry project reconciliation failed");
            ApiError::RegistryProviderUnavailable
        })?;
    let credentials = state
        .store
        .list_registry_credentials(OrganizationId(organization_id))
        .await
        .map_err(ApiError::from_store)?;
    let image_prefix = project
        .image_prefix()
        .map_err(|_| ApiError::RegistryProviderUnavailable)?;
    Ok(Json(json!({
        "endpoint": project.endpoint,
        "project": project.name,
        "image_prefix": image_prefix,
        "storage_limit_bytes": project.storage_limit_bytes,
        "storage_used_bytes": project.storage_used_bytes,
        "max_credentials": limits.registry.max_credentials,
        "credentials": credentials,
    })))
}

async fn list_registry_images(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Json<Value>, ApiError> {
    let actor = authenticated_actor(&state, &headers, &jar).await?;
    authorize_actor(
        &state,
        &actor,
        OrganizationId(organization_id),
        "registry:GetRegistry",
        &organization_resource(organization_id, "registry/*"),
    )
    .await?;
    let limits = state
        .store
        .effective_resource_quota(OrganizationId(organization_id))
        .await
        .map_err(ApiError::from_store)?;
    let registry = state
        .registry
        .as_deref()
        .ok_or(ApiError::RegistryProviderUnavailable)?;
    let project = registry
        .ensure_project(OrganizationId(organization_id), &limits)
        .await
        .map_err(|error| {
            tracing::warn!(%organization_id, error = %error, "registry project reconciliation failed");
            ApiError::RegistryProviderUnavailable
        })?;
    let images = registry.list_images(&project).await.map_err(|error| {
        tracing::warn!(%organization_id, error = %error, "registry image listing failed");
        ApiError::RegistryProviderUnavailable
    })?;
    Ok(Json(json!({ "items": images })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeleteRegistryImageQuery {
    repository: String,
}

async fn delete_registry_image(
    State(state): State<Arc<AppState>>,
    Path((organization_id, digest)): Path<(Uuid, String)>,
    Query(query): Query<DeleteRegistryImageQuery>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Json<Value>, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    authorize_actor(
        &state,
        &actor,
        OrganizationId(organization_id),
        "registry:DeleteImage",
        &organization_resource(organization_id, &format!("registry/image/{digest}")),
    )
    .await?;
    let limits = state
        .store
        .effective_resource_quota(OrganizationId(organization_id))
        .await
        .map_err(ApiError::from_store)?;
    let registry = state
        .registry
        .as_deref()
        .ok_or(ApiError::RegistryProviderUnavailable)?;
    let project = registry
        .ensure_project(OrganizationId(organization_id), &limits)
        .await
        .map_err(|error| {
            tracing::warn!(%organization_id, error = %error, "registry project reconciliation failed");
            ApiError::RegistryProviderUnavailable
        })?;
    let images = registry.list_images(&project).await.map_err(|error| {
        tracing::warn!(%organization_id, error = %error, "registry image lookup before deletion failed");
        ApiError::RegistryProviderUnavailable
    })?;
    if !images
        .iter()
        .any(|image| image.repository == query.repository && image.digest == digest)
    {
        return Err(ApiError::NotFound);
    }
    let deleted = registry
        .delete_image(&project, &query.repository, &digest)
        .await
        .map_err(|error| {
            tracing::warn!(
                %organization_id,
                repository = %query.repository,
                %digest,
                error = %error,
                "registry image deletion failed"
            );
            ApiError::RegistryProviderUnavailable
        })?;
    if !deleted {
        return Err(ApiError::NotFound);
    }
    let storage_used_bytes = registry.storage_usage(&project).await.map_err(|error| {
        tracing::warn!(
            %organization_id,
            error = %error,
            "registry usage lookup after image deletion failed"
        );
        ApiError::RegistryProviderUnavailable
    })?;
    Ok(Json(json!({ "storage_used_bytes": storage_used_bytes })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateRegistryCredential {
    name: String,
}

async fn create_registry_credential(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(request): Json<CreateRegistryCredential>,
) -> Result<impl IntoResponse, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    validate_name(&request.name)?;
    let authorization = authorize_actor(
        &state,
        &actor,
        OrganizationId(organization_id),
        "registry:CreateCredential",
        &organization_resource(organization_id, "registry/credential/*"),
    )
    .await?;
    let limits = state
        .store
        .effective_resource_quota(OrganizationId(organization_id))
        .await
        .map_err(ApiError::from_store)?;
    let registry = state
        .registry
        .as_deref()
        .ok_or(ApiError::RegistryProviderUnavailable)?;
    let project = registry
        .ensure_project(OrganizationId(organization_id), &limits)
        .await
        .map_err(|error| {
            tracing::warn!(%organization_id, error = %error, "registry project reconciliation failed");
            ApiError::RegistryProviderUnavailable
        })?;
    let reservation = state
        .store
        .reserve_registry_credential(
            OrganizationId(organization_id),
            authorization.principal_id,
            request.name.trim(),
        )
        .await
        .map_err(ApiError::from_store)?;
    let robot_name = format!("hc-{}", reservation.id.simple());
    let secret = match registry.create_push_credential(&project, &robot_name).await {
        Ok(secret) => secret,
        Err(error) => {
            let _ = state
                .store
                .cancel_registry_credential_reservation(
                    OrganizationId(organization_id),
                    reservation.id,
                )
                .await;
            tracing::warn!(%organization_id, error = %error, "registry credential creation failed");
            return Err(ApiError::RegistryProviderUnavailable);
        }
    };
    let credential = match state
        .store
        .activate_registry_credential(
            OrganizationId(organization_id),
            reservation.id,
            secret.robot_id,
            &secret.username,
        )
        .await
    {
        Ok(credential) => credential,
        Err(error) => {
            let _ = registry.delete_credential(secret.robot_id).await;
            let _ = state
                .store
                .cancel_registry_credential_reservation(
                    OrganizationId(organization_id),
                    reservation.id,
                )
                .await;
            return Err(ApiError::from_store(error));
        }
    };
    let login_host = project
        .authority()
        .map_err(|_| ApiError::RegistryProviderUnavailable)?;
    let image_prefix = project
        .image_prefix()
        .map_err(|_| ApiError::RegistryProviderUnavailable)?;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "credential": credential,
            "username": secret.username,
            "password": secret.password,
            "login_host": login_host,
            "login_command": format!("docker login {login_host} --username '{}' --password-stdin", credential.username.as_deref().unwrap_or("")),
            "image_prefix": image_prefix,
        })),
    ))
}

async fn delete_registry_credential(
    State(state): State<Arc<AppState>>,
    Path((organization_id, credential_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<StatusCode, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    authorize_actor(
        &state,
        &actor,
        OrganizationId(organization_id),
        "registry:DeleteCredential",
        &organization_resource(
            organization_id,
            &format!("registry/credential/{credential_id}"),
        ),
    )
    .await?;
    let credential = state
        .store
        .registry_credential_for_delete(OrganizationId(organization_id), credential_id)
        .await
        .map_err(ApiError::from_store)?;
    let robot_id = credential.harbor_robot_id.ok_or(ApiError::Internal)?;
    let registry = state
        .registry
        .as_deref()
        .ok_or(ApiError::RegistryProviderUnavailable)?;
    registry
        .delete_credential(robot_id)
        .await
        .map_err(|error| {
            tracing::warn!(%organization_id, %credential_id, error = %error, "registry credential deletion failed");
            ApiError::RegistryProviderUnavailable
        })?;
    state
        .store
        .delete_registry_credential(OrganizationId(organization_id), credential_id)
        .await
        .map_err(ApiError::from_store)?;
    Ok(StatusCode::NO_CONTENT)
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
    PeerAddress(peer): PeerAddress,
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
    let source_ip = request_source_ip(
        state.config.owner_console_mode,
        &state.config.trusted_proxy_networks,
        &headers,
        peer,
    )
    .map(|address| address.to_string());
    state
        .store
        .create_session(
            password_user.user.id,
            &token_digest,
            expires_at,
            source_ip.as_deref(),
            "local",
        )
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
        Json(SessionResponse::new(session_user, csrf, false)),
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
    PeerAddress(peer): PeerAddress,
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
    let source_ip = request_source_ip(
        state.config.owner_console_mode,
        &state.config.trusted_proxy_networks,
        &headers,
        peer,
    )
    .map(|address| address.to_string());
    issue_session(&state, jar, session_user, source_ip.as_deref(), "local").await
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
    headers: HeaderMap,
    jar: CookieJar,
    PeerAddress(peer): PeerAddress,
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
        let source_ip = request_source_ip(
            state.config.owner_console_mode,
            &state.config.trusted_proxy_networks,
            &headers,
            peer,
        )
        .map(|address| address.to_string());
        let (cookie, _) =
            create_session_cookie(&state, session_user.user.id, source_ip.as_deref(), "oidc")
                .await?;
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
    headers: HeaderMap,
    jar: CookieJar,
    PeerAddress(peer): PeerAddress,
) -> Result<Json<SessionResponse>, ApiError> {
    let authenticated = authenticated_session(&state, &jar).await?;
    let owner_console = owner_request_allowed(
        &state.config,
        &headers,
        peer,
        &authenticated.user.user.email,
    );
    Ok(Json(SessionResponse::new(
        authenticated.user,
        authenticated.csrf,
        owner_console,
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
        None => deserialize_stored_flow_spec(current.spec)?,
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
            "flow",
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
            "flow",
            authorization.principal_id,
        )
        .await
        .map_err(ApiError::from_store)?;
    Ok((StatusCode::ACCEPTED, Json(service)))
}

#[derive(Default, Deserialize)]
struct FlashListQuery {
    project_id: Option<Uuid>,
}

async fn list_flash_services(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<Uuid>,
    Query(query): Query<FlashListQuery>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Json<Value>, ApiError> {
    let actor = authenticated_actor(&state, &headers, &jar).await?;
    authorize_actor(
        &state,
        &actor,
        OrganizationId(organization_id),
        "flash:ListInstances",
        &flash_collection_resource(organization_id),
    )
    .await?;
    let items = state
        .store
        .list_service_instances(
            OrganizationId(organization_id),
            query.project_id.map(ProjectId),
            Some("flash"),
        )
        .await
        .map_err(ApiError::from_store)?;
    Ok(Json(json!({ "items": items })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateFlashService {
    project_id: Uuid,
    name: String,
    spec: FlashSpec,
}

async fn create_flash_service(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(request): Json<CreateFlashService>,
) -> Result<impl IntoResponse, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    validate_name(&request.name)?;
    validate_flash_spec(&request.spec)?;
    let authorization = authorize_actor(
        &state,
        &actor,
        OrganizationId(organization_id),
        "flash:CreateInstance",
        &flash_collection_resource(organization_id),
    )
    .await?;
    let instance = state
        .store
        .create_service_instance(
            OrganizationId(organization_id),
            ProjectId(request.project_id),
            authorization.principal_id,
            "flash",
            &request.name,
            serde_json::to_value(request.spec).map_err(|_| ApiError::Internal)?,
        )
        .await
        .map_err(ApiError::from_store)?;
    Ok((StatusCode::ACCEPTED, Json(instance)))
}

async fn get_flash_service(
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
        "flash:GetInstance",
        &flash_service_resource(organization_id, service_instance_id),
    )
    .await?;
    Ok(Json(
        flash_service(&state, organization_id, service_instance_id).await?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateFlashService {
    name: String,
    spec: FlashSpec,
}

async fn update_flash_service(
    State(state): State<Arc<AppState>>,
    Path((organization_id, service_instance_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(request): Json<UpdateFlashService>,
) -> Result<impl IntoResponse, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    flash_service(&state, organization_id, service_instance_id).await?;
    validate_name(&request.name)?;
    validate_flash_spec(&request.spec)?;
    let authorization = authorize_actor(
        &state,
        &actor,
        OrganizationId(organization_id),
        "flash:UpdateInstance",
        &flash_service_resource(organization_id, service_instance_id),
    )
    .await?;
    let instance = state
        .store
        .update_service_instance(
            OrganizationId(organization_id),
            ServiceInstanceId(service_instance_id),
            "flash",
            authorization.principal_id,
            &request.name,
            serde_json::to_value(request.spec).map_err(|_| ApiError::Internal)?,
        )
        .await
        .map_err(ApiError::from_store)?;
    Ok((StatusCode::ACCEPTED, Json(instance)))
}

async fn delete_flash_service(
    State(state): State<Arc<AppState>>,
    Path((organization_id, service_instance_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<impl IntoResponse, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    flash_service(&state, organization_id, service_instance_id).await?;
    let authorization = authorize_actor(
        &state,
        &actor,
        OrganizationId(organization_id),
        "flash:DeleteInstance",
        &flash_service_resource(organization_id, service_instance_id),
    )
    .await?;
    let instance = state
        .store
        .begin_delete_service_instance(
            OrganizationId(organization_id),
            ServiceInstanceId(service_instance_id),
            "flash",
            authorization.principal_id,
        )
        .await
        .map_err(ApiError::from_store)?;
    Ok((StatusCode::ACCEPTED, Json(instance)))
}

async fn list_flash_containers(
    State(state): State<Arc<AppState>>,
    Path((organization_id, service_instance_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Json<FlashContainerList>, ApiError> {
    let actor = authenticated_actor(&state, &headers, &jar).await?;
    let authorization = authorize_actor(
        &state,
        &actor,
        OrganizationId(organization_id),
        "flash:ExecInstance",
        &flash_service_resource(organization_id, service_instance_id),
    )
    .await?;
    let instance = flash_service(&state, organization_id, service_instance_id).await?;
    if instance.state != ServiceState::Ready {
        return Err(ApiError::ServiceInstanceNotReady);
    }
    let provider = state
        .flash_provider
        .as_ref()
        .ok_or(ApiError::FlashProviderUnavailable)?;
    let containers = provider
        .list_containers(flash_provider_context(
            &instance,
            authorization.principal_id,
        ))
        .await
        .map_err(|error| {
            tracing::warn!(error = %error, "Flash container discovery failed");
            ApiError::FlashProviderUnavailable
        })?;
    Ok(Json(containers))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FlashExecQuery {
    pod: String,
}

async fn exec_flash_container(
    State(state): State<Arc<AppState>>,
    Path((organization_id, service_instance_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<FlashExecQuery>,
    headers: HeaderMap,
    jar: CookieJar,
    upgrade: WebSocketUpgrade,
) -> Result<impl IntoResponse, ApiError> {
    require_same_origin(&state.config, &headers)?;
    if !valid_kubernetes_name(&query.pod) {
        return Err(ApiError::BadRequest("pod is invalid".into()));
    }
    let actor = authenticated_actor(&state, &headers, &jar).await?;
    let authorization = authorize_actor(
        &state,
        &actor,
        OrganizationId(organization_id),
        "flash:ExecInstance",
        &flash_service_resource(organization_id, service_instance_id),
    )
    .await?;
    let instance = flash_service(&state, organization_id, service_instance_id).await?;
    if instance.state != ServiceState::Ready {
        return Err(ApiError::ServiceInstanceNotReady);
    }
    let provider = state
        .flash_provider
        .as_ref()
        .ok_or(ApiError::FlashProviderUnavailable)?;
    let provider_socket = provider
        .connect_exec(
            flash_provider_context(&instance, authorization.principal_id),
            &query.pod,
        )
        .await
        .map_err(|error| {
            tracing::warn!(error = %error, "Flash exec connection failed");
            ApiError::FlashProviderUnavailable
        })?;
    Ok(upgrade
        .max_message_size(64 * 1024)
        .on_upgrade(move |browser_socket| bridge_websockets(browser_socket, provider_socket)))
}

fn flash_provider_context(
    instance: &ServiceInstance,
    principal_id: PrincipalId,
) -> FlashProviderContext {
    FlashProviderContext {
        principal_id,
        organization_id: instance.organization_id,
        project_id: instance.project_id,
        service_instance_id: instance.id,
        generation: instance.generation,
    }
}

fn valid_kubernetes_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'.')
        })
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
    rate_limit: FlowRateLimit,
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
    let rate_limit = deserialize_stored_flow_spec(instance.spec.clone())?.rate_limit;
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

    let (issued_at, expires_at, issued_at_time, expires_at_time) =
        flow_access_window(expires_in_seconds)?;
    let context_id = Uuid::now_v7();
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
            context_id,
        )
        .map_err(|_| ApiError::Internal)?;
    let stored_permissions = signed
        .context
        .permissions
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    state
        .store
        .record_flow_access_context(&NewFlowAccessContext {
            context_id,
            organization_id: instance.organization_id,
            project_id: instance.project_id,
            service_instance_id: instance.id,
            credential_id: None,
            principal_id: authorization.principal_id,
            permissions: &stored_permissions,
            issued_at: issued_at_time,
            expires_at: expires_at_time,
        })
        .await
        .map_err(ApiError::from_store)?;
    let response = flow_access_response(signed, &state.config.flow_public_endpoints, rate_limit);
    Ok((
        StatusCode::CREATED,
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::PRAGMA, "no-cache"),
        ],
        Json(response),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateRealtimeDeveloperCredential {
    name: String,
    permissions: BTreeSet<String>,
    expires_in_days: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RotateRealtimeDeveloperCredential {}

#[derive(Serialize)]
struct FlowDeveloperCredentialResponse {
    id: Uuid,
    name: String,
    prefix: String,
    permissions: Vec<String>,
    expires_at: DateTime<Utc>,
    last_used_at: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

impl From<FlowDeveloperCredentialRecord> for FlowDeveloperCredentialResponse {
    fn from(record: FlowDeveloperCredentialRecord) -> Self {
        Self {
            id: record.id,
            name: record.name,
            prefix: record.prefix,
            permissions: record.permissions,
            expires_at: record.expires_at,
            last_used_at: record.last_used_at,
            revoked_at: record.revoked_at,
            created_at: record.created_at,
        }
    }
}

#[derive(Serialize)]
struct FlowDeveloperCredentialCreationResponse {
    #[serde(flatten)]
    item: FlowDeveloperCredentialResponse,
    credential: String,
    mint_endpoint: Url,
}

#[derive(Serialize)]
struct CollectionResponse<T> {
    items: Vec<T>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FlowCredentialListQuery {
    limit: Option<i64>,
}

async fn list_realtime_developer_credentials(
    State(state): State<Arc<AppState>>,
    Path((organization_id, service_instance_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<FlowCredentialListQuery>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<impl IntoResponse, ApiError> {
    let actor = authenticated_actor(&state, &headers, &jar).await?;
    realtime_service(&state, organization_id, service_instance_id).await?;
    authorize_flow_credential_management(
        &state,
        &actor,
        organization_id,
        service_instance_id,
        None,
    )
    .await?;
    let limit = validate_list_limit(query.limit, MAX_FLOW_DEVELOPER_CREDENTIAL_LIST_SIZE)?;
    let items = state
        .store
        .list_flow_developer_credentials(
            OrganizationId(organization_id),
            ServiceInstanceId(service_instance_id),
            limit,
        )
        .await
        .map_err(ApiError::from_store)?
        .into_iter()
        .map(FlowDeveloperCredentialResponse::from)
        .collect();
    Ok((
        sensitive_response_headers(),
        Json(CollectionResponse { items }),
    ))
}

async fn create_realtime_developer_credential(
    State(state): State<Arc<AppState>>,
    Path((organization_id, service_instance_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(request): Json<CreateRealtimeDeveloperCredential>,
) -> Result<impl IntoResponse, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    let name = validate_developer_credential_name(&request.name)?;
    validate_flow_permissions(&request.permissions)?;
    validate_developer_credential_expiry(request.expires_in_days)?;
    realtime_service(&state, organization_id, service_instance_id).await?;
    let authorization = authorize_flow_credential_management(
        &state,
        &actor,
        organization_id,
        service_instance_id,
        Some(&request.permissions),
    )
    .await?;
    let (prefix, credential, credential_hash) = generate_flow_developer_credential()?;
    let permissions = request.permissions.into_iter().collect::<Vec<_>>();
    let created_at = Utc::now();
    let expires_at = created_at + ChronoDuration::days(request.expires_in_days);
    let record = state
        .store
        .create_flow_developer_credential(NewFlowDeveloperCredential {
            organization_id: OrganizationId(organization_id),
            service_instance_id: ServiceInstanceId(service_instance_id),
            created_by: authorization.principal_id,
            name,
            prefix: &prefix,
            secret_hash: &credential_hash,
            permissions: &permissions,
            expires_at,
            created_at,
        })
        .await
        .map_err(ApiError::from_store)?;
    Ok((
        StatusCode::CREATED,
        sensitive_response_headers(),
        Json(FlowDeveloperCredentialCreationResponse {
            item: record.into(),
            credential: credential.expose_secret().to_owned(),
            mint_endpoint: flow_developer_mint_endpoint(&state.config)?,
        }),
    ))
}

async fn revoke_realtime_developer_credential(
    State(state): State<Arc<AppState>>,
    Path((organization_id, service_instance_id, credential_id)): Path<(Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<impl IntoResponse, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    realtime_service(&state, organization_id, service_instance_id).await?;
    let authorization = authorize_flow_credential_management(
        &state,
        &actor,
        organization_id,
        service_instance_id,
        None,
    )
    .await?;
    let _revocation = state
        .store
        .revoke_flow_developer_credential(
            OrganizationId(organization_id),
            ServiceInstanceId(service_instance_id),
            credential_id,
            authorization.principal_id,
        )
        .await
        .map_err(ApiError::from_store)?;
    Ok((StatusCode::NO_CONTENT, sensitive_response_headers()))
}

async fn rotate_realtime_developer_credential(
    State(state): State<Arc<AppState>>,
    Path((organization_id, service_instance_id, credential_id)): Path<(Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(_request): Json<RotateRealtimeDeveloperCredential>,
) -> Result<impl IntoResponse, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    realtime_service(&state, organization_id, service_instance_id).await?;
    let authorization = authorize_flow_credential_management(
        &state,
        &actor,
        organization_id,
        service_instance_id,
        None,
    )
    .await?;
    let existing = state
        .store
        .flow_developer_credential(
            OrganizationId(organization_id),
            ServiceInstanceId(service_instance_id),
            credential_id,
        )
        .await
        .map_err(ApiError::from_store)?
        .ok_or(ApiError::NotFound)?;
    let permissions = existing
        .permissions
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    authorize_flow_permissions(
        &state,
        &actor,
        organization_id,
        service_instance_id,
        &permissions,
    )
    .await?;
    let (prefix, credential, credential_hash) = generate_flow_developer_credential()?;
    let rotation = state
        .store
        .rotate_flow_developer_credential(
            OrganizationId(organization_id),
            ServiceInstanceId(service_instance_id),
            credential_id,
            &prefix,
            &credential_hash,
            authorization.principal_id,
        )
        .await
        .map_err(ApiError::from_store)?;
    Ok((
        StatusCode::CREATED,
        sensitive_response_headers(),
        Json(FlowDeveloperCredentialCreationResponse {
            item: rotation.credential.into(),
            credential: credential.expose_secret().to_owned(),
            mint_endpoint: flow_developer_mint_endpoint(&state.config)?,
        }),
    ))
}

async fn list_realtime_access_contexts(
    State(state): State<Arc<AppState>>,
    Path((organization_id, service_instance_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<FlowCredentialListQuery>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<impl IntoResponse, ApiError> {
    let actor = authenticated_actor(&state, &headers, &jar).await?;
    realtime_service(&state, organization_id, service_instance_id).await?;
    authorize_flow_credential_management(
        &state,
        &actor,
        organization_id,
        service_instance_id,
        None,
    )
    .await?;
    let limit = validate_list_limit(query.limit, MAX_FLOW_ACCESS_CONTEXT_LIST_SIZE)?;
    let items = state
        .store
        .list_flow_access_contexts(
            OrganizationId(organization_id),
            ServiceInstanceId(service_instance_id),
            limit,
        )
        .await
        .map_err(ApiError::from_store)?;
    Ok((
        sensitive_response_headers(),
        Json(CollectionResponse { items }),
    ))
}

async fn revoke_realtime_access_context(
    State(state): State<Arc<AppState>>,
    Path((organization_id, service_instance_id, context_id)): Path<(Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<impl IntoResponse, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    realtime_service(&state, organization_id, service_instance_id).await?;
    let authorization = authorize_flow_credential_management(
        &state,
        &actor,
        organization_id,
        service_instance_id,
        None,
    )
    .await?;
    state
        .store
        .revoke_flow_access_context(
            OrganizationId(organization_id),
            ServiceInstanceId(service_instance_id),
            context_id,
            authorization.principal_id,
        )
        .await
        .map_err(ApiError::from_store)?;
    Ok((StatusCode::NO_CONTENT, sensitive_response_headers()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateDeveloperAccessCredential {
    principal_id: Uuid,
    permissions: BTreeSet<String>,
    expires_in_seconds: u64,
}

async fn create_developer_access_credential(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<CreateDeveloperAccessCredential>,
) -> Result<impl IntoResponse, ApiError> {
    let (prefix, secret_hash) = authenticate_flow_developer_bearer(&headers)?;
    validate_flow_permissions(&request.permissions)?;
    let expires_in_seconds = validate_flow_access_ttl(Some(request.expires_in_seconds))?;
    let (issued_at, expires_at, issued_at_time, expires_at_time) =
        flow_access_window(expires_in_seconds)?;
    let context_id = Uuid::now_v7();
    let permissions = request.permissions.iter().cloned().collect::<Vec<_>>();
    let outcome = state
        .store
        .mint_flow_access_context_with_developer_credential(&DeveloperCredentialMint {
            prefix,
            secret_hash: &secret_hash,
            context_id,
            principal_id: PrincipalId(request.principal_id),
            permissions: &permissions,
            issued_at: issued_at_time,
            expires_at: expires_at_time,
        })
        .await
        .map_err(ApiError::from_store)?;
    let scope = match outcome {
        DeveloperCredentialMintOutcome::Issued(scope) => scope,
        DeveloperCredentialMintOutcome::InvalidCredential => return Err(ApiError::Unauthorized),
        DeveloperCredentialMintOutcome::PermissionDenied => return Err(ApiError::Forbidden),
        DeveloperCredentialMintOutcome::ServiceInstanceNotReady => {
            return Err(ApiError::ServiceInstanceNotReady);
        }
    };
    let rate_limit = deserialize_stored_flow_spec(scope.service_spec)?.rate_limit;
    let signed = state
        .config
        .flow_access_signer
        .sign(
            FlowAccessInput {
                organization_id: scope.organization_id,
                project_id: scope.project_id,
                service_instance_id: scope.service_instance_id,
                principal_id: PrincipalId(request.principal_id),
                permissions: request.permissions,
            },
            issued_at,
            expires_at,
            context_id,
        )
        .map_err(|_| ApiError::Internal)?;
    Ok((
        StatusCode::CREATED,
        sensitive_response_headers(),
        Json(flow_access_response(
            signed,
            &state.config.flow_public_endpoints,
            rate_limit,
        )),
    ))
}

async fn list_developer_access_contexts(
    State(state): State<Arc<AppState>>,
    Query(query): Query<FlowCredentialListQuery>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    let (prefix, secret_hash) = authenticate_flow_developer_bearer(&headers)?;
    let limit = validate_list_limit(query.limit, MAX_FLOW_ACCESS_CONTEXT_LIST_SIZE)?;
    let items = state
        .store
        .list_flow_access_contexts_for_developer_credential(prefix, &secret_hash, limit)
        .await
        .map_err(ApiError::from_store)?
        .ok_or(ApiError::Unauthorized)?;
    Ok((
        sensitive_response_headers(),
        Json(CollectionResponse { items }),
    ))
}

async fn revoke_developer_access_context(
    State(state): State<Arc<AppState>>,
    Path(context_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    let (prefix, secret_hash) = authenticate_flow_developer_bearer(&headers)?;
    state
        .store
        .revoke_flow_access_context_with_developer_credential(prefix, &secret_hash, context_id)
        .await
        .map_err(ApiError::from_store)?
        .ok_or(ApiError::Unauthorized)?;
    Ok((StatusCode::NO_CONTENT, sensitive_response_headers()))
}

async fn get_realtime_service_metrics(
    State(state): State<Arc<AppState>>,
    Path((organization_id, service_instance_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<impl IntoResponse, ApiError> {
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
    let target = RealtimeMetricCollectionTarget {
        service_instance_id: instance.id,
        organization_id: instance.organization_id,
        project_id: instance.project_id,
    };
    let metrics =
        fetch_and_record_realtime_metrics(&state, &target, authorization.principal_id).await?;
    Ok(Json(metrics))
}

#[derive(Deserialize)]
struct RealtimeMetricHistoryQuery {
    range: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RealtimeMetricHistoryRange {
    OneHour,
    SixHours,
    OneDay,
    SevenDays,
    ThirtyDays,
}

impl RealtimeMetricHistoryRange {
    fn parse(value: &str) -> Result<Self, ApiError> {
        match value {
            "1h" => Ok(Self::OneHour),
            "6h" => Ok(Self::SixHours),
            "24h" => Ok(Self::OneDay),
            "7d" => Ok(Self::SevenDays),
            "30d" => Ok(Self::ThirtyDays),
            _ => Err(ApiError::BadRequest(
                "range must be one of 1h, 6h, 24h, 7d, or 30d".into(),
            )),
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::OneHour => "1h",
            Self::SixHours => "6h",
            Self::OneDay => "24h",
            Self::SevenDays => "7d",
            Self::ThirtyDays => "30d",
        }
    }

    const fn duration(self) -> ChronoDuration {
        match self {
            Self::OneHour => ChronoDuration::hours(1),
            Self::SixHours => ChronoDuration::hours(6),
            Self::OneDay => ChronoDuration::hours(24),
            Self::SevenDays => ChronoDuration::days(7),
            Self::ThirtyDays => ChronoDuration::days(30),
        }
    }

    const fn step_seconds(self) -> i64 {
        match self {
            Self::OneHour => 15,
            Self::SixHours => 90,
            Self::OneDay => 360,
            Self::SevenDays => 2_520,
            Self::ThirtyDays => 10_800,
        }
    }
}

#[derive(Serialize)]
struct RealtimeMetricHistoryResponse {
    service_instance_id: ServiceInstanceId,
    range: &'static str,
    step_seconds: i64,
    max_samples: i64,
    samples: Vec<heterocloud_store::RealtimeMetricHistorySample>,
}

async fn get_realtime_service_metrics_history(
    State(state): State<Arc<AppState>>,
    Path((organization_id, project_id, service_instance_id)): Path<(Uuid, Uuid, Uuid)>,
    Query(query): Query<RealtimeMetricHistoryQuery>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Json<RealtimeMetricHistoryResponse>, ApiError> {
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
    if instance.project_id != ProjectId(project_id) {
        return Err(ApiError::NotFound);
    }
    authorize_actor(
        &state,
        &actor,
        OrganizationId(organization_id),
        "realtime:GetMetrics",
        &realtime_service_resource(organization_id, service_instance_id),
    )
    .await?;
    let range = RealtimeMetricHistoryRange::parse(&query.range)?;
    let samples = state
        .store
        .realtime_metric_history(
            instance.id,
            Utc::now() - range.duration(),
            range.step_seconds(),
        )
        .await
        .map_err(ApiError::from_store)?;
    Ok(Json(RealtimeMetricHistoryResponse {
        service_instance_id: instance.id,
        range: range.label(),
        step_seconds: range.step_seconds(),
        max_samples: MAX_REALTIME_METRIC_HISTORY_SAMPLES,
        samples,
    }))
}

async fn authorize_flow_credential_management(
    state: &AppState,
    actor: &AuthenticatedActor,
    organization_id: Uuid,
    service_instance_id: Uuid,
    permissions: Option<&BTreeSet<String>>,
) -> Result<AuthorizationContext, ApiError> {
    let resource = realtime_service_resource(organization_id, service_instance_id);
    let authorization = authorize_actor(
        state,
        actor,
        OrganizationId(organization_id),
        "realtime:IssueAccessCredential",
        &resource,
    )
    .await?;
    if let Some(permissions) = permissions {
        authorize_flow_permissions(
            state,
            actor,
            organization_id,
            service_instance_id,
            permissions,
        )
        .await?;
    }
    Ok(authorization)
}

async fn authorize_flow_permissions(
    state: &AppState,
    actor: &AuthenticatedActor,
    organization_id: Uuid,
    service_instance_id: Uuid,
    permissions: &BTreeSet<String>,
) -> Result<(), ApiError> {
    let resource = realtime_service_resource(organization_id, service_instance_id);
    for permission in permissions {
        let action = flow_permission_iam_action(permission).ok_or(ApiError::Internal)?;
        authorize_actor(
            state,
            actor,
            OrganizationId(organization_id),
            action,
            &resource,
        )
        .await?;
    }
    Ok(())
}

fn generate_flow_developer_credential() -> Result<(String, SecretString, [u8; 32]), ApiError> {
    let fragment = Uuid::now_v7().simple().to_string()[..16].to_owned();
    let prefix = format!("hcf_{fragment}");
    let secret = generate_token().map_err(|_| ApiError::Internal)?;
    let credential = SecretString::from(format!("{prefix}_{}", secret.expose_secret()));
    let digest = token_hash(credential.expose_secret());
    Ok((prefix, credential, digest))
}

fn authenticate_flow_developer_bearer(headers: &HeaderMap) -> Result<(&str, [u8; 32]), ApiError> {
    let authorization = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or(ApiError::Unauthorized)?;
    let token = authorization
        .strip_prefix("Bearer ")
        .ok_or(ApiError::Unauthorized)?;
    let prefix = parse_flow_developer_credential_prefix(token)?;
    Ok((prefix, token_hash(token)))
}

fn parse_flow_developer_credential_prefix(token: &str) -> Result<&str, ApiError> {
    let rest = token.strip_prefix("hcf_").ok_or(ApiError::Unauthorized)?;
    let (fragment, secret) = rest.split_once('_').ok_or(ApiError::Unauthorized)?;
    if fragment.len() != FLOW_DEVELOPER_CREDENTIAL_FRAGMENT_LENGTH
        || !fragment
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || secret.len() != FLOW_DEVELOPER_CREDENTIAL_SECRET_LENGTH
        || !secret
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(ApiError::Unauthorized);
    }
    let prefix_length = "hcf_".len() + FLOW_DEVELOPER_CREDENTIAL_FRAGMENT_LENGTH;
    token.get(..prefix_length).ok_or(ApiError::Unauthorized)
}

fn flow_access_window(
    expires_in_seconds: u64,
) -> Result<(u64, u64, DateTime<Utc>, DateTime<Utc>), ApiError> {
    let issued_at_seconds = Utc::now().timestamp();
    let expires_in_seconds = i64::try_from(expires_in_seconds).map_err(|_| ApiError::Internal)?;
    let expires_at_seconds = issued_at_seconds
        .checked_add(expires_in_seconds)
        .ok_or(ApiError::Internal)?;
    let issued_at = u64::try_from(issued_at_seconds).map_err(|_| ApiError::Internal)?;
    let expires_at = u64::try_from(expires_at_seconds).map_err(|_| ApiError::Internal)?;
    let issued_at_time =
        DateTime::from_timestamp(issued_at_seconds, 0).ok_or(ApiError::Internal)?;
    let expires_at_time =
        DateTime::from_timestamp(expires_at_seconds, 0).ok_or(ApiError::Internal)?;
    Ok((issued_at, expires_at, issued_at_time, expires_at_time))
}

fn flow_developer_mint_endpoint(config: &RuntimeConfig) -> Result<Url, ApiError> {
    let origin = config.public_origin.origin().ascii_serialization();
    Url::parse(&format!("{origin}/api/v1/flow/v1/access-credentials"))
        .map_err(|_| ApiError::Internal)
}

fn sensitive_response_headers() -> [(header::HeaderName, &'static str); 2] {
    [
        (header::CACHE_CONTROL, "no-store"),
        (header::PRAGMA, "no-cache"),
    ]
}

fn validate_developer_credential_name(name: &str) -> Result<&str, ApiError> {
    if name.trim() != name || name.chars().any(char::is_control) {
        return Err(ApiError::BadRequest(
            "name must not contain surrounding whitespace or control characters".into(),
        ));
    }
    validate_name(name)?;
    Ok(name)
}

fn validate_developer_credential_expiry(expires_in_days: i64) -> Result<(), ApiError> {
    if !(FLOW_DEVELOPER_CREDENTIAL_MIN_TTL_DAYS..=FLOW_DEVELOPER_CREDENTIAL_MAX_TTL_DAYS)
        .contains(&expires_in_days)
    {
        return Err(ApiError::BadRequest(format!(
            "expires_in_days must be {FLOW_DEVELOPER_CREDENTIAL_MIN_TTL_DAYS}..{FLOW_DEVELOPER_CREDENTIAL_MAX_TTL_DAYS}"
        )));
    }
    Ok(())
}

fn validate_list_limit(limit: Option<i64>, maximum: i64) -> Result<i64, ApiError> {
    let limit = limit.unwrap_or(maximum);
    if !(1..=maximum).contains(&limit) {
        return Err(ApiError::BadRequest(format!(
            "limit must be between 1 and {maximum}"
        )));
    }
    Ok(limit)
}

fn flow_access_response(
    signed: SignedFlowAccessContext,
    flow_public_endpoints: &[Url],
    rate_limit: FlowRateLimit,
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
        rate_limit,
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

async fn flash_service(
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
            service.organization_id == OrganizationId(organization_id)
                && service.provider == "flash"
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
    source_ip: Option<&str>,
    authentication_method: &str,
) -> Result<(CookieJar, Json<SessionResponse>), ApiError> {
    let (cookie, csrf) = create_session_cookie(
        state,
        session_user.user.id,
        source_ip,
        authentication_method,
    )
    .await?;
    Ok((
        jar.add(cookie),
        Json(SessionResponse::new(session_user, csrf, false)),
    ))
}

async fn create_session_cookie(
    state: &AppState,
    user_id: heterocloud_domain::UserId,
    source_ip: Option<&str>,
    authentication_method: &str,
) -> Result<(Cookie<'static>, SecretString), ApiError> {
    let token = generate_token().map_err(|_| ApiError::Internal)?;
    let digest = token_hash(token.expose_secret());
    let expires_at = Utc::now()
        + ChronoDuration::from_std(state.config.session_ttl).map_err(|_| ApiError::Internal)?;
    state
        .store
        .create_session(
            user_id,
            &digest,
            expires_at,
            source_ip,
            authentication_method,
        )
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

async fn require_owner(
    state: &AppState,
    headers: &HeaderMap,
    jar: &CookieJar,
    peer: Option<SocketAddr>,
    mutation: bool,
) -> Result<AuthenticatedSession, ApiError> {
    let authenticated = if mutation {
        authenticated_mutation(state, headers, jar).await?
    } else {
        authenticated_session(state, jar).await?
    };
    if !owner_request_allowed(&state.config, headers, peer, &authenticated.user.user.email) {
        return Err(ApiError::Forbidden);
    }
    Ok(authenticated)
}

fn owner_request_allowed(
    config: &RuntimeConfig,
    headers: &HeaderMap,
    peer: Option<SocketAddr>,
    email: &str,
) -> bool {
    let (Some(origin), Some(owner_email)) =
        (config.owner_origin.as_ref(), config.owner_email.as_deref())
    else {
        return false;
    };
    if !email.eq_ignore_ascii_case(owner_email)
        || !owner_network_boundary_allows(
            config.owner_console_mode,
            &config.owner_allowed_networks,
            peer,
        )
    {
        return false;
    }
    let Some(host) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let Some(expected_host) = origin.host_str() else {
        return false;
    };
    let expected_authority = match origin.port() {
        Some(port) => format!("{expected_host}:{port}"),
        None => expected_host.to_owned(),
    };
    host.eq_ignore_ascii_case(&expected_authority)
}

fn owner_network_boundary_allows(
    owner_console_mode: bool,
    allowed_networks: &[ipnet::IpNet],
    peer: Option<SocketAddr>,
) -> bool {
    // The owner-only Kubernetes deployment is already restricted by a
    // NetworkPolicy. Its ClusterIP Service may SNAT the TCP peer before Axum
    // sees it, so the application cannot repeat that CIDR check reliably.
    owner_console_mode
        || peer.is_some_and(|peer| {
            allowed_networks
                .iter()
                .any(|network| network.contains(&peer.ip()))
        })
}

fn request_source_ip(
    owner_console_mode: bool,
    trusted_proxy_networks: &[ipnet::IpNet],
    headers: &HeaderMap,
    peer: Option<SocketAddr>,
) -> Option<IpAddr> {
    let peer_ip = peer?.ip();
    let peer_is_trusted_proxy = !owner_console_mode
        && trusted_proxy_networks
            .iter()
            .any(|network| network.contains(&peer_ip));
    if !peer_is_trusted_proxy {
        return Some(peer_ip);
    }
    headers
        .get("x-envoy-external-address")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<IpAddr>().ok())
        .or(Some(peer_ip))
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

fn flash_collection_resource(organization_id: Uuid) -> String {
    organization_resource(organization_id, "flash/*")
}

fn flash_service_resource(organization_id: Uuid, service_instance_id: Uuid) -> String {
    organization_resource(
        organization_id,
        &format!("flash/instance/{service_instance_id}"),
    )
}

fn deserialize_stored_flow_spec(mut value: Value) -> Result<FlowSpec, ApiError> {
    let object = value.as_object_mut().ok_or(ApiError::Internal)?;
    object
        .entry("max_rooms")
        .or_insert_with(|| json!(DEFAULT_FLOW_MAX_ROOMS));
    object.entry("rate_limit").or_insert_with(|| {
        json!({
            "requests_per_second": DEFAULT_FLOW_RATE_LIMIT_REQUESTS_PER_SECOND,
            "burst": DEFAULT_FLOW_RATE_LIMIT_BURST,
        })
    });
    serde_json::from_value(value).map_err(|_| ApiError::Internal)
}

fn validate_flow_spec(spec: &FlowSpec) -> Result<(), ApiError> {
    if !(1..=MAX_FLOW_ROOMS).contains(&spec.max_rooms) {
        return Err(ApiError::BadRequest(format!(
            "max_rooms must be between 1 and {MAX_FLOW_ROOMS}"
        )));
    }
    if spec.max_participants == 0 || spec.max_participants > 100_000 {
        return Err(ApiError::BadRequest(
            "max_participants must be between 1 and 100000".into(),
        ));
    }
    if !(1..=MAX_FLOW_RATE_LIMIT_REQUESTS_PER_SECOND).contains(&spec.rate_limit.requests_per_second)
    {
        return Err(ApiError::BadRequest(format!(
            "rate_limit.requests_per_second must be between 1 and {MAX_FLOW_RATE_LIMIT_REQUESTS_PER_SECOND}"
        )));
    }
    if !(1..=MAX_FLOW_RATE_LIMIT_BURST).contains(&spec.rate_limit.burst) {
        return Err(ApiError::BadRequest(format!(
            "rate_limit.burst must be between 1 and {MAX_FLOW_RATE_LIMIT_BURST}"
        )));
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

fn validate_flash_spec(spec: &FlashSpec) -> Result<(), ApiError> {
    spec.validate_request()
        .map_err(|error| ApiError::BadRequest(error.to_string()))
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
const FLOW_CREDENTIAL_BODY_LIMIT_BYTES: usize = 16 * 1024;
const FLOW_ACCESS_DEFAULT_TTL_SECONDS: u64 = 300;
const FLOW_ACCESS_MIN_TTL_SECONDS: u64 = 30;
const FLOW_ACCESS_MAX_TTL_SECONDS: u64 = 300;
const FLOW_DEVELOPER_CREDENTIAL_FRAGMENT_LENGTH: usize = 16;
const FLOW_DEVELOPER_CREDENTIAL_SECRET_LENGTH: usize = 43;
const FLOW_DEVELOPER_CREDENTIAL_MIN_TTL_DAYS: i64 = 1;
const FLOW_DEVELOPER_CREDENTIAL_MAX_TTL_DAYS: i64 = 365;

#[derive(Serialize)]
struct SessionResponse {
    user: heterocloud_domain::User,
    memberships: Vec<heterocloud_store::Membership>,
    csrf_token: String,
    owner_console: bool,
}

impl SessionResponse {
    fn new(session: SessionUser, csrf_token: SecretString, owner_console: bool) -> Self {
        Self {
            user: session.user,
            memberships: session.memberships,
            csrf_token: csrf_token.expose_secret().to_owned(),
            owner_console,
        }
    }
}

#[allow(dead_code)]
fn _service_id_type_guard(id: Uuid) -> ServiceInstanceId {
    ServiceInstanceId(id)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        net::{IpAddr, SocketAddr},
    };

    use axum::http::{HeaderMap, HeaderValue};
    use chrono::Utc;
    use heterocloud_domain::{
        FlashExposure, FlashExposureType, FlashPort, FlashProtocol, FlashSpec, FlashTrafficMode,
        FlowRateLimit, FlowSpec, MAX_FLOW_ROOMS, OrganizationId, ProjectId, ServiceInstance,
        ServiceInstanceId, ServiceState,
    };
    use ipnet::IpNet;
    use serde_json::{Value, json};
    use url::Url;
    use uuid::Uuid;

    use crate::error::ApiError;

    use super::{
        CSRF_HEADER, CreateDeveloperAccessCredential, CreateInvitation,
        FLOW_DEVELOPER_CREDENTIAL_MAX_TTL_DAYS, FLOW_DEVELOPER_CREDENTIAL_MIN_TTL_DAYS,
        FlowDeveloperCredentialCreationResponse, FlowDeveloperCredentialResponse,
        INVITATION_MAX_TTL_HOURS, RealtimeMetricHistoryRange, RealtimeMetricHistoryResponse,
        SESSION_COOKIE, deserialize_stored_flow_spec, flash_collection_resource,
        flash_service_resource, flow_permission_iam_action, owner_network_boundary_allows,
        parse_api_key_prefix, parse_flow_developer_credential_prefix, request_source_ip,
        valid_kubernetes_name, validate_developer_credential_expiry,
        validate_developer_credential_name, validate_flash_spec, validate_flow_access_target,
        validate_flow_access_ttl, validate_flow_permissions, validate_flow_spec,
        validate_invitation_ttl, validate_list_limit, validate_slug,
    };

    #[test]
    fn public_security_names_are_stable() {
        assert_eq!(SESSION_COOKIE, "hc_session");
        assert_eq!(CSRF_HEADER, "x-heterocloud-csrf");
    }

    #[test]
    fn owner_only_deployment_relies_on_its_network_policy_after_service_snat()
    -> Result<(), Box<dyn std::error::Error>> {
        let allowed_networks = ["10.250.0.0/24".parse::<IpNet>()?];
        let vpn_peer = "10.250.0.42:12345".parse::<SocketAddr>()?;
        let snat_peer = "10.244.3.1:12345".parse::<SocketAddr>()?;

        assert!(owner_network_boundary_allows(
            true,
            &allowed_networks,
            Some(snat_peer),
        ));
        assert!(owner_network_boundary_allows(
            false,
            &allowed_networks,
            Some(vpn_peer),
        ));
        assert!(!owner_network_boundary_allows(
            false,
            &allowed_networks,
            Some(snat_peer),
        ));
        assert!(!owner_network_boundary_allows(
            false,
            &allowed_networks,
            None,
        ));
        Ok(())
    }

    #[test]
    fn login_ip_uses_only_the_canonical_header_from_a_trusted_proxy()
    -> Result<(), Box<dyn std::error::Error>> {
        let trusted = ["10.244.0.0/16".parse::<IpNet>()?];
        let proxy = "10.244.2.7:43120".parse::<SocketAddr>()?;
        let direct = "198.51.100.8:43120".parse::<SocketAddr>()?;
        let client = "203.0.113.42".parse::<IpAddr>()?;
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-envoy-external-address",
            HeaderValue::from_static("203.0.113.42"),
        );

        assert_eq!(
            request_source_ip(false, &trusted, &headers, Some(proxy)),
            Some(client)
        );
        assert_eq!(
            request_source_ip(false, &trusted, &headers, Some(direct)),
            Some(direct.ip())
        );
        assert_eq!(
            request_source_ip(true, &trusted, &headers, Some(proxy)),
            Some(proxy.ip())
        );
        assert_eq!(request_source_ip(false, &trusted, &headers, None), None);
        Ok(())
    }

    #[test]
    fn validates_dns_slugs() {
        assert!(validate_slug("realtime-prod").is_ok());
        assert!(validate_slug("-invalid").is_err());
        assert!(validate_slug("Invalid").is_err());
    }

    #[test]
    fn validates_flash_exec_pod_names() {
        assert!(valid_kubernetes_name("flash-api-7bdbd985d7-x8k2m"));
        assert!(valid_kubernetes_name("flash.worker-1"));
        assert!(!valid_kubernetes_name(""));
        assert!(!valid_kubernetes_name("-flash-worker"));
        assert!(!valid_kubernetes_name("flash-worker-"));
        assert!(!valid_kubernetes_name("Flash-worker"));
        assert!(!valid_kubernetes_name("flash_worker"));
        assert!(!valid_kubernetes_name(&"a".repeat(254)));
    }

    #[test]
    fn flow_room_limit_is_positive_and_old_rows_get_the_conservative_default() {
        let mut spec = FlowSpec {
            region: "heteronet-global".into(),
            max_participants: 100,
            max_rooms: 1,
            rate_limit: FlowRateLimit {
                requests_per_second: 20,
                burst: 40,
            },
            metadata: json!({}),
        };
        assert!(validate_flow_spec(&spec).is_ok());
        spec.max_rooms = 0;
        assert!(validate_flow_spec(&spec).is_err());
        spec.max_rooms = MAX_FLOW_ROOMS;
        assert!(validate_flow_spec(&spec).is_ok());
        spec.max_rooms = MAX_FLOW_ROOMS + 1;
        assert!(validate_flow_spec(&spec).is_err());
        spec.max_rooms = 100;
        spec.rate_limit.requests_per_second = 0;
        assert!(validate_flow_spec(&spec).is_err());
        spec.rate_limit.requests_per_second = 1_000;
        spec.rate_limit.burst = 5_001;
        assert!(validate_flow_spec(&spec).is_err());

        let stored = deserialize_stored_flow_spec(json!({
            "region": "heteronet-global",
            "max_participants": 100,
            "metadata": {}
        }));
        assert_eq!(
            stored.ok().map(|spec| (
                spec.max_rooms,
                spec.rate_limit.requests_per_second,
                spec.rate_limit.burst,
            )),
            Some((100, 20, 40))
        );
    }

    #[test]
    fn flash_validation_and_iam_resources_are_provider_scoped() {
        let organization_id = Uuid::from_u128(1);
        let service_id = Uuid::from_u128(2);
        assert_eq!(
            flash_collection_resource(organization_id),
            format!("hc:org:{organization_id}:flash/*")
        );
        assert_eq!(
            flash_service_resource(organization_id, service_id),
            format!("hc:org:{organization_id}:flash/instance/{service_id}")
        );
        let spec = FlashSpec {
            region: "heteronet-global".into(),
            image: "ghcr.io/example/udp-server:v1".into(),
            replicas: 2,
            cpu_millis: 500,
            memory_mib: 512,
            ephemeral_storage_gib: 10,
            ports: vec![FlashPort {
                name: "game-udp".into(),
                protocol: FlashProtocol::Udp,
                container_port: 7777,
                service_port: 0,
            }],
            exposure: FlashExposure {
                exposure_type: FlashExposureType::Public,
                traffic_mode: FlashTrafficMode::Direct,
                allowed_source_cidrs: Vec::new(),
                denied_source_cidrs: Vec::new(),
            },
            env: Default::default(),
            command: Vec::new(),
            args: Vec::new(),
            metadata: Default::default(),
        };
        assert!(validate_flash_spec(&spec).is_ok());
    }

    #[test]
    fn metric_history_ranges_are_fixed_and_bounded_to_240_buckets() {
        for (label, duration_seconds, bucket_seconds) in [
            ("1h", 3_600, 15),
            ("6h", 21_600, 90),
            ("24h", 86_400, 360),
            ("7d", 604_800, 2_520),
            ("30d", 2_592_000, 10_800),
        ] {
            let range = RealtimeMetricHistoryRange::parse(label);
            assert_eq!(range.as_ref().ok().map(|range| range.label()), Some(label));
            assert_eq!(
                range
                    .as_ref()
                    .ok()
                    .map(|range| range.duration().num_seconds()),
                Some(duration_seconds)
            );
            assert_eq!(
                range.ok().map(RealtimeMetricHistoryRange::step_seconds),
                Some(bucket_seconds)
            );
            assert_eq!(duration_seconds / bucket_seconds, 240);
        }
        assert!(RealtimeMetricHistoryRange::parse("2h").is_err());
    }

    #[test]
    fn metric_history_response_uses_console_step_seconds_contract() {
        let response = RealtimeMetricHistoryResponse {
            service_instance_id: ServiceInstanceId(Uuid::from_u128(7)),
            range: "1h",
            step_seconds: 15,
            max_samples: 240,
            samples: Vec::new(),
        };
        let rendered = serde_json::to_value(response).ok();
        assert_eq!(
            rendered.as_ref().map(|value| &value["step_seconds"]),
            Some(&json!(15))
        );
        assert!(
            rendered
                .as_ref()
                .is_some_and(|value| value.get("bucket_seconds").is_none())
        );
    }

    #[test]
    fn api_key_prefix_is_strictly_parsed() {
        let token = "hc_0123456789abcdef_0123456789abcdefghijklmnopqrstuvwxyzABCDEFG";
        assert_eq!(parse_api_key_prefix(token).ok(), Some("0123456789abcdef"));
        assert!(parse_api_key_prefix("not-a-key").is_err());
    }

    #[test]
    fn flow_developer_credential_format_and_requests_are_strict() {
        let credential = format!("hcf_0123456789abcdef_{}", "A".repeat(43));
        assert_eq!(
            parse_flow_developer_credential_prefix(&credential).ok(),
            Some("hcf_0123456789abcdef")
        );
        assert!(
            parse_flow_developer_credential_prefix(&format!(
                "hcf_0123456789abcdeF_{}",
                "A".repeat(43)
            ))
            .is_err()
        );
        assert!(
            parse_flow_developer_credential_prefix(&format!(
                "hcf_0123456789abcdef_{}",
                "A".repeat(42)
            ))
            .is_err()
        );
        assert!(
            serde_json::from_value::<CreateDeveloperAccessCredential>(json!({
                "principal_id": Uuid::nil(),
                "permissions": ["flow.room.join"]
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<CreateDeveloperAccessCredential>(json!({
                "principal_id": Uuid::nil(),
                "permissions": ["flow.room.join"],
                "expires_in_seconds": 60,
                "credential": "must-not-be-accepted"
            }))
            .is_err()
        );
    }

    #[test]
    fn flow_developer_credential_names_expiry_and_lists_are_bounded() {
        assert_eq!(
            validate_developer_credential_name("application backend").ok(),
            Some("application backend")
        );
        assert!(validate_developer_credential_name(" application backend").is_err());
        assert!(validate_developer_credential_name(&"x".repeat(121)).is_err());
        assert!(
            validate_developer_credential_expiry(FLOW_DEVELOPER_CREDENTIAL_MIN_TTL_DAYS).is_ok()
        );
        assert!(
            validate_developer_credential_expiry(FLOW_DEVELOPER_CREDENTIAL_MAX_TTL_DAYS).is_ok()
        );
        assert!(validate_developer_credential_expiry(0).is_err());
        assert!(validate_developer_credential_expiry(366).is_err());
        assert_eq!(validate_list_limit(None, 100).ok(), Some(100));
        assert_eq!(validate_list_limit(Some(1), 100).ok(), Some(1));
        assert!(validate_list_limit(Some(0), 100).is_err());
        assert!(validate_list_limit(Some(101), 100).is_err());
    }

    #[test]
    fn flow_developer_credential_creation_response_has_stable_public_fields()
    -> Result<(), Box<dyn std::error::Error>> {
        let timestamp = chrono::DateTime::from_timestamp(1_785_480_000, 0)
            .ok_or("test timestamp is invalid")?;
        let response = FlowDeveloperCredentialCreationResponse {
            item: FlowDeveloperCredentialResponse {
                id: Uuid::from_u128(1),
                name: "application backend".into(),
                prefix: "hcf_0123456789abcdef".into(),
                permissions: vec!["flow.room.join".into()],
                expires_at: timestamp,
                last_used_at: None,
                revoked_at: None,
                created_at: timestamp,
            },
            credential: format!("hcf_0123456789abcdef_{}", "A".repeat(43)),
            mint_endpoint: Url::parse(
                "https://heterocloud.example.test/api/v1/flow/v1/access-credentials",
            )?,
        };
        let value = serde_json::to_value(response)?;
        let object = value.as_object().ok_or("response is not an object")?;
        assert_eq!(object.len(), 10);
        for field in [
            "id",
            "name",
            "prefix",
            "permissions",
            "expires_at",
            "last_used_at",
            "revoked_at",
            "created_at",
            "credential",
            "mint_endpoint",
        ] {
            assert!(object.contains_key(field), "missing response field {field}");
        }
        Ok(())
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
