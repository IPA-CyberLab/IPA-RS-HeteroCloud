use chrono::{DateTime, Utc};
use heterocloud_domain::{OrganizationId, PrincipalId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Store, StoreError, lock_tenant_allocations};

/// Public metadata only. S3 credentials must never enter this record.
#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct FlashBackupDestination {
    pub id: Uuid,
    pub organization_id: Uuid,
    #[serde(skip_serializing)]
    pub principal_id: Uuid,
    #[serde(skip_serializing)]
    pub request_id: Uuid,
    pub name: String,
    pub endpoint: String,
    pub bucket: String,
    pub region: String,
    pub prefix: String,
    pub virtual_hosted: bool,
    pub state: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewFlashBackupDestination {
    pub name: String,
    pub request_id: Uuid,
    pub endpoint: String,
    pub bucket: String,
    pub region: String,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub virtual_hosted: bool,
    pub access_key: String,
    pub secret_key: String,
}

impl Store {
    pub async fn reserve_flash_backup_destination(
        &self,
        org: OrganizationId,
        principal: PrincipalId,
        input: &NewFlashBackupDestination,
    ) -> Result<FlashBackupDestination, StoreError> {
        let mut tx = self.pool.begin().await?;
        lock_tenant_allocations(&mut tx, org).await?;
        if let Some(existing) = sqlx::query_as::<_, FlashBackupDestination>(
            "SELECT * FROM flash_backup_destinations WHERE organization_id=$1 AND request_id=$2",
        )
        .bind(org.0)
        .bind(input.request_id)
        .fetch_optional(&mut *tx)
        .await?
        {
            if existing.principal_id != principal.0
                || existing.name != input.name
                || existing.endpoint != input.endpoint
                || existing.bucket != input.bucket
                || existing.region != input.region
                || existing.prefix != input.prefix
                || existing.virtual_hosted != input.virtual_hosted
            {
                return Err(StoreError::Conflict);
            }
            tx.commit().await?;
            return Ok(existing);
        }
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM flash_backup_destinations WHERE organization_id=$1",
        )
        .bind(org.0)
        .fetch_one(&mut *tx)
        .await?;
        if count >= 20 {
            return Err(StoreError::RequestRejected(
                "At most 20 backup destinations per organization.".into(),
            ));
        }
        let duplicate: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM flash_backup_destinations WHERE organization_id=$1 AND name=$2)")
            .bind(org.0).bind(&input.name).fetch_one(&mut *tx).await?;
        if duplicate {
            return Err(StoreError::Conflict);
        }
        let row = sqlx::query_as("INSERT INTO flash_backup_destinations(id,organization_id,principal_id,request_id,name,endpoint,bucket,region,prefix,virtual_hosted) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) RETURNING *")
            .bind(Uuid::now_v7()).bind(org.0).bind(principal.0).bind(input.request_id)
            .bind(&input.name).bind(&input.endpoint).bind(&input.bucket).bind(&input.region)
            .bind(&input.prefix).bind(input.virtual_hosted).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        Ok(row)
    }

    pub async fn flash_backup_destinations(
        &self,
        org: OrganizationId,
    ) -> Result<Vec<FlashBackupDestination>, StoreError> {
        Ok(sqlx::query_as(
            "SELECT * FROM flash_backup_destinations WHERE organization_id=$1 ORDER BY created_at",
        )
        .bind(org.0)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn flash_backup_destinations_to_refresh(
        &self,
    ) -> Result<Vec<FlashBackupDestination>, StoreError> {
        Ok(
            sqlx::query_as("SELECT * FROM flash_backup_destinations ORDER BY updated_at LIMIT 32")
                .fetch_all(&self.pool)
                .await?,
        )
    }

    pub async fn refresh_flash_backup_destination(
        &self,
        org: OrganizationId,
        id: Uuid,
        observed_at: DateTime<Utc>,
        ready: Option<bool>,
    ) -> Result<(), StoreError> {
        sqlx::query("UPDATE flash_backup_destinations SET state=coalesce($3,state),updated_at=now() WHERE organization_id=$1 AND id=$2 AND updated_at=$4")
            .bind(org.0).bind(id).bind(ready.map(|value| if value { "ready" } else { "pending" })).bind(observed_at).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn flash_backup_destination(
        &self,
        org: OrganizationId,
        id: Uuid,
    ) -> Result<FlashBackupDestination, StoreError> {
        sqlx::query_as("SELECT * FROM flash_backup_destinations WHERE organization_id=$1 AND id=$2")
            .bind(org.0)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or(StoreError::NotFound)
    }

    pub async fn update_flash_backup_destination_state(
        &self,
        org: OrganizationId,
        id: Uuid,
        ready: bool,
    ) -> Result<FlashBackupDestination, StoreError> {
        sqlx::query_as("UPDATE flash_backup_destinations SET state=$3,updated_at=now() WHERE organization_id=$1 AND id=$2 RETURNING *")
            .bind(org.0).bind(id).bind(if ready { "ready" } else { "pending" })
            .fetch_optional(&self.pool).await?.ok_or(StoreError::NotFound)
    }
}
