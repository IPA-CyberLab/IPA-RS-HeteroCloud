use super::{Store, StoreError};
use chrono::{DateTime, Utc};
use heterocloud_domain::{OrganizationId, PrincipalId};
use uuid::Uuid;

#[derive(sqlx::FromRow)]
pub struct WorkloadTokenPrincipal {
    pub token_id: Uuid,
    pub organization_id: Uuid,
    pub principal_id: Uuid,
    pub service_instance_id: Uuid,
    pub pod_uid: Uuid,
}

impl Store {
    pub async fn list_iam_bindings(
        &self,
        org: OrganizationId,
    ) -> Result<Vec<serde_json::Value>, StoreError> {
        Ok(sqlx::query_scalar("SELECT jsonb_build_object('id',b.id,'principal_id',b.principal_id,'policy_id',b.policy_id,'created_at',b.created_at) FROM iam_bindings b WHERE b.organization_id=$1 ORDER BY b.created_at,b.id")
            .bind(org.0).fetch_all(&self.pool).await?)
    }
    pub async fn revoke_iam_api_key(
        &self,
        org: OrganizationId,
        principal: PrincipalId,
        id: Uuid,
    ) -> Result<(), StoreError> {
        let result=sqlx::query("UPDATE api_keys SET revoked_at=COALESCE(revoked_at,now()) WHERE id=$1 AND principal_id=$2 AND organization_id=$3")
            .bind(id).bind(principal.0).bind(org.0).execute(&self.pool).await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    pub async fn enabled_task_role(
        &self,
        org: OrganizationId,
        role: PrincipalId,
    ) -> Result<bool, StoreError> {
        Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM principals WHERE id=$1 AND organization_id=$2 AND kind='service_account' AND enabled=true)")
            .bind(role.0).bind(org.0).fetch_one(&self.pool).await?)
    }

    pub async fn mint_workload_token(
        &self,
        service_id: Uuid,
        role: Uuid,
        pod_uid: Uuid,
        digest: &[u8; 32],
        expires_at: DateTime<Utc>,
    ) -> Result<WorkloadTokenPrincipal, StoreError> {
        let mut tx = self.pool.begin().await?;
        let org: Option<Uuid> = sqlx::query_scalar("SELECT s.organization_id FROM service_instances s JOIN principals p ON p.id=$2 AND p.organization_id=s.organization_id AND p.kind='service_account' AND p.enabled=true WHERE s.id=$1 AND s.provider='flash' AND s.state <> 'deleting' AND s.spec->>'task_role'=$2::uuid::text AND COALESCE(s.spec->>'stopped','false')='false' FOR UPDATE OF s,p")
            .bind(service_id).bind(role).fetch_optional(&mut *tx).await?;
        let org = org.ok_or(StoreError::NotFound)?;
        sqlx::query("DELETE FROM workload_access_tokens WHERE expires_at <= now()")
            .execute(&mut *tx)
            .await?;
        let count:i64=sqlx::query_scalar("SELECT count(*) FROM workload_access_tokens WHERE service_instance_id=$1 AND pod_uid=$2")
            .bind(service_id).bind(pod_uid).fetch_one(&mut *tx).await?;
        if count >= 4096 {
            return Err(StoreError::RequestRejected(
                "Too many active workload sessions for this Pod".into(),
            ));
        }
        let token_id = Uuid::now_v7();
        sqlx::query("INSERT INTO workload_access_tokens (id,organization_id,service_instance_id,principal_id,pod_uid,token_hash,expires_at) VALUES ($1,$2,$3,$4,$5,$6,$7)")
            .bind(token_id).bind(org).bind(service_id).bind(role).bind(pod_uid).bind(digest.as_slice()).bind(expires_at).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(WorkloadTokenPrincipal {
            token_id,
            organization_id: org,
            principal_id: role,
            service_instance_id: service_id,
            pod_uid,
        })
    }

    /// Current attachment and enabled principal are checked on every request.
    pub async fn authenticate_workload_token(
        &self,
        digest: &[u8; 32],
    ) -> Result<Option<WorkloadTokenPrincipal>, StoreError> {
        Ok(sqlx::query_as("SELECT t.id AS token_id,t.organization_id,t.principal_id,t.service_instance_id,t.pod_uid FROM workload_access_tokens t JOIN service_instances s ON s.id=t.service_instance_id AND s.organization_id=t.organization_id JOIN principals p ON p.id=t.principal_id AND p.organization_id=t.organization_id AND p.kind='service_account' AND p.enabled=true WHERE t.token_hash=$1 AND t.expires_at>now() AND s.provider='flash' AND s.state <> 'deleting' AND s.spec->>'task_role'=t.principal_id::text AND COALESCE(s.spec->>'stopped','false')='false'")
            .bind(digest.as_slice()).fetch_optional(&self.pool).await?)
    }

    pub async fn set_service_account_enabled(
        &self,
        org: OrganizationId,
        role: PrincipalId,
        enabled: bool,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        let result=sqlx::query("UPDATE principals SET enabled=$3 WHERE id=$1 AND organization_id=$2 AND kind='service_account'")
            .bind(role.0).bind(org.0).bind(enabled).execute(&mut *tx).await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::NotFound);
        }
        if !enabled {
            sqlx::query(
                "DELETE FROM workload_access_tokens WHERE organization_id=$1 AND principal_id=$2",
            )
            .bind(org.0)
            .bind(role.0)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }
    pub async fn delete_binding(&self, org: OrganizationId, id: Uuid) -> Result<(), StoreError> {
        let result = sqlx::query("DELETE FROM iam_bindings WHERE id=$1 AND organization_id=$2")
            .bind(id)
            .bind(org.0)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }
}
