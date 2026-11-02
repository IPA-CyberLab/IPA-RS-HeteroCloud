use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use axum_extra::extract::cookie::CookieJar;
use heterocloud_auth::{generate_token, token_hash};
use heterocloud_domain::{OrganizationId, PrincipalId, UserId};
use heterocloud_iam::semantics_digest;
use secrecy::ExposeSecret;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{
    AppState, AuthenticatedActor, authenticated_actor, authenticated_actor_mutation,
    authorize_actor, authorize_actor_with_implicit_allow, organization_resource,
    parse_policy_document, require_policy_conditions_enabled, validate_name,
};
use crate::error::ApiError;

async fn require_management(
    state: &AppState,
    actor: &AuthenticatedActor,
    account: Uuid,
    action: &str,
) -> Result<(), ApiError> {
    require_management_at(state, actor, account, action, "identity-center/*").await
}

async fn require_management_at(
    state: &AppState,
    actor: &AuthenticatedActor,
    account: Uuid,
    action: &str,
    resource: &str,
) -> Result<(), ApiError> {
    let org = state
        .store
        .cloud_organization_for_account(OrganizationId(account))
        .await
        .map_err(ApiError::from_store)?
        .ok_or(ApiError::NotFound)?;
    if org.management_account_id != account {
        return Err(ApiError::Forbidden);
    }
    authorize_actor(
        state,
        actor,
        OrganizationId(account),
        action,
        &organization_resource(account, resource),
    )
    .await?;
    if !state
        .store
        .identity_center_enabled()
        .await
        .map_err(ApiError::from_store)?
    {
        return Err(ApiError::ServiceControlUnavailable);
    }
    Ok(())
}

