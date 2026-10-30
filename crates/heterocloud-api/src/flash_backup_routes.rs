use super::*;
use crate::flash_backups::{context as backup_context, validated_status};
use crate::flash_provider::FlashProviderError;
use heterocloud_store::FlashBackup;
use reqwest::Method;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BackupListQuery {
    service_instance_id: Option<Uuid>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateBackup {
    name: String,
    request_id: Uuid,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RestoreBackup {
    project_id: Uuid,
    name: String,
    request_id: Uuid,
}

fn resource(org: Uuid, id: Uuid) -> String {
    format!("hc:org:{org}:flash/backup/{id}")
}
fn provider(state: &AppState) -> Result<&FlashProviderProxy, ApiError> {
    state
        .flash_provider
        .as_deref()
        .ok_or(ApiError::FlashProviderUnavailable)
}

pub(super) async fn list_backups(
    State(state): State<Arc<AppState>>,
    Path(org): Path<Uuid>,
    Query(query): Query<BackupListQuery>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Json<Value>, ApiError> {
    let actor = authenticated_actor(&state, &headers, &jar).await?;
    let target = query
        .service_instance_id
        .map(|id| flash_service_resource(org, id))
        .unwrap_or_else(|| format!("hc:org:{org}:flash/backup/*"));
    authorize_actor(
        &state,
        &actor,
        OrganizationId(org),
        "flash:ListBackups",
        &target,
    )
    .await?;
    let items = state
        .store
        .list_flash_backups(
            OrganizationId(org),
            query.service_instance_id.map(ServiceInstanceId),
        )
        .await
        .map_err(ApiError::from_store)?;
    Ok(Json(json!({"items":items})))
}

pub(super) async fn get_backup(
    State(state): State<Arc<AppState>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Json<FlashBackup>, ApiError> {
    let actor = authenticated_actor(&state, &headers, &jar).await?;
    authorize_actor(
        &state,
        &actor,
        OrganizationId(org),
        "flash:GetBackup",
        &resource(org, id),
    )
    .await?;
    Ok(Json(
        state
            .store
            .flash_backup(OrganizationId(org), id)
            .await
            .map_err(ApiError::from_store)?,
    ))
}

pub(super) async fn create_backup(
    State(state): State<Arc<AppState>>,
    Path((org, service_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(request): Json<CreateBackup>,
) -> Result<impl IntoResponse, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    validate_name(&request.name)?;
    if request.request_id.is_nil() {
        return Err(ApiError::BadRequest(
            "request_id must be a nonzero UUID.".into(),
        ));
    }
    let authorization = authorize_actor(
        &state,
        &actor,
        OrganizationId(org),
        "flash:CreateBackup",
        &flash_service_resource(org, service_id),
    )
    .await?;
    if let Some(backup) = state
        .store
        .flash_backup_request(
            OrganizationId(org),
            authorization.principal_id,
            request.request_id,
            ServiceInstanceId(service_id),
            &request.name,
        )
        .await
        .map_err(ApiError::from_store)?
    {
        return Ok((StatusCode::ACCEPTED, Json(backup)));
    }
    let service = flash_service(&state, org, service_id).await?;
    let observed = provider(&state)?
        .backup_request(
            flash_provider_context(&service, authorization.principal_id),
            "flash.backups.source",
            Method::GET,
            None,
            None,
        )
        .await
        .map_err(|error| match error {
            FlashProviderError::ProviderStatus(400) => {
                ApiError::BadRequest("This persistent disk does not support backups.".into())
            }
            FlashProviderError::ProviderStatus(404 | 409) => ApiError::Conflict,
            _ => ApiError::FlashProviderUnavailable,
        })?;
    let size = observed["volume_size_bytes"]
        .as_u64()
        .and_then(|v| i64::try_from(v).ok())
        .ok_or(ApiError::FlashProviderUnavailable)?;
    let pvc_uid = observed["source_pvc_uid"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or(ApiError::FlashProviderUnavailable)?;
    let generation = observed["generation"]
        .as_i64()
        .ok_or(ApiError::FlashProviderUnavailable)?;
    let backup = state
        .store
        .request_flash_backup(
            OrganizationId(org),
            ServiceInstanceId(service_id),
            authorization.principal_id,
            request.request_id,
            &request.name,
            size,
            pvc_uid,
            observed["resolved_image"].as_str(),
            generation,
        )
        .await
        .map_err(ApiError::from_store)?;
    Ok((StatusCode::ACCEPTED, Json(backup)))
}

pub(super) async fn delete_backup(
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
        "flash:DeleteBackup",
        &resource(org, id),
    )
    .await?;
    let backup = state
        .store
        .request_flash_backup_delete(OrganizationId(org), id)
        .await
        .map_err(ApiError::from_store)?;
    Ok((StatusCode::ACCEPTED, Json(backup)))
}

/// Restore only disk contents. New identities, network access and secrets must
/// be configured for the new service; no app is started implicitly.
pub(super) fn restored_spec(value: Value, id: Uuid) -> Result<FlashSpec, ApiError> {
    let mut spec: FlashSpec = serde_json::from_value(value).map_err(|_| ApiError::Internal)?;
    spec.restore_from_backup_id = Some(id);
    spec.stopped = true;
    spec.replicas = 1;
    spec.autoscaling = None;
    spec.task_role = None;
    spec.network = None;
    spec.env.clear();
    spec.secret_env.clear();
    spec.secret_files.clear();
    spec.gpu_type = None;
    spec.exposure.exposure_type = heterocloud_domain::FlashExposureType::Internal;
    spec.exposure.traffic_mode = heterocloud_domain::FlashTrafficMode::Forwarded;
    spec.exposure.endpoint_mode = heterocloud_domain::FlashEndpointMode::Ip;
    spec.exposure.authentication = None;
    spec.exposure.allowed_source_cidrs.clear();
    spec.exposure.denied_source_cidrs.clear();
    spec.egress = heterocloud_domain::FlashEgress {
        mode: heterocloud_domain::FlashEgressMode::Disabled,
        ..Default::default()
    };
    for port in &mut spec.ports {
        port.service_port = 0;
    }
    validate_flash_spec(&spec)?;
    Ok(spec)
}

pub(super) async fn restore_backup(
    State(state): State<Arc<AppState>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(request): Json<RestoreBackup>,
) -> Result<impl IntoResponse, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    validate_name(&request.name)?;
    if request.request_id.is_nil() {
        return Err(ApiError::BadRequest(
            "request_id must be a nonzero UUID.".into(),
        ));
    }
    let authorization = authorize_actor(
        &state,
        &actor,
        OrganizationId(org),
        "flash:RestoreBackup",
        &resource(org, id),
    )
    .await?;
    authorize_actor(
        &state,
        &actor,
        OrganizationId(org),
        "flash:CreateInstance",
        &flash_collection_resource(org),
    )
    .await?;
    let backup = state
        .store
        .flash_backup(OrganizationId(org), id)
        .await
        .map_err(ApiError::from_store)?;
    if backup.delete_requested || backup.phase != "ready" {
        return Err(ApiError::Conflict);
    }
    let remote = provider(&state)?
        .backup_request(
            backup_context(&backup),
            "flash.backups.get",
            Method::GET,
            Some(id),
            None,
        )
        .await
        .map_err(|_| ApiError::FlashProviderUnavailable)?;
    let remote = validated_status(id, remote).map_err(|_| ApiError::FlashProviderUnavailable)?;
    if remote["phase"] != "ready" {
        return Err(ApiError::Conflict);
    }
    let spec = restored_spec(backup.source_spec, id)?;
    let target = state
        .store
        .reserve_flash_restore(
            OrganizationId(org),
            authorization.principal_id,
            request.request_id,
            id,
            ProjectId(request.project_id),
            &request.name,
        )
        .await
        .map_err(ApiError::from_store)?;
    let quota_user = quota_user_for_actor(&state, &actor, OrganizationId(org)).await?;
    let service = state
        .store
        .restore_service_instance_for_user(
            OrganizationId(org),
            ProjectId(request.project_id),
            authorization.principal_id,
            "flash",
            &request.name,
            serde_json::to_value(spec).map_err(|_| ApiError::Internal)?,
            target,
            quota_user,
        )
        .await
        .map_err(ApiError::from_store)?;
    Ok((StatusCode::ACCEPTED, Json(service)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restored_workload_cannot_start_with_old_network_or_identity()
    -> Result<(), Box<dyn std::error::Error>> {
        let input = json!({"region":"test","image":"nginx:stable","replicas":2,"cpu_millis":100,"memory_mib":128,
            "task_role":Uuid::now_v7(),"ports":[{"name":"http","protocol":"tcp","container_port":80,"service_port":32123}],
            "exposure":{"type":"public","traffic_mode":"forwarded","endpoint_mode":"web"},
            "secret_env":{"PASSWORD":"old-service-password"},"env":{},"command":[],"args":[],"metadata":{}});
        let id = Uuid::now_v7();
        let spec = restored_spec(input, id)?;
        assert!(spec.stopped);
        assert_eq!(spec.restore_from_backup_id, Some(id));
        assert_eq!(
            spec.exposure.exposure_type,
            heterocloud_domain::FlashExposureType::Internal
        );
        assert_eq!(
            spec.egress.mode,
            heterocloud_domain::FlashEgressMode::Disabled
        );
        assert!(spec.secret_env.is_empty() && spec.task_role.is_none() && spec.network.is_none());
        assert_eq!(spec.ports[0].service_port, 0);
        Ok(())
    }
}
