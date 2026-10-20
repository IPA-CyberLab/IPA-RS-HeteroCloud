//! Continuous status, alert and backup-policy reconciliation. Every write is
//! generation-fenced; a delayed observation cannot overwrite a newer user edit.
use crate::{flash_provider::FlashProviderContext, routes::AppState};
use chrono::Utc;
use futures_util::{StreamExt, stream};
use heterocloud_domain::{
    DeploymentPolicy, OrganizationId, PrincipalId, ServiceInstance, ServiceInstanceId, ServiceState,
};
use heterocloud_iam::{AuthorizationRequest, Decision, authorize};
use heterocloud_store::{FlashBackupPolicy, rollback_spec};
use reqwest::Method;
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

pub fn context(instance: &ServiceInstance, principal: PrincipalId) -> FlashProviderContext {
    FlashProviderContext {
        principal_id: principal,
        user_id: None,
        organization_id: instance.organization_id,
        project_id: instance.project_id,
        service_instance_id: instance.id,
        generation: instance.generation,
    }
}

pub async fn allowed(
    state: &AppState,
    principal: PrincipalId,
    org: OrganizationId,
    action: &str,
    resource: &str,
) -> bool {
    // Browser authentication normally checks these flags. Background actions
    // have no browser token, so validate the principal/user before resolving
    // the owner's membership-based permissions as well.
    if !matches!(
        state
            .store
            .authorization_context_for_principal(principal, org)
            .await,
        Ok(Some(_))
    ) {
        return false;
    }
    let context = match state.store.principal_user_id(principal).await {
        Ok(Some(user)) => state.store.authorization_context(user, org).await,
        Ok(None) => {
            state
                .store
                .authorization_context_for_principal(principal, org)
                .await
        }
        Err(_) => return false,
    };
    let Ok(Some(context)) = context else {
        return false;
    };
    context.role == "owner"
        || authorize(
            &AuthorizationRequest {
                principal_organization_id: org,
                resource_organization_id: org,
                action,
                resource,
            },
            &context.policies,
        )
        .is_ok_and(|e| e.decision == Decision::Allow)
}

pub fn spawn_reconciler(state: Arc<AppState>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(15));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            let Some(provider) = &state.flash_provider else {
                continue;
            };
            if let Ok(instances) = state.store.flash_operation_targets().await {
                stream::iter(instances)
                    .map(|instance| {
                        let state = Arc::clone(&state);
                        async move {
                            observe(&state, instance).await;
                        }
                    })
                    .buffer_unordered(8)
                    .collect::<Vec<_>>()
                    .await;
            }
            if let Ok(policies) = state.store.claim_backup_policies().await {
                for policy in policies {
                    let result = run_backup_policy(&state, &policy).await;
                    let (backup, error) = match result {
                        Ok(id) => (Some(id), None),
                        Err(code) => (None, Some(code)),
                    };
                    let _ = state
                        .store
                        .finish_backup_policy(&policy, backup, error)
                        .await;
                }
            }
            // Retention runs after upload/restore verification, including when
            // the scheduler process restarted between those steps.
            if let Ok(policies) = state.store.active_backup_policies().await {
                for policy in policies {
                    let principal = PrincipalId(policy.principal_id);
                    let org = OrganizationId(policy.organization_id);
                    let can_delete = allowed(
                        &state,
                        principal,
                        org,
                        "flash:DeleteBackup",
                        &format!("hc:org:{org}:flash/backup/*"),
                    )
                    .await;
                    if policy.verify_restore {
                        if !allowed(
                            &state,
                            principal,
                            org,
                            "flash:RestoreBackup",
                            &format!("hc:org:{org}:flash/backup/*"),
                        )
                        .await
                        {
                            continue;
                        }
                        if let Ok(backups) = state
                            .store
                            .list_flash_backups(
                                OrganizationId(policy.organization_id),
                                Some(ServiceInstanceId(policy.service_instance_id)),
                            )
                            .await
                        {
                            for backup in backups
                                .into_iter()
                                .filter(|b| {
                                    b.schedule_service_id == Some(policy.service_instance_id)
                                        && b.phase == "ready"
                                        && b.status["restore_verification"]["phase"].is_null()
                                })
                                .take(1)
                            {
                                let _ = provider
                                    .backup_request(
                                        crate::flash_backups::context(&backup),
                                        "flash.backups.verify",
                                        Method::POST,
                                        Some(backup.id),
                                        None,
                                    )
                                    .await;
                            }
                        }
                    }
                    if can_delete {
                        let _ = state.store.expire_scheduled_backups(&policy).await;
                    }
                }
            }
        }
    })
}

