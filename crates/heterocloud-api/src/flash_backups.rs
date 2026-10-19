//! Durable reconciliation continues after the initiating HTTP request disconnects.
use crate::{
    flash_provider::{FlashProviderContext, FlashProviderError},
    routes::AppState,
};
use heterocloud_domain::{OrganizationId, PrincipalId, ProjectId, ServiceInstanceId};
use heterocloud_store::FlashBackup;
use reqwest::Method;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

pub fn context(backup: &FlashBackup) -> FlashProviderContext {
    FlashProviderContext {
        principal_id: PrincipalId(backup.principal_id),
        user_id: None,
        organization_id: OrganizationId(backup.organization_id),
        project_id: ProjectId(backup.project_id),
        service_instance_id: ServiceInstanceId(backup.service_instance_id),
        generation: backup.source_generation,
    }
}

pub fn validated_status(id: uuid::Uuid, value: Value) -> Result<Value, FlashProviderError> {
    if value.get("id").and_then(Value::as_str) != Some(id.to_string().as_str())
        || !matches!(
            value["phase"].as_str(),
            Some("pending" | "creating" | "ready" | "error" | "deleting")
        )
    {
        return Err(FlashProviderError::ProviderStatus(502));
    }
    // Never persist unbounded or arbitrary backend error bodies/URLs.
    Ok(
        json!({"id":id,"phase":value["phase"],"completed_at":value["completed_at"],
        "message":match value["message"].as_str() {Some("backup_storage_error"|"backup_storage_missing"|"backup_source_unavailable")=>value["message"].clone(),_=>Value::Null}}),
    )
}

pub fn spawn_reconciler(state: Arc<AppState>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(5));
        loop {
            ticker.tick().await;
            let Some(provider) = &state.flash_provider else {
                continue;
            };
            let Ok(backups) = state.store.pending_flash_backups().await else {
                continue;
            };
            for backup in backups {
                // Rotate attempted rows even when a storage/API outage persists.
                let _ = state
                    .store
                    .update_flash_backup_status(backup.id, &backup.phase, backup.status.clone())
                    .await;
                if backup.delete_requested {
                    if state
                        .store
                        .flash_backup_restore_pending(backup.id)
                        .await
                        .unwrap_or(true)
                    {
                        continue;
                    }
                    if let Ok(result) = provider
                        .backup_request(
                            context(&backup),
                            "flash.backups.delete",
                            Method::DELETE,
                            Some(backup.id),
                            None,
                        )
                        .await
                        && result["deleted"] == true
                    {
                        let _ = state.store.release_flash_backup(backup.id).await;
                    }
                    continue;
                }
                let response = if backup.phase == "pending" {
                    provider.backup_request(context(&backup),"flash.backups.create",Method::POST,None,Some(&json!({"id":backup.id,"name":backup.name,"source_pvc_uid":backup.source_pvc_uid,"volume_size_bytes":backup.volume_size_bytes}))).await
                } else {
                    provider
                        .backup_request(
                            context(&backup),
                            "flash.backups.get",
                            Method::GET,
                            Some(backup.id),
                            None,
                        )
                        .await
                };
                match response.and_then(|value| validated_status(backup.id, value)) {
                    Ok(mut value) => {
                        let phase = value["phase"].as_str().unwrap_or("error").to_owned();
                        value["last_verified_at"] = json!(chrono::Utc::now());
                        value["provider_unavailable"] = json!(false);
                        let _ = state
                            .store
                            .update_flash_backup_status(backup.id, &phase, value)
                            .await;
                    }
                    Err(FlashProviderError::ProviderStatus(400 | 403 | 404 | 409)) => {
                        let _ = state
                            .store
                            .update_flash_backup_status(
                                backup.id,
                                "error",
                                json!({"message":"backup_source_unavailable"}),
                            )
                            .await;
                    }
                    Err(_) => {
                        let mut status = backup.status.clone();
                        status["provider_unavailable"] = json!(true);
                        let _ = state
                            .store
                            .update_flash_backup_status(backup.id, &backup.phase, status)
                            .await;
                        tracing::warn!(backup=%backup.id,"backup provider unavailable; request retained for retry");
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn untrusted_provider_errors_and_wrong_backup_id_are_not_persisted()
    -> Result<(), Box<dyn std::error::Error>> {
        let id = uuid::Uuid::now_v7();
        let value = validated_status(
            id,
            json!({"id":id,"phase":"error","message":"s3://private:credential@bucket/secret-path","other_secret":"must-not-copy"}),
        )?;
        assert!(value["message"].is_null());
        assert!(value.get("other_secret").is_none());
        assert!(validated_status(id, json!({"id":uuid::Uuid::now_v7(),"phase":"ready"})).is_err());
        assert!(validated_status(id, json!({"id":id,"phase":"unknown"})).is_err());
        Ok(())
    }
}
