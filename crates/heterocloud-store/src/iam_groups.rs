use chrono::{DateTime, Utc};
use heterocloud_domain::{OrganizationId, PolicyDocument, PrincipalId};
use serde::Serialize;
use uuid::Uuid;

use crate::{Store, StoreError};

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct IamGroup {
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub members: Vec<Uuid>,
    pub policies: Vec<Uuid>,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct IamPermissionsBoundary {
    pub principal_id: Uuid,
    pub policy_id: Uuid,
}

impl Store {
    pub async fn permissions_boundary(
        &self,
        account: OrganizationId,
        principal: PrincipalId,
    ) -> Result<Option<PolicyDocument>, StoreError> {
        let document = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT policy.document FROM principals principal
             JOIN iam_policies policy ON policy.organization_id = principal.organization_id AND policy.id = principal.permissions_boundary_policy_id
             WHERE principal.organization_id = $1 AND principal.id = $2")
            .bind(account.0).bind(principal.0).fetch_optional(&self.pool).await?;
        document
            .map(serde_json::from_value)
            .transpose()
            .map_err(StoreError::from)
    }

    pub async fn list_permissions_boundaries(
        &self,
        account: OrganizationId,
    ) -> Result<Vec<IamPermissionsBoundary>, StoreError> {
        Ok(sqlx::query_as::<_, IamPermissionsBoundary>("SELECT id AS principal_id, permissions_boundary_policy_id AS policy_id FROM principals WHERE organization_id = $1 AND permissions_boundary_policy_id IS NOT NULL ORDER BY id")
            .bind(account.0).fetch_all(&self.pool).await?)
    }

    pub async fn set_permissions_boundary(
        &self,
        account: OrganizationId,
        principal: PrincipalId,
        policy: Option<Uuid>,
    ) -> Result<(), StoreError> {
        let result = sqlx::query(
            "UPDATE principals SET permissions_boundary_policy_id = $3, updated_at = now()
            WHERE organization_id = $1 AND id = $2 AND ($3::uuid IS NULL OR EXISTS
                (SELECT 1 FROM iam_policies WHERE organization_id = $1 AND id = $3))",
        )
        .bind(account.0)
        .bind(principal.0)
        .bind(policy)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    pub async fn list_iam_groups(
        &self,
        account: OrganizationId,
    ) -> Result<Vec<IamGroup>, StoreError> {
        Ok(sqlx::query_as::<_, IamGroup>(
            "SELECT g.id, g.name, g.created_at,
                ARRAY(SELECT m.principal_id FROM iam_group_members m WHERE m.group_id = g.id ORDER BY m.principal_id) AS members,
                ARRAY(SELECT a.policy_id FROM iam_group_policy_attachments a WHERE a.group_id = g.id ORDER BY a.policy_id) AS policies
             FROM iam_groups g WHERE g.organization_id = $1 ORDER BY g.name, g.id",
        ).bind(account.0).fetch_all(&self.pool).await?)
    }

    pub async fn create_iam_group(
        &self,
        account: OrganizationId,
        name: &str,
    ) -> Result<Uuid, StoreError> {
        let id = Uuid::now_v7();
        sqlx::query("INSERT INTO iam_groups (id, organization_id, name) VALUES ($1, $2, $3)")
            .bind(id)
            .bind(account.0)
            .bind(name)
            .execute(&self.pool)
            .await?;
        Ok(id)
    }

    pub async fn delete_iam_group(
        &self,
        account: OrganizationId,
        group: Uuid,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        let exists = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM iam_groups WHERE organization_id = $1 AND id = $2 FOR UPDATE",
        )
        .bind(account.0)
        .bind(group)
        .fetch_optional(&mut *tx)
        .await?;
        if exists.is_none() {
            return Err(StoreError::NotFound);
        }
        let attached = sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM iam_group_members WHERE group_id = $1) OR EXISTS(SELECT 1 FROM iam_group_policy_attachments WHERE group_id = $1)")
            .bind(group).fetch_one(&mut *tx).await?;
        if attached {
            return Err(StoreError::RequestRejected(
                "Remove group users and policies before deleting the group.".into(),
            ));
        }
        sqlx::query("DELETE FROM iam_groups WHERE organization_id = $1 AND id = $2")
            .bind(account.0)
            .bind(group)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn change_iam_group_member(
        &self,
        account: OrganizationId,
        group: Uuid,
        principal: Uuid,
        add: bool,
    ) -> Result<(), StoreError> {
        let result = if add {
            sqlx::query("INSERT INTO iam_group_members (organization_id, group_id, principal_id)
                SELECT g.organization_id, g.id, p.id FROM iam_groups g JOIN principals p ON p.organization_id = g.organization_id
                WHERE g.organization_id = $1 AND g.id = $2 AND p.id = $3 AND p.kind = 'user' AND p.enabled
                ON CONFLICT DO NOTHING")
                .bind(account.0).bind(group).bind(principal).execute(&self.pool).await?
        } else {
            sqlx::query("DELETE FROM iam_group_members WHERE organization_id = $1 AND group_id = $2 AND principal_id = $3")
                .bind(account.0).bind(group).bind(principal).execute(&self.pool).await?
        };
        if result.rows_affected() != 1 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    pub async fn change_iam_group_policy(
        &self,
        account: OrganizationId,
        group: Uuid,
        policy: Uuid,
        attach: bool,
    ) -> Result<(), StoreError> {
        let result = if attach {
            sqlx::query("INSERT INTO iam_group_policy_attachments (organization_id, group_id, policy_id)
                SELECT g.organization_id, g.id, p.id FROM iam_groups g JOIN iam_policies p ON p.organization_id = g.organization_id
                WHERE g.organization_id = $1 AND g.id = $2 AND p.id = $3 ON CONFLICT DO NOTHING")
                .bind(account.0).bind(group).bind(policy).execute(&self.pool).await?
        } else {
            sqlx::query("DELETE FROM iam_group_policy_attachments WHERE organization_id = $1 AND group_id = $2 AND policy_id = $3")
                .bind(account.0).bind(group).bind(policy).execute(&self.pool).await?
        };
        if result.rows_affected() != 1 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }
}