async fn observe(state: &AppState, instance: ServiceInstance) {
    let Some(provider) = &state.flash_provider else {
        return;
    };
    let Ok(status) = provider
        .get_status(context(&instance, PrincipalId(Uuid::nil())))
        .await
    else {
        let _=state.store.record_flash_event(&instance,"provider_unavailable","warning","The current service status could not be retrieved. The last observed state is retained.").await;
        return;
    };
    if state
        .store
        .record_flash_observation(&instance, &status)
        .await
        .is_err()
    {
        return;
    }
    if instance.spec["stopped"] == true || instance.state == ServiceState::Deleting {
        return;
    }
    if [
        "cpu_quota_exhausted",
        "memory_quota_exhausted",
        "gpu_quota_exhausted",
    ]
    .iter()
    .any(|key| status[*key] == true)
    {
        return; // Runtime rollback cannot repair an intentionally exhausted quota.
    }
    let Ok(current) = state
        .store
        .flash_revision(instance.organization_id, instance.id, instance.generation)
        .await
    else {
        return;
    };
    let policy: DeploymentPolicy = instance
        .spec
        .get("deployment")
        .filter(|v| !v.is_null())
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let Some(activation) = current.activation_started_at.or_else(|| {
        (instance.spec["autoscaling"]["min_replicas"] != 0).then_some(current.created_at)
    }) else {
        return;
    };
    if !policy.auto_rollback
        || current.ready_at.is_some()
        || Utc::now().signed_duration_since(activation).num_seconds()
            < i64::from(policy.deadline_seconds)
    {
        return;
    }
    let Ok(revisions) = state
        .store
        .flash_revisions(instance.organization_id, instance.id)
        .await
    else {
        return;
    };
    let Some(previous) = revisions
        .into_iter()
        .find(|r| r.generation < instance.generation && r.phase == "ready")
    else {
        return;
    };
    let Some(principal) = current.principal_id.map(PrincipalId) else {
        return;
    };
    let resource = format!(
        "hc:org:{}:flash/instance/{}",
        instance.organization_id, instance.id
    );
    if !allowed(
        state,
        principal,
        instance.organization_id,
        "flash:UpdateInstance",
        &resource,
    )
    .await
    {
        return;
    }
    let spec = rollback_spec(&previous.spec, &instance.spec);
    if state
        .store
        .update_service_instance_checked(
            instance.organization_id,
            instance.id,
            "flash",
            principal,
            None,
            &instance.name,
            spec,
            Some(instance.generation),
        )
        .await
        .is_ok()
    {
        let _ = state
            .store
            .mark_flash_revision_rolled_back(instance.id, instance.generation)
            .await;
        let _=state.store.record_flash_event(&instance,"deployment_rolled_back","warning","The new revision did not become healthy before its deadline. Execution settings were rolled back; current access controls and persistent data were retained.").await;
    }
}

async fn run_backup_policy(
    state: &AppState,
    policy: &FlashBackupPolicy,
) -> Result<Uuid, &'static str> {
    let principal = PrincipalId(policy.principal_id);
    let org = OrganizationId(policy.organization_id);
    let resource = format!("hc:org:{org}:flash/instance/{}", policy.service_instance_id);
    if !allowed(state, principal, org, "flash:CreateBackup", &resource).await {
        return Err("backup_permission_revoked");
    }
    let backup_resource = format!("hc:org:{org}:flash/backup/*");
    if !allowed(
        state,
        principal,
        org,
        "flash:DeleteBackup",
        &backup_resource,
    )
    .await
        || (policy.verify_restore
            && !allowed(
                state,
                principal,
                org,
                "flash:RestoreBackup",
                &backup_resource,
            )
            .await)
    {
        return Err("backup_permission_revoked");
    }
    let instance = state
        .store
        .service_instance(ServiceInstanceId(policy.service_instance_id))
        .await
        .map_err(|_| "backup_source_unavailable")?
        .filter(|s| {
            s.organization_id == org && s.provider == "flash" && s.state != ServiceState::Deleting
        })
        .ok_or("backup_source_unavailable")?;
    let provider = state
        .flash_provider
        .as_ref()
        .ok_or("backup_provider_unavailable")?;
    let observed = provider
        .backup_request(
            context(&instance, principal),
            "flash.backups.source",
            Method::GET,
            None,
            None,
        )
        .await
        .map_err(|_| "backup_source_unavailable")?;
    let hash = Sha256::digest(
        format!(
            "{}:{}",
            policy.service_instance_id,
            policy.next_run_at.timestamp()
        )
        .as_bytes(),
    );
    let request_id = Uuid::from_bytes(
        hash[..16]
            .try_into()
            .map_err(|_| "backup_request_invalid")?,
    );
    let name = format!("scheduled-{}", policy.next_run_at.format("%Y%m%d-%H%M%S"));
    let backup = state
        .store
        .request_flash_backup(
            org,
            instance.id,
            principal,
            request_id,
            &name,
            observed["volume_size_bytes"]
                .as_i64()
                .ok_or("backup_source_unavailable")?,
            observed["source_pvc_uid"]
                .as_str()
                .ok_or("backup_source_unavailable")?,
            observed["resolved_image"].as_str(),
            instance.generation,
        )
        .await
        .map_err(|_| "backup_quota_or_source_unavailable")?;
    state
        .store
        .attach_scheduled_backup(policy, backup.id)
        .await
        .map_err(|_| "backup_storage_unavailable")?;
    Ok(backup.id)
}
