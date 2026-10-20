use super::*;
use heterocloud_store::{BackupPolicyInput, FlashBackupPolicy};

pub(super) async fn list_events(
    State(state): State<Arc<AppState>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Json<Value>, ApiError> {
    let actor = authenticated_actor(&state, &headers, &jar).await?;
    authorize_actor(
        &state,
        &actor,
        OrganizationId(org),
        "flash:ListEvents",
        &format!("hc:org:{org}:flash/instance/*"),
    )
    .await?;
    Ok(Json(
        json!({"items":state.store.flash_events(OrganizationId(org),None).await.map_err(ApiError::from_store)?}),
    ))
}

pub(super) async fn owner_events(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    jar: CookieJar,
    PeerAddress(peer): PeerAddress,
) -> Result<Json<Value>, ApiError> {
    require_owner(&state, &headers, &jar, peer, false).await?;
    Ok(Json(
        json!({"items":state.store.owner_flash_events().await.map_err(ApiError::from_store)?}),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RollbackRequest {
    pub generation: i64,
    pub expected_generation: i64,
}

pub(super) async fn revisions(
    State(state): State<Arc<AppState>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Json<Value>, ApiError> {
    let actor = authenticated_actor(&state, &headers, &jar).await?;
    authorize_actor(
        &state,
        &actor,
        OrganizationId(org),
        "flash:GetInstance",
        &flash_service_resource(org, id),
    )
    .await?;
    flash_service(&state, org, id).await?;
    Ok(Json(
        json!({"items":state.store.flash_revisions(OrganizationId(org),ServiceInstanceId(id)).await.map_err(ApiError::from_store)?}),
    ))
}

pub(super) async fn rollback(
    State(state): State<Arc<AppState>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(request): Json<RollbackRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    let authorization = authorize_actor(
        &state,
        &actor,
        OrganizationId(org),
        "flash:UpdateInstance",
        &flash_service_resource(org, id),
    )
    .await?;
    let current = flash_service(&state, org, id).await?;
    if current.generation != request.expected_generation {
        return Err(ApiError::Conflict);
    }
    let target = state
        .store
        .flash_revision(
            OrganizationId(org),
            ServiceInstanceId(id),
            request.generation,
        )
        .await
        .map_err(ApiError::from_store)?;
    if target.phase != "ready" || target.generation >= current.generation {
        return Err(ApiError::BadRequest(
            "Select an earlier healthy revision.".into(),
        ));
    }
    let spec = heterocloud_store::rollback_spec(&target.spec, &current.spec);
    let parsed: FlashSpec = serde_json::from_value(spec.clone()).map_err(|_| ApiError::Internal)?;
    validate_flash_spec(&parsed)?;
    // Access controls and bindings are taken from the current service, never
    // resurrected from a historical revision. Ordinary quota checks still run.
    let instance = state
        .store
        .update_service_instance_checked(
            OrganizationId(org),
            ServiceInstanceId(id),
            "flash",
            authorization.principal_id,
            None,
            &current.name,
            spec,
            Some(current.generation),
        )
        .await
        .map_err(ApiError::from_store)?;
    Ok((StatusCode::ACCEPTED, Json(instance)))
}

pub(super) async fn events(
    State(state): State<Arc<AppState>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Json<Value>, ApiError> {
    let actor = authenticated_actor(&state, &headers, &jar).await?;
    authorize_actor(
        &state,
        &actor,
        OrganizationId(org),
        "flash:GetInstance",
        &flash_service_resource(org, id),
    )
    .await?;
    flash_service(&state, org, id).await?;
    Ok(Json(
        json!({"items":state.store.flash_events(OrganizationId(org),Some(ServiceInstanceId(id))).await.map_err(ApiError::from_store)?}),
    ))
}

pub(super) async fn get_policy(
    State(state): State<Arc<AppState>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Json<Value>, ApiError> {
    let actor = authenticated_actor(&state, &headers, &jar).await?;
    authorize_actor(
        &state,
        &actor,
        OrganizationId(org),
        "flash:GetInstance",
        &flash_service_resource(org, id),
    )
    .await?;
    flash_service(&state, org, id).await?;
    Ok(Json(json!(
        state
            .store
            .flash_backup_policy(OrganizationId(org), ServiceInstanceId(id))
            .await
            .map_err(ApiError::from_store)?
    )))
}

pub(super) async fn put_policy(
    State(state): State<Arc<AppState>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(request): Json<BackupPolicyInput>,
) -> Result<Json<FlashBackupPolicy>, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    let authorization = authorize_actor(
        &state,
        &actor,
        OrganizationId(org),
        "flash:CreateBackup",
        &flash_service_resource(org, id),
    )
    .await?;
    // Retention is deletion authority, including when enabled later by a worker.
    authorize_actor(
        &state,
        &actor,
        OrganizationId(org),
        "flash:DeleteBackup",
        &format!("hc:org:{org}:flash/backup/*"),
    )
    .await?;
    if request.verify_restore {
        authorize_actor(
            &state,
            &actor,
            OrganizationId(org),
            "flash:RestoreBackup",
            &format!("hc:org:{org}:flash/backup/*"),
        )
        .await?;
    }
    flash_service(&state, org, id).await?;
    Ok(Json(
        state
            .store
            .put_flash_backup_policy(
                OrganizationId(org),
                ServiceInstanceId(id),
                authorization.principal_id,
                &request,
            )
            .await
            .map_err(ApiError::from_store)?,
    ))
}

pub(super) async fn run_policy(
    State(state): State<Arc<AppState>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<impl IntoResponse, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    authorize_actor(
        &state,
        &actor,
        OrganizationId(org),
        "flash:CreateBackup",
        &flash_service_resource(org, id),
    )
    .await?;
    flash_service(&state, org, id).await?;
    state
        .store
        .run_flash_backup_policy_now(OrganizationId(org), ServiceInstanceId(id))
        .await
        .map_err(ApiError::from_store)?;
    Ok((StatusCode::ACCEPTED, Json(json!({"scheduled":true}))))
}

pub(super) async fn verify_backup(
    State(state): State<Arc<AppState>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<impl IntoResponse, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    authorize_actor(
        &state,
        &actor,
        OrganizationId(org),
        "flash:RestoreBackup",
        &format!("hc:org:{org}:flash/backup/{id}"),
    )
    .await?;
    let backup = state
        .store
        .flash_backup(OrganizationId(org), id)
        .await
        .map_err(ApiError::from_store)?;
    if backup.phase != "ready" || backup.delete_requested {
        return Err(ApiError::Conflict);
    }
    let provider = state
        .flash_provider
        .as_ref()
        .ok_or(ApiError::FlashProviderUnavailable)?;
    provider
        .backup_request(
            crate::flash_backups::context(&backup),
            "flash.backups.verify",
            reqwest::Method::POST,
            Some(id),
            None,
        )
        .await
        .map_err(|_| ApiError::FlashProviderUnavailable)?;
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({"id":id,"verification_requested":true})),
    ))
}
