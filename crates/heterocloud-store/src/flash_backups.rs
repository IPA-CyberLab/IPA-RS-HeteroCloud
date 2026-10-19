use chrono::{DateTime, Utc};
use heterocloud_domain::{OrganizationId, PrincipalId, ProjectId, ServiceInstanceId};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::{Store, StoreError, lock_tenant_allocations};

/// The provider owns data transfer; PostgreSQL durably owns user intent and quotas.
#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct FlashBackup {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub service_instance_id: Uuid,
    #[serde(skip_serializing)]
    pub principal_id: Uuid,
    #[serde(skip_serializing)]
    pub request_id: Uuid,
    pub name: String,
    pub source_name: String,
    #[serde(skip_serializing)]
    pub source_generation: i64,
    #[serde(skip_serializing)]
    pub source_pvc_uid: String,
    #[serde(skip_serializing)]
    pub source_spec: Value,
    pub volume_size_bytes: i64,
    pub phase: String,
    pub status: Value,
    pub delete_requested: bool,
    #[serde(skip_serializing)]
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Store {
    pub async fn flash_backup_request(
        &self,
        organization: OrganizationId,
        principal: PrincipalId,
        request_id: Uuid,
        service: ServiceInstanceId,
        name: &str,
    ) -> Result<Option<FlashBackup>, StoreError> {
        let existing = sqlx::query_as::<_, FlashBackup>(
            "SELECT * FROM flash_backups WHERE organization_id=$1 AND request_id=$2",
        )
        .bind(organization.0)
        .bind(request_id)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(ref backup) = existing
            && (backup.principal_id != principal.0
                || backup.service_instance_id != service.0
                || backup.name != name
                || backup.delete_requested)
        {
            return Err(StoreError::Conflict);
        }
        Ok(existing)
    }

    /// A restore accepted by the API must reach the provider before its source
    /// is deleted. The provider then pins snapshots until the new disk is Bound.
    pub async fn flash_backup_restore_pending(&self, backup_id: Uuid) -> Result<bool, StoreError> {
        Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM service_instances WHERE provider='flash' AND spec->>'restore_from_backup_id'=$1 AND state IN ('provisioning','updating'))")
            .bind(backup_id.to_string()).fetch_one(&self.pool).await?)
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn request_flash_backup(
        &self,
        organization: OrganizationId,
        service: ServiceInstanceId,
        principal: PrincipalId,
        request_id: Uuid,
        name: &str,
        volume_size_bytes: i64,
        source_pvc_uid: &str,
        resolved_image: Option<&str>,
        expected_generation: i64,
    ) -> Result<FlashBackup, StoreError> {
        if volume_size_bytes <= 0 || source_pvc_uid.is_empty() {
            return Err(StoreError::RequestRejected(
                "Persistent disk is not ready.".into(),
            ));
        }
        let mut tx = self.pool.begin().await?;
        lock_tenant_allocations(&mut tx, organization).await?;
        if let Some(existing) = sqlx::query_as::<_, FlashBackup>(
            "SELECT * FROM flash_backups WHERE organization_id=$1 AND request_id=$2",
        )
        .bind(organization.0)
        .bind(request_id)
        .fetch_optional(&mut *tx)
        .await?
        {
            if existing.service_instance_id != service.0
                || existing.name != name
                || existing.principal_id != principal.0
                || existing.delete_requested
            {
                return Err(StoreError::Conflict);
            }
            tx.commit().await?;
            return Ok(existing);
        }
        let source = sqlx::query_as::<_, (Uuid, String, i64, Value)>(
            "SELECT project_id,name,generation,spec FROM service_instances WHERE id=$1 AND organization_id=$2 AND provider='flash' AND state <> 'deleting' FOR SHARE",
        ).bind(service.0).bind(organization.0).fetch_optional(&mut *tx).await?.ok_or(StoreError::NotFound)?;
        if source.2 != expected_generation {
            return Err(StoreError::Conflict);
        }
        let (count, bytes, service_count): (i64, i64, i64) = sqlx::query_as(
            "SELECT count(*),coalesce(sum(volume_size_bytes),0)::bigint,count(*) FILTER (WHERE service_instance_id=$2) FROM flash_backups WHERE organization_id=$1 AND phase <> 'deleted'",
        ).bind(organization.0).bind(service.0).fetch_one(&mut *tx).await?;
        // Failed/deleting requests reserve their space until explicitly removed.
        if count >= 100
            || service_count >= 10
            || bytes
                .checked_add(volume_size_bytes)
                .is_none_or(|n| n > 128 * 1024_i64.pow(3))
        {
            return Err(StoreError::RequestRejected("Backup quota exceeded (10 per service, 100 and 128 GiB of source disks per organization).".into()));
        }
        let mut spec = source.3;
        for key in ["env", "secret_env", "secret_files"] {
            spec[key] = serde_json::json!({});
        }
        spec["task_role"] = Value::Null;
        spec["network"] = Value::Null;
        spec["exposure"]["authentication"] = Value::Null;
        if let Some(image) = resolved_image {
            spec["image"] = Value::String(image.to_owned());
        }
        let backup = sqlx::query_as::<_, FlashBackup>(
            "INSERT INTO flash_backups(id,organization_id,project_id,service_instance_id,principal_id,request_id,name,source_name,source_generation,source_pvc_uid,source_spec,volume_size_bytes) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12) RETURNING *",
        ).bind(Uuid::now_v7()).bind(organization.0).bind(source.0).bind(service.0)
            .bind(principal.0).bind(request_id).bind(name).bind(source.1).bind(source.2)
            .bind(source_pvc_uid).bind(spec).bind(volume_size_bytes).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        Ok(backup)
    }

    pub async fn list_flash_backups(
        &self,
        organization: OrganizationId,
        service: Option<ServiceInstanceId>,
    ) -> Result<Vec<FlashBackup>, StoreError> {
        Ok(sqlx::query_as("SELECT * FROM flash_backups WHERE organization_id=$1 AND phase <> 'deleted' AND ($2::uuid IS NULL OR service_instance_id=$2) ORDER BY created_at DESC LIMIT 100")
            .bind(organization.0).bind(service.map(|id| id.0)).fetch_all(&self.pool).await?)
    }

    pub async fn flash_backup(
        &self,
        organization: OrganizationId,
        id: Uuid,
    ) -> Result<FlashBackup, StoreError> {
        sqlx::query_as(
            "SELECT * FROM flash_backups WHERE organization_id=$1 AND id=$2 AND phase <> 'deleted'",
        )
        .bind(organization.0)
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(StoreError::NotFound)
    }

    pub async fn request_flash_backup_delete(
        &self,
        organization: OrganizationId,
        id: Uuid,
    ) -> Result<FlashBackup, StoreError> {
        let mut tx = self.pool.begin().await?;
        lock_tenant_allocations(&mut tx, organization).await?;
        let backup=sqlx::query_as("UPDATE flash_backups SET delete_requested=true,phase='deleting',updated_at=now() WHERE organization_id=$1 AND id=$2 AND phase <> 'deleted' RETURNING *")
            .bind(organization.0).bind(id).fetch_optional(&mut *tx).await?.ok_or(StoreError::NotFound)?;
        tx.commit().await?;
        Ok(backup)
    }

    pub async fn pending_flash_backups(&self) -> Result<Vec<FlashBackup>, StoreError> {
        // Refresh Ready records too, so removal/failure in the data plane is visible.
        Ok(sqlx::query_as("SELECT * FROM flash_backups WHERE deleted_at IS NULL OR deleted_at > now()-interval '1 day' ORDER BY updated_at LIMIT 32").fetch_all(&self.pool).await?)
    }

    pub async fn update_flash_backup_status(
        &self,
        id: Uuid,
        phase: &str,
        status: Value,
    ) -> Result<(), StoreError> {
        sqlx::query("UPDATE flash_backups SET phase=CASE WHEN deleted_at IS NOT NULL THEN 'deleted' WHEN delete_requested THEN 'deleting' ELSE $2 END,status=$3,updated_at=now() WHERE id=$1")
            .bind(id).bind(phase).bind(status).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn release_flash_backup(&self, id: Uuid) -> Result<(), StoreError> {
        // Retain a tombstone longer than provider JWT/request lifetimes. A stale
        // reconciler cannot permanently resurrect data after quota is released.
        sqlx::query("UPDATE flash_backups SET phase='deleted',status='{}',deleted_at=coalesce(deleted_at,now()),updated_at=now() WHERE id=$1 AND delete_requested").bind(id).execute(&self.pool).await?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn reserve_flash_restore(
        &self,
        organization: OrganizationId,
        principal: PrincipalId,
        request_id: Uuid,
        backup_id: Uuid,
        project: ProjectId,
        name: &str,
    ) -> Result<ServiceInstanceId, StoreError> {
        let mut tx = self.pool.begin().await?;
        lock_tenant_allocations(&mut tx, organization).await?;
        let existing: Option<(Uuid, Uuid, String, Uuid)> = sqlx::query_as(
            "SELECT backup_id,project_id,name,service_instance_id FROM flash_backup_restore_requests WHERE organization_id=$1 AND principal_id=$2 AND request_id=$3",
        ).bind(organization.0).bind(principal.0).bind(request_id).fetch_optional(&mut *tx).await?;
        if let Some((backup, project_id, existing_name, service)) = existing {
            if backup != backup_id || project_id != project.0 || existing_name != name {
                return Err(StoreError::Conflict);
            }
            tx.commit().await?;
            return Ok(ServiceInstanceId(service));
        }
        let usable: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM flash_backups WHERE id=$1 AND organization_id=$2 AND phase='ready' AND NOT delete_requested)")
            .bind(backup_id).bind(organization.0).fetch_one(&mut *tx).await?;
        if !usable {
            return Err(StoreError::Conflict);
        }
        let id = ServiceInstanceId::new();
        sqlx::query("INSERT INTO flash_backup_restore_requests(organization_id,principal_id,request_id,backup_id,project_id,name,service_instance_id) VALUES($1,$2,$3,$4,$5,$6,$7)")
            .bind(organization.0).bind(principal.0).bind(request_id).bind(backup_id).bind(project.0).bind(name).bind(id.0).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(id)
    }
}
