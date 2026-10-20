use crate::{Store, StoreError};
use chrono::{DateTime, Utc};
use heterocloud_domain::{OrganizationId, PrincipalId, ServiceInstance, ServiceInstanceId};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct FlashRevision {
    pub service_instance_id: Uuid,
    pub generation: i64,
    pub organization_id: Uuid,
    pub project_id: Uuid,
    #[serde(skip_serializing)]
    pub principal_id: Option<Uuid>,
    pub name: String,
    #[serde(skip_serializing)]
    pub spec: Value,
    pub phase: String,
    pub created_at: DateTime<Utc>,
    pub ready_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing)]
    pub activation_started_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct FlashEvent {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub service_instance_id: Uuid,
    pub generation: i64,
    pub code: String,
    pub severity: String,
    pub message: String,
    pub created_at: DateTime<Utc>,
    pub last_observed_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub acknowledged_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BackupPolicyInput {
    pub enabled: bool,
    pub interval_seconds: i32,
    pub retention_count: i32,
    #[serde(default = "verify_default")]
    pub verify_restore: bool,
}
fn verify_default() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct FlashBackupPolicy {
    pub service_instance_id: Uuid,
    pub organization_id: Uuid,
    #[serde(skip_serializing)]
    pub principal_id: Uuid,
    pub enabled: bool,
    pub interval_seconds: i32,
    pub retention_count: i32,
    pub verify_restore: bool,
    pub next_run_at: DateTime<Utc>,
    pub last_run_at: Option<DateTime<Utc>>,
    pub last_backup_id: Option<Uuid>,
    pub last_error: Option<String>,
    pub updated_at: DateTime<Utc>,
    #[serde(skip_serializing)]
    pub lease_until: Option<DateTime<Utc>>,
}

/// Revert execution settings only. Current IAM, secrets, exposure and network
/// restrictions remain authoritative, and persistent storage is never shrunk.
pub fn rollback_spec(target: &Value, current: &Value) -> Value {
    let mut next = target.clone();
    for key in [
        "task_role",
        "env",
        "secret_env",
        "secret_files",
        "network",
        "exposure",
        "egress",
        "stopped",
        "restore_from_backup_id",
        "metadata",
        "region",
    ] {
        if let Some(value) = current.get(key) {
            next[key] = value.clone();
        } else if let Some(object) = next.as_object_mut() {
            object.remove(key);
        }
    }
    for key in ["ephemeral_storage_gib", "persistent_storage_gib"] {
        if let Some(value) = current.get(key).and_then(Value::as_u64) {
            let previous = next.get(key).and_then(Value::as_u64).unwrap_or_default();
            next[key] = json!(previous.max(value));
        }
    }
    next
}

impl Store {
    pub async fn owner_flash_events(&self) -> Result<Vec<FlashEvent>, StoreError> {
        Ok(sqlx::query_as("SELECT * FROM flash_service_events ORDER BY (resolved_at IS NULL) DESC,created_at DESC LIMIT 200").fetch_all(&self.pool).await?)
    }
    pub async fn run_flash_backup_policy_now(
        &self,
        org: OrganizationId,
        service: ServiceInstanceId,
    ) -> Result<(), StoreError> {
        let result=sqlx::query("UPDATE flash_backup_policies SET next_run_at=now(),updated_at=now() WHERE organization_id=$1 AND service_instance_id=$2 AND enabled AND (lease_until IS NULL OR lease_until<now())")
            .bind(org.0).bind(service.0).execute(&self.pool).await?;
        if result.rows_affected() == 0 {
            return Err(StoreError::Conflict);
        }
        Ok(())
    }
    pub async fn record_flash_event(
        &self,
        instance: &ServiceInstance,
        code: &str,
        severity: &str,
        message: &str,
    ) -> Result<(), StoreError> {
        sqlx::query("INSERT INTO flash_service_events(id,organization_id,service_instance_id,generation,code,severity,message) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(service_instance_id,generation,code) DO UPDATE SET last_observed_at=now(),resolved_at=NULL")
            .bind(Uuid::now_v7()).bind(instance.organization_id.0).bind(instance.id.0).bind(instance.generation).bind(code).bind(severity).bind(message).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn mark_flash_revision_rolled_back(
        &self,
        service: ServiceInstanceId,
        generation: i64,
    ) -> Result<(), StoreError> {
        sqlx::query("UPDATE flash_service_revisions SET phase='rolled_back' WHERE service_instance_id=$1 AND generation=$2 AND ready_at IS NULL")
            .bind(service.0).bind(generation).execute(&self.pool).await?;
        Ok(())
    }
    pub async fn flash_revisions(
        &self,
        org: OrganizationId,
        service: ServiceInstanceId,
    ) -> Result<Vec<FlashRevision>, StoreError> {
        Ok(sqlx::query_as("SELECT * FROM flash_service_revisions WHERE organization_id=$1 AND service_instance_id=$2 ORDER BY generation DESC LIMIT 30")
            .bind(org.0).bind(service.0).fetch_all(&self.pool).await?)
    }

    pub async fn flash_revision(
        &self,
        org: OrganizationId,
        service: ServiceInstanceId,
        generation: i64,
    ) -> Result<FlashRevision, StoreError> {
        sqlx::query_as("SELECT * FROM flash_service_revisions WHERE organization_id=$1 AND service_instance_id=$2 AND generation=$3")
            .bind(org.0).bind(service.0).bind(generation).fetch_optional(&self.pool).await?.ok_or(StoreError::NotFound)
    }

    pub async fn flash_operation_targets(&self) -> Result<Vec<ServiceInstance>, StoreError> {
        sqlx::query_as::<_,crate::ServiceRow>("SELECT id,organization_id,project_id,provider,name,generation,state,spec,status,created_at,updated_at FROM service_instances WHERE provider='flash' AND state<>'deleting' ORDER BY id").fetch_all(&self.pool).await?.into_iter().map(ServiceInstance::try_from).collect()
    }

    pub async fn flash_events(
        &self,
        org: OrganizationId,
        service: Option<ServiceInstanceId>,
    ) -> Result<Vec<FlashEvent>, StoreError> {
        Ok(sqlx::query_as("SELECT * FROM flash_service_events WHERE organization_id=$1 AND ($2::uuid IS NULL OR service_instance_id=$2) ORDER BY created_at DESC LIMIT 100")
            .bind(org.0).bind(service.map(|s|s.0)).fetch_all(&self.pool).await?)
    }

    pub async fn acknowledge_flash_event(
        &self,
        org: OrganizationId,
        id: Uuid,
    ) -> Result<(), StoreError> {
        let result = sqlx::query("UPDATE flash_service_events SET acknowledged_at=now() WHERE id=$1 AND organization_id=$2")
            .bind(id).bind(org.0).execute(&self.pool).await?;
        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    pub async fn record_flash_observation(
        &self,
        instance: &ServiceInstance,
        status: &Value,
    ) -> Result<(), StoreError> {
        // A provider can still report the preceding runtime generation while an
        // update is reconciling. It cannot start this revision's activation timer,
        // change its state or resolve its notifications.
        if status["observed_generation"].as_i64() != Some(instance.generation) {
            return Ok(());
        }
        let mut tx = self.pool.begin().await?;
        let phase = status["phase"].as_str().unwrap_or("provisioning");
        let state = match phase {
            "ready" => "ready",
            "error" => "error",
            _ => "updating",
        };
        let result = sqlx::query("UPDATE service_instances SET status=jsonb_build_object('status',$4::jsonb),state=$5 WHERE id=$1 AND organization_id=$2 AND generation=$3 AND provider='flash' AND state<>'deleting'")
            .bind(instance.id.0).bind(instance.organization_id.0).bind(instance.generation).bind(status).bind(state).execute(&mut *tx).await?;
        if result.rows_affected() == 0 {
            tx.rollback().await?;
            return Ok(());
        }
        if status["desired_replicas"].as_u64().is_some_and(|n| n > 0)
            && instance.spec["stopped"] != true
        {
            sqlx::query("UPDATE flash_service_revisions SET activation_started_at=coalesce(activation_started_at,now()) WHERE service_instance_id=$1 AND generation=$2")
                .bind(instance.id.0).bind(instance.generation).execute(&mut *tx).await?;
        }
        let mut codes = Vec::new();
        for diagnostic in status["diagnostics"]
            .as_array()
            .into_iter()
            .flatten()
            .take(16)
        {
            let Some(code) = diagnostic["code"].as_str().filter(|s| {
                !s.is_empty()
                    && s.len() <= 80
                    && s.bytes().all(|c| c.is_ascii_lowercase() || c == b'_')
            }) else {
                continue;
            };
            let severity = diagnostic["severity"].as_str().unwrap_or("warning");
            if !matches!(severity, "info" | "warning" | "error") {
                continue;
            }
            let message = diagnostic["message"]
                .as_str()
                .unwrap_or("Service requires attention.");
            if message.len() > 1024 {
                continue;
            }
            codes.push(code.to_owned());
            sqlx::query("INSERT INTO flash_service_events(id,organization_id,service_instance_id,generation,code,severity,message) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(service_instance_id,generation,code) DO UPDATE SET last_observed_at=now(),resolved_at=NULL,severity=EXCLUDED.severity,message=EXCLUDED.message")
                .bind(Uuid::now_v7()).bind(instance.organization_id.0).bind(instance.id.0).bind(instance.generation).bind(code).bind(severity).bind(message).execute(&mut *tx).await?;
        }
        sqlx::query("UPDATE flash_service_events SET resolved_at=now() WHERE service_instance_id=$1 AND resolved_at IS NULL AND (generation<>$2 OR NOT(code=ANY($3)))")
            .bind(instance.id.0).bind(instance.generation).bind(codes).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn flash_backup_policy(
        &self,
        org: OrganizationId,
        service: ServiceInstanceId,
    ) -> Result<Option<FlashBackupPolicy>, StoreError> {
        Ok(sqlx::query_as("SELECT * FROM flash_backup_policies WHERE organization_id=$1 AND service_instance_id=$2")
            .bind(org.0).bind(service.0).fetch_optional(&self.pool).await?)
    }

    pub async fn active_backup_policies(&self) -> Result<Vec<FlashBackupPolicy>, StoreError> {
        Ok(sqlx::query_as("SELECT p.* FROM flash_backup_policies p JOIN service_instances s ON s.id=p.service_instance_id WHERE p.enabled AND s.state<>'deleting' ORDER BY p.next_run_at")
            .fetch_all(&self.pool).await?)
    }

    pub async fn put_flash_backup_policy(
        &self,
        org: OrganizationId,
        service: ServiceInstanceId,
        principal: PrincipalId,
        input: &BackupPolicyInput,
    ) -> Result<FlashBackupPolicy, StoreError> {
        if !(3600..=604800).contains(&input.interval_seconds)
            || !(1..=8).contains(&input.retention_count)
        {
            return Err(StoreError::RequestRejected(
                "Backup interval must be 1 hour to 7 days and retention 1 to 8 generations.".into(),
            ));
        }
        sqlx::query_as("INSERT INTO flash_backup_policies(service_instance_id,organization_id,principal_id,enabled,interval_seconds,retention_count,verify_restore) SELECT id,organization_id,$3,$4,$5,$6,$7 FROM service_instances WHERE id=$1 AND organization_id=$2 AND provider='flash' AND state<>'deleting' ON CONFLICT(service_instance_id) DO UPDATE SET principal_id=$3,enabled=$4,interval_seconds=$5,retention_count=$6,verify_restore=$7,updated_at=now() RETURNING *")
            .bind(service.0).bind(org.0).bind(principal.0).bind(input.enabled).bind(input.interval_seconds).bind(input.retention_count).bind(input.verify_restore).fetch_optional(&self.pool).await?.ok_or(StoreError::NotFound)
    }

    pub async fn claim_backup_policies(&self) -> Result<Vec<FlashBackupPolicy>, StoreError> {
        Ok(sqlx::query_as("UPDATE flash_backup_policies SET lease_until=now()+interval '2 minutes' WHERE service_instance_id IN (SELECT p.service_instance_id FROM flash_backup_policies p JOIN service_instances s ON s.id=p.service_instance_id WHERE p.enabled AND p.next_run_at<=now() AND (p.lease_until IS NULL OR p.lease_until<now()) AND s.state<>'deleting' ORDER BY p.next_run_at FOR UPDATE OF p SKIP LOCKED LIMIT 4) RETURNING *")
            .fetch_all(&self.pool).await?)
    }

    pub async fn finish_backup_policy(
        &self,
        policy: &FlashBackupPolicy,
        backup: Option<Uuid>,
        error: Option<&str>,
    ) -> Result<(), StoreError> {
        sqlx::query("UPDATE flash_backup_policies SET lease_until=NULL,last_run_at=now(),last_backup_id=coalesce($2,last_backup_id),last_error=$3,next_run_at=now()+make_interval(secs=>CASE WHEN $3::text IS NULL THEN interval_seconds ELSE least(interval_seconds,900) END),updated_at=now() WHERE service_instance_id=$1 AND lease_until=$4 AND principal_id=$5 AND updated_at=$6")
            .bind(policy.service_instance_id).bind(backup).bind(error).bind(policy.lease_until).bind(policy.principal_id).bind(policy.updated_at).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn attach_scheduled_backup(
        &self,
        policy: &FlashBackupPolicy,
        id: Uuid,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "UPDATE flash_backups SET schedule_service_id=$2 WHERE id=$1 AND organization_id=$3",
        )
        .bind(id)
        .bind(policy.service_instance_id)
        .bind(policy.organization_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn expire_scheduled_backups(
        &self,
        policy: &FlashBackupPolicy,
    ) -> Result<(), StoreError> {
        // Only successfully uploaded scheduled backups count toward retention.
        // Manual backups and the last known good backup are never pruned here.
        sqlx::query("UPDATE flash_backups SET delete_requested=true,phase='deleting',updated_at=now() WHERE id IN (SELECT id FROM flash_backups WHERE schedule_service_id=$1 AND organization_id=$2 AND phase='ready' AND NOT delete_requested AND (NOT $4 OR status->'restore_verification'->>'phase'='passed') ORDER BY created_at DESC OFFSET $3)")
            .bind(policy.service_instance_id).bind(policy.organization_id).bind(i64::from(policy.retention_count)).bind(policy.verify_restore).execute(&self.pool).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rollback_keeps_current_security_and_never_shrinks_a_disk() {
        let old = json!({"image":"old","memory_mib":256,"ephemeral_storage_gib":8,"persistent_storage_gib":5,"env":{"OLD":"secret"},"task_role":"revoked","exposure":{"type":"public"},"metadata":{"old":true}});
        let current = json!({"image":"broken","memory_mib":512,"ephemeral_storage_gib":32,"persistent_storage_gib":24,"env":{"CURRENT":"keep"},"exposure":{"type":"internal"},"stopped":true,"metadata":{"new":true}});
        let next = rollback_spec(&old, &current);
        assert_eq!(next["image"], "old");
        assert_eq!(next["memory_mib"], 256);
        assert_eq!(next["ephemeral_storage_gib"], 32);
        assert_eq!(next["persistent_storage_gib"], 24);
        assert_eq!(next["env"], current["env"]);
        assert_eq!(next["exposure"], current["exposure"]);
        assert!(next.get("task_role").is_none());
        assert_eq!(next["stopped"], true);
    }
}
