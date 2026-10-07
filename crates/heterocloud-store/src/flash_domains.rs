use crate::{Store, StoreError};
use chrono::{DateTime, Utc};
use heterocloud_domain::{OrganizationId, PrincipalId, ServiceInstanceId};
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct FlashDomainBinding {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub service_instance_id: Uuid,
    pub principal_id: Uuid,
    pub hostname: String,
    pub verification_value: String,
    pub delete_requested: bool,
    pub reconcile_pending: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Store {
    pub async fn reserve_flash_domain(
        &self,
        organization: OrganizationId,
        service: ServiceInstanceId,
        principal: PrincipalId,
        hostname: &str,
    ) -> Result<FlashDomainBinding, StoreError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext('heterocloud.flash.custom-domains'))")
            .execute(&mut *tx)
            .await?;
        let row = sqlx::query_as::<_, (String, String, serde_json::Value)>("SELECT provider,state,spec FROM service_instances WHERE id=$1 AND organization_id=$2 FOR UPDATE")
            .bind(service.0).bind(organization.0).fetch_optional(&mut *tx).await?.ok_or(StoreError::NotFound)?;
        if row.0 != "flash"
            || row.1 == "deleting"
            || row.2["exposure"]["endpoint_mode"] != "web"
            || row.2["exposure"]["type"] != "public"
            || row.2["exposure"]["traffic_mode"] != "forwarded"
        {
            return Err(StoreError::RequestRejected(
                "Custom domains require a public HTTP/HTTPS Flash service.".into(),
            ));
        }
        if let Some(existing) = sqlx::query_as::<_, FlashDomainBinding>(
            "SELECT * FROM flash_domain_bindings WHERE hostname=$1",
        )
        .bind(hostname)
        .fetch_optional(&mut *tx)
        .await?
        {
            if existing.organization_id == organization.0
                && existing.service_instance_id == service.0
                && !existing.delete_requested
            {
                tx.commit().await?;
                return Ok(existing);
            }
            return Err(StoreError::AlreadyExists);
        }
        let total: (i64,) = sqlx::query_as("SELECT count(*) FROM flash_domain_bindings")
            .fetch_one(&mut *tx)
            .await?;
        let owned: (i64,) = sqlx::query_as(
            "SELECT count(*) FROM flash_domain_bindings WHERE service_instance_id=$1",
        )
        .bind(service.0)
        .fetch_one(&mut *tx)
        .await?;
        if total.0 >= 128 || owned.0 >= 8 {
            return Err(StoreError::RequestRejected(
                "Custom domain capacity reached (8 per service).".into(),
            ));
        }
        let id = Uuid::now_v7();
        let verification = format!(
            "heterocloud-domain={}:{}",
            id.simple(),
            Uuid::now_v7().simple()
        );
        let row = sqlx::query_as::<_, FlashDomainBinding>("INSERT INTO flash_domain_bindings(id,organization_id,service_instance_id,principal_id,hostname,verification_value) VALUES($1,$2,$3,$4,$5,$6) RETURNING *")
            .bind(id).bind(organization.0).bind(service.0).bind(principal.0).bind(hostname).bind(verification).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        Ok(row)
    }
    pub async fn flash_domain_bindings(
        &self,
        organization: OrganizationId,
        service: ServiceInstanceId,
    ) -> Result<Vec<FlashDomainBinding>, StoreError> {
        Ok(sqlx::query_as("SELECT * FROM flash_domain_bindings WHERE organization_id=$1 AND service_instance_id=$2 ORDER BY created_at").bind(organization.0).bind(service.0).fetch_all(&self.pool).await?)
    }
    pub async fn request_flash_domain_delete(
        &self,
        organization: OrganizationId,
        service: ServiceInstanceId,
        id: Uuid,
    ) -> Result<(), StoreError> {
        let r=sqlx::query("UPDATE flash_domain_bindings SET delete_requested=true,reconcile_pending=true,updated_at=now() WHERE id=$1 AND organization_id=$2 AND service_instance_id=$3")
            .bind(id).bind(organization.0).bind(service.0).execute(&self.pool).await?;
        if r.rows_affected() == 0 {
            return Err(StoreError::NotFound);
        };
        Ok(())
    }
    pub async fn pending_flash_domains(&self) -> Result<Vec<FlashDomainBinding>, StoreError> {
        // Periodically re-apply every binding to recover an accidentally removed
        // provider resource. Rotating attempted rows also prevents a failing
        // provider from starving domains beyond the first batch.
        Ok(
            sqlx::query_as("SELECT * FROM flash_domain_bindings ORDER BY updated_at LIMIT 64")
                .fetch_all(&self.pool)
                .await?,
        )
    }
    pub async fn mark_flash_domain_attempted(&self, id: Uuid) -> Result<(), StoreError> {
        sqlx::query("UPDATE flash_domain_bindings SET updated_at=now() WHERE id=$1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    pub async fn mark_flash_domain_synced(&self, id: Uuid) -> Result<(), StoreError> {
        sqlx::query("UPDATE flash_domain_bindings SET reconcile_pending=false,updated_at=now() WHERE id=$1 AND NOT delete_requested").bind(id).execute(&self.pool).await?;
        Ok(())
    }
    pub async fn release_flash_domain(&self, id: Uuid) -> Result<(), StoreError> {
        sqlx::query("DELETE FROM flash_domain_bindings WHERE id=$1 AND delete_requested")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