async fn require_human(
    state: &AppState,
    actor: &AuthenticatedActor,
    account: Uuid,
    action: &str,
    resource: &str,
) -> Result<UserId, ApiError> {
    let user = match actor {
        AuthenticatedActor::User(session) => session.user.user.id,
        AuthenticatedActor::CliToken(token) if token.organization_id.0 == account => {
            token.user.user.id
        }
        _ => return Err(ApiError::Forbidden),
    };
    let org = state
        .store
        .cloud_organization_for_account(OrganizationId(account))
        .await
        .map_err(ApiError::from_store)?
        .ok_or(ApiError::NotFound)?;
    if org.management_account_id != account {
        return Err(ApiError::Forbidden);
    }
    authorize_actor_with_implicit_allow(
        state,
        actor,
        OrganizationId(account),
        action,
        &organization_resource(account, resource),
        true,
    )
    .await?;
    if !state
        .store
        .identity_center_enabled()
        .await
        .map_err(ApiError::from_store)?
    {
        return Err(ApiError::ServiceControlUnavailable);
    }
    Ok(user)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PermissionSetInput {
    name: String,
    document: Value,
    #[serde(default = "default_duration")]
    session_duration_seconds: i32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdatePermissionSetInput {
    document: Value,
    #[serde(default = "default_duration")]
    session_duration_seconds: i32,
}

const fn default_duration() -> i32 {
    3600
}
fn validate_duration(duration: i32) -> Result<(), ApiError> {
    if !(3600..=43200).contains(&duration) {
        return Err(ApiError::BadRequest(
            "Session duration must be between one and twelve hours.".into(),
        ));
    }
    Ok(())
}
fn validate_set_name(name: &str) -> Result<(), ApiError> {
    if name.is_empty()
        || name.len() > 32
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"+=,.@_-".contains(&b))
    {
        return Err(ApiError::BadRequest(
            "Permission set names must use 1 to 32 ASCII letters, numbers, or +=,.@_-.".into(),
        ));
    }
    Ok(())
}

pub(super) async fn list_identity_center_permission_sets(
    State(state): State<Arc<AppState>>,
    Path(account): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Json<Value>, ApiError> {
    let actor = authenticated_actor(&state, &headers, &jar).await?;
    require_management(&state, &actor, account, "sso:ListPermissionSets").await?;
    let items = state
        .store
        .identity_center_permission_sets(OrganizationId(account))
        .await
        .map_err(ApiError::from_store)?;
    Ok(Json(json!({"items":items})))
}

pub(super) async fn create_identity_center_permission_set(
    State(state): State<Arc<AppState>>,
    Path(account): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(input): Json<PermissionSetInput>,
) -> Result<impl IntoResponse, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    validate_set_name(&input.name)?;
    require_management_at(
        &state,
        &actor,
        account,
        "sso:CreatePermissionSet",
        &format!("identity-center/permission-set/{}", input.name),
    )
    .await?;
    validate_duration(input.session_duration_seconds)?;
    let document = parse_policy_document(input.document)?;
    require_policy_conditions_enabled(&state, &document).await?;
    let item = state
        .store
        .create_identity_center_permission_set(
            OrganizationId(account),
            &input.name,
            &document,
            input.session_duration_seconds,
        )
        .await
        .map_err(ApiError::from_store)?;
    Ok((StatusCode::CREATED, Json(item)))
}

pub(super) async fn update_identity_center_permission_set(
    State(state): State<Arc<AppState>>,
    Path((account, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(input): Json<UpdatePermissionSetInput>,
) -> Result<impl IntoResponse, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    require_management_at(
        &state,
        &actor,
        account,
        "sso:UpdatePermissionSet",
        &format!("identity-center/permission-set/{id}"),
    )
    .await?;
    validate_duration(input.session_duration_seconds)?;
    let document = parse_policy_document(input.document)?;
    require_policy_conditions_enabled(&state, &document).await?;
    let item = state
        .store
        .update_identity_center_permission_set(
            OrganizationId(account),
            id,
            &document,
            input.session_duration_seconds,
            &semantics_digest(),
        )
        .await
        .map_err(ApiError::from_store)?;
    Ok(Json(item))
}

pub(super) async fn delete_identity_center_permission_set(
    State(state): State<Arc<AppState>>,
    Path((account, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<StatusCode, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    require_management_at(
        &state,
        &actor,
        account,
        "sso:DeletePermissionSet",
        &format!("identity-center/permission-set/{id}"),
    )
    .await?;
    state
        .store
        .delete_identity_center_permission_set(OrganizationId(account), id)
        .await
        .map_err(ApiError::from_store)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn identity_center_users(
    State(state): State<Arc<AppState>>,
    Path(account): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Json<Value>, ApiError> {
    let actor = authenticated_actor(&state, &headers, &jar).await?;
    require_management(&state, &actor, account, "identitystore:ListUsers").await?;
    let items = state
        .store
        .identity_center_users(OrganizationId(account))
        .await
        .map_err(ApiError::from_store)?;
    Ok(Json(json!({"items":items})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GroupInput {
    name: String,
}

pub(super) async fn list_identity_center_groups(
    State(state): State<Arc<AppState>>,
    Path(account): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Json<Value>, ApiError> {
    let actor = authenticated_actor(&state, &headers, &jar).await?;
    require_management(&state, &actor, account, "identitystore:ListGroups").await?;
    let items = state
        .store
        .identity_center_groups(OrganizationId(account))
        .await
        .map_err(ApiError::from_store)?;
    Ok(Json(json!({"items":items})))
}
pub(super) async fn create_identity_center_group(
    State(state): State<Arc<AppState>>,
    Path(account): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(input): Json<GroupInput>,
) -> Result<impl IntoResponse, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    validate_name(&input.name)?;
    require_management_at(
        &state,
        &actor,
        account,
        "identitystore:CreateGroup",
        "identity-center/group/*",
    )
    .await?;
    let item = state
        .store
        .create_identity_center_group(OrganizationId(account), &input.name)
        .await
        .map_err(ApiError::from_store)?;
    Ok((StatusCode::CREATED, Json(item)))
}
pub(super) async fn delete_identity_center_group(
    State(state): State<Arc<AppState>>,
    Path((account, group)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<StatusCode, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    require_management_at(
        &state,
        &actor,
        account,
        "identitystore:DeleteGroup",
        &format!("identity-center/group/{group}"),
    )
    .await?;
    state
        .store
        .delete_identity_center_group(OrganizationId(account), group)
        .await
        .map_err(ApiError::from_store)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn group_user(
    state: Arc<AppState>,
    account: Uuid,
    group: Uuid,
    user: Uuid,
    headers: HeaderMap,
    jar: CookieJar,
    add: bool,
) -> Result<StatusCode, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    require_management_at(
        &state,
        &actor,
        account,
        if add {
            "identitystore:CreateGroupMembership"
        } else {
            "identitystore:DeleteGroupMembership"
        },
        &format!("identity-center/group/{group}/user/{user}"),
    )
    .await?;
    state
        .store
        .change_identity_center_group_member(OrganizationId(account), group, UserId(user), add)
        .await
        .map_err(ApiError::from_store)?;
    Ok(StatusCode::NO_CONTENT)
}
pub(super) async fn add_identity_center_group_user(
    State(state): State<Arc<AppState>>,
    Path((account, group, user)): Path<(Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<StatusCode, ApiError> {
    group_user(state, account, group, user, headers, jar, true).await
}
pub(super) async fn remove_identity_center_group_user(
    State(state): State<Arc<AppState>>,
    Path((account, group, user)): Path<(Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<StatusCode, ApiError> {
    group_user(state, account, group, user, headers, jar, false).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AssignmentInput {
    permission_set_id: Uuid,
    account_id: Uuid,
    subject_type: String,
    subject_id: Uuid,
}

pub(super) async fn list_identity_center_assignments(
    State(state): State<Arc<AppState>>,
    Path(account): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Json<Value>, ApiError> {
    let actor = authenticated_actor(&state, &headers, &jar).await?;
    require_management(&state, &actor, account, "sso:ListAccountAssignments").await?;
    let items = state
        .store
        .identity_center_assignments(OrganizationId(account), None)
        .await
        .map_err(ApiError::from_store)?;
    Ok(Json(json!({"items":items})))
}
pub(super) async fn create_identity_center_assignment(
    State(state): State<Arc<AppState>>,
    Path(account): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(input): Json<AssignmentInput>,
) -> Result<impl IntoResponse, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    require_management_at(
        &state,
        &actor,
        account,
        "sso:CreateAccountAssignment",
        &format!(
            "identity-center/account/{}/permission-set/{}/subject/{}",
            input.account_id, input.permission_set_id, input.subject_id
        ),
    )
    .await?;
    if !matches!(input.subject_type.as_str(), "user" | "group") {
        return Err(ApiError::BadRequest(
            "Subject type must be user or group.".into(),
        ));
    }
    let item = state
        .store
        .create_identity_center_assignment(
            OrganizationId(account),
            input.permission_set_id,
            OrganizationId(input.account_id),
            &input.subject_type,
            input.subject_id,
            &semantics_digest(),
        )
        .await
        .map_err(ApiError::from_store)?;
    Ok((StatusCode::CREATED, Json(item)))
}
pub(super) async fn revoke_identity_center_assignment(
    State(state): State<Arc<AppState>>,
    Path((account, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<StatusCode, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    require_management_at(
        &state,
        &actor,
        account,
        "sso:DeleteAccountAssignment",
        &format!("identity-center/assignment/{id}"),
    )
    .await?;
    state
        .store
        .revoke_identity_center_assignment(OrganizationId(account), id)
        .await
        .map_err(ApiError::from_store)?;
    Ok(StatusCode::NO_CONTENT)
}
pub(super) async fn my_identity_center_assignments(
    State(state): State<Arc<AppState>>,
    Path(account): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Json<Value>, ApiError> {
    let actor = authenticated_actor(&state, &headers, &jar).await?;
    let user = require_human(
        &state,
        &actor,
        account,
        "sso:ListMyAssignments",
        "identity-center/self/*",
    )
    .await?;
    let items = state
        .store
        .identity_center_assignments(OrganizationId(account), Some(user))
        .await
        .map_err(ApiError::from_store)?;
    Ok(Json(json!({"items":items})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SessionInput {
    #[serde(default)]
    switch_console: bool,
}

pub(super) async fn start_identity_center_session(
    State(state): State<Arc<AppState>>,
    Path((account, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(input): Json<SessionInput>,
) -> Result<impl IntoResponse, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    let user = require_human(
        &state,
        &actor,
        account,
        "sso:StartAccountSession",
        &format!("identity-center/self/assignment/{id}"),
    )
    .await?;
    let browser = if input.switch_console {
        match &actor {
            AuthenticatedActor::User(browser) => Some(browser),
            _ => return Err(ApiError::Forbidden),
        }
    } else {
        None
    };
    let secret = generate_token().map_err(|_| ApiError::Internal)?;
    let token = format!("hcr_{}", secret.expose_secret());
    let session = state
        .store
        .create_identity_center_session(OrganizationId(account), id, user, &token_hash(&token))
        .await
        .map_err(ApiError::from_store)?;
    if let Some(browser) = browser {
        if let Err(error) = state
            .store
            .switch_browser_role(&browser.token_hash, None, session.session_id)
            .await
        {
            let _ = state
                .store
                .revoke_iam_role_session(
                    OrganizationId(session.account_id),
                    PrincipalId(session.principal_id),
                    session.session_id,
                )
                .await;
            return Err(ApiError::from_store(error));
        }
        return Ok((
            [
                (axum::http::header::CACHE_CONTROL, "no-store"),
                (axum::http::header::PRAGMA, "no-cache"),
            ],
            Json(json!({"switched":true,"session":session})),
        ));
    }
    Ok((
        [
            (axum::http::header::CACHE_CONTROL, "no-store"),
            (axum::http::header::PRAGMA, "no-cache"),
        ],
        Json(json!({"access_token":token,"session":session})),
    ))
}
