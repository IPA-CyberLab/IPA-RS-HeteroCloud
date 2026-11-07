use chrono::{DateTime, Utc};
use heterocloud_domain::{OrganizationId, ServiceInstanceId, UserId};
use serde::Serialize;
use uuid::Uuid;

use crate::{Store, StoreError};

const MAX_KEYS_PER_USER: i64 = 50;
pub const MAX_BINDINGS_PER_SERVICE: usize = 50;

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct FlashSshKey {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub public_key: String,
    pub fingerprint: String,
    pub github_username: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl Store {
    pub async fn flash_ssh_keys(
        &self,
        organization: OrganizationId,
    ) -> Result<Vec<FlashSshKey>, StoreError> {
        Ok(sqlx::query_as::<_, FlashSshKey>(
            "SELECT * FROM flash_ssh_keys WHERE organization_id=$1 ORDER BY created_at, id",
        )
        .bind(organization.0)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn create_flash_ssh_key(
        &self,
        organization: OrganizationId,
        user: UserId,
        name: &str,
        public_key: &str,
        fingerprint: &str,
        github_username: Option<&str>,
    ) -> Result<FlashSshKey, StoreError> {
        let mut tx = self.pool.begin().await?;
        // Serialize this user's key limit and recheck current membership.
        sqlx::query("SELECT id FROM users WHERE id=$1 AND status='active' FOR UPDATE")
            .bind(user.0)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(StoreError::NotFound)?;
        let member: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM organization_memberships m JOIN principals p ON p.id=m.principal_id AND p.enabled WHERE m.organization_id=$1 AND m.user_id=$2)")
            .bind(organization.0).bind(user.0).fetch_one(&mut *tx).await?;
        if !member {
            return Err(StoreError::NotFound);
        }
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM flash_ssh_keys WHERE organization_id=$1 AND user_id=$2",
        )
        .bind(organization.0)
        .bind(user.0)
        .fetch_one(&mut *tx)
        .await?;
        if count >= MAX_KEYS_PER_USER {
            return Err(StoreError::RequestRejected("Too many SSH keys".into()));
        }
        let item = sqlx::query_as::<_, FlashSshKey>("INSERT INTO flash_ssh_keys(id,organization_id,user_id,name,public_key,fingerprint,github_username) VALUES($1,$2,$3,$4,$5,$6,$7) RETURNING *")
            .bind(Uuid::now_v7()).bind(organization.0).bind(user.0).bind(name).bind(public_key).bind(fingerprint).bind(github_username)
            .fetch_one(&mut *tx).await.map_err(|error| {
                if error.as_database_error().is_some_and(|db| db.is_unique_violation()) {
                    StoreError::AlreadyExists
                } else {
                    StoreError::from(error)
                }
            })?;
        tx.commit().await?;
        Ok(item)
    }

    pub async fn delete_flash_ssh_key(
        &self,
        organization: OrganizationId,
        user: UserId,
        key: Uuid,
    ) -> Result<(), StoreError> {
        let result = sqlx::query(
            "DELETE FROM flash_ssh_keys WHERE organization_id=$1 AND user_id=$2 AND id=$3",
        )
        .bind(organization.0)
        .bind(user.0)
        .bind(key)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    pub async fn flash_service_ssh_key_ids(
        &self,
        organization: OrganizationId,
        service: ServiceInstanceId,
    ) -> Result<Vec<Uuid>, StoreError> {
        Ok(sqlx::query_scalar("SELECT key_id FROM flash_ssh_key_bindings WHERE organization_id=$1 AND service_instance_id=$2 ORDER BY key_id")
            .bind(organization.0).bind(service.0).fetch_all(&self.pool).await?)
    }

    pub async fn set_flash_service_ssh_key(
        &self,
        organization: OrganizationId,
        service: ServiceInstanceId,
        key: Uuid,
        enabled: bool,
    ) -> Result<Vec<Uuid>, StoreError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT id FROM service_instances WHERE id=$1 AND organization_id=$2 AND provider='flash' AND state<>'deleting' FOR UPDATE")
            .bind(service.0).bind(organization.0).fetch_optional(&mut *tx).await?.ok_or(StoreError::NotFound)?;
        if enabled {
            sqlx::query(
                "SELECT id FROM flash_ssh_keys WHERE organization_id=$1 AND id=$2 FOR SHARE",
            )
            .bind(organization.0)
            .bind(key)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(StoreError::NotFound)?;
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM flash_ssh_key_bindings WHERE organization_id=$1 AND service_instance_id=$2")
                .bind(organization.0).bind(service.0).fetch_one(&mut *tx).await?;
            let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM flash_ssh_key_bindings WHERE organization_id=$1 AND service_instance_id=$2 AND key_id=$3)")
                .bind(organization.0).bind(service.0).bind(key).fetch_one(&mut *tx).await?;
            if !exists && count >= MAX_BINDINGS_PER_SERVICE as i64 {
                return Err(StoreError::RequestRejected(
                    "Too many SSH keys on this service".into(),
                ));
            }
            sqlx::query("INSERT INTO flash_ssh_key_bindings(organization_id,service_instance_id,key_id) VALUES($1,$2,$3) ON CONFLICT DO NOTHING")
                .bind(organization.0).bind(service.0).bind(key).execute(&mut *tx).await?;
        } else {
            sqlx::query("DELETE FROM flash_ssh_key_bindings WHERE organization_id=$1 AND service_instance_id=$2 AND key_id=$3")
                .bind(organization.0).bind(service.0).bind(key).execute(&mut *tx).await?;
        }
        let keys = sqlx::query_scalar("SELECT key_id FROM flash_ssh_key_bindings WHERE organization_id=$1 AND service_instance_id=$2 ORDER BY key_id")
            .bind(organization.0).bind(service.0).fetch_all(&mut *tx).await?;
        tx.commit().await?;
        Ok(keys)
    }

    pub async fn authorized_flash_ssh_key(
        &self,
        organization: OrganizationId,
        service: ServiceInstanceId,
        fingerprint: &str,
    ) -> Result<Option<FlashSshKey>, StoreError> {
        Ok(sqlx::query_as::<_, FlashSshKey>(
            "SELECT k.* FROM flash_ssh_keys k
             JOIN flash_ssh_key_bindings b ON b.organization_id=k.organization_id AND b.key_id=k.id
             JOIN users u ON u.id=k.user_id AND u.status='active'
             JOIN organization_memberships m ON m.organization_id=k.organization_id AND m.user_id=k.user_id
             JOIN principals p ON p.id=m.principal_id AND p.organization_id=k.organization_id AND p.enabled
             JOIN service_instances s ON s.id=b.service_instance_id AND s.organization_id=b.organization_id AND s.provider='flash' AND s.state<>'deleting'
             WHERE k.organization_id=$1 AND b.service_instance_id=$2 AND k.fingerprint=$3"
        ).bind(organization.0).bind(service.0).bind(fingerprint).fetch_optional(&self.pool).await?)
    }
}
