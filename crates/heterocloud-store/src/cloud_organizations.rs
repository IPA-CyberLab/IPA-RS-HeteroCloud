use chrono::{DateTime, Utc};
use heterocloud_domain::{Organization, OrganizationId, PolicyDocument, PrincipalId, UserId};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::{OrganizationRow, Store, StoreError};

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct CloudOrganization {
    pub id: Uuid,
    pub name: String,
    pub management_account_id: Uuid,
    pub management_account_number: String,
    pub root_id: Uuid,
    pub current_account_id: Uuid,
    pub current_account_number: String,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct CloudOrganizationalUnit {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct CloudAccount {
    pub id: Uuid,
    pub account_number: String,
    pub name: String,
    pub slug: String,
    pub parent_id: Uuid,
    pub is_management_account: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CloudOrganizationTree {
    pub organization: CloudOrganization,
    pub units: Vec<CloudOrganizationalUnit>,
    pub accounts: Vec<CloudAccount>,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct CloudServiceControlPolicy {
    pub id: Uuid,
    pub name: String,
    pub document: Value,
    pub is_managed: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct CloudScpAttachment {
    pub target_type: String,
    pub target_id: Uuid,
    pub policy_id: Uuid,
}

#[derive(sqlx::FromRow)]
struct ScpRow {
    target_id: Uuid,
    is_root: bool,
    document: Option<Value>,
}

impl Store {
    pub async fn service_control_enabled(&self) -> Result<bool, StoreError> {
        Ok(sqlx::query_scalar::<_, bool>(
            "SELECT scp_enabled FROM cloud_security_activation WHERE singleton = true",
        )
        .fetch_one(&self.pool)
        .await?)
    }

    /// Return every SCP level for member accounts. An empty vector means that
    /// the caller is the management account, which AWS SCPs do not restrict.
    pub async fn service_control_levels(
        &self,
        account_id: OrganizationId,
    ) -> Result<Vec<Vec<PolicyDocument>>, StoreError> {
        if !self.service_control_enabled().await? {
            return Ok(Vec::new());
        }
        let organization = self
            .cloud_organization_for_account(account_id)
            .await?
            .ok_or(StoreError::Invariant("account is missing its organization"))?;
        if organization.management_account_id == account_id.0 {
            return Ok(Vec::new());
        }
        let rows = sqlx::query_as::<_, ScpRow>(
            "WITH RECURSIVE ancestors AS (
                SELECT u.id, u.parent_id, u.organization_id, ARRAY[u.id] AS visited
                  FROM cloud_organization_accounts a
                  JOIN cloud_organizational_units u ON u.id = a.parent_id AND u.organization_id = a.organization_id
                  WHERE a.account_id = $1
                UNION ALL
                SELECT u.id, u.parent_id, u.organization_id, ancestors.visited || u.id
                  FROM ancestors JOIN cloud_organizational_units u
                    ON u.id = ancestors.parent_id AND u.organization_id = ancestors.organization_id
                  WHERE NOT u.id = ANY(ancestors.visited)
             )
             SELECT a.id AS target_id, a.parent_id IS NULL AS is_root, p.document
               FROM ancestors a
               LEFT JOIN cloud_scp_unit_attachments t ON t.unit_id = a.id
               LEFT JOIN cloud_service_control_policies p ON p.id = t.policy_id
             UNION ALL
             SELECT a.account_id AS target_id, false AS is_root, p.document
               FROM cloud_organization_accounts a
               LEFT JOIN cloud_scp_account_attachments t ON t.account_id = a.account_id
               LEFT JOIN cloud_service_control_policies p ON p.id = t.policy_id
              WHERE a.account_id = $1",
        )
        .bind(account_id.0)
        .fetch_all(&self.pool)
        .await?;
        if !rows.iter().any(|row| row.is_root) {
            return Err(StoreError::Invariant(
                "account organization has no reachable root",
            ));
        }
        let mut levels = std::collections::BTreeMap::<Uuid, Vec<PolicyDocument>>::new();
        for row in rows {
            let level = levels.entry(row.target_id).or_default();
            if let Some(document) = row.document {
                level.push(serde_json::from_value(document)?);
            }
        }
        Ok(levels.into_values().collect())
    }

    pub async fn list_service_control_policies(
        &self,
        management_account_id: OrganizationId,
    ) -> Result<(Vec<CloudServiceControlPolicy>, Vec<CloudScpAttachment>), StoreError> {
        let organization = self
            .cloud_organization_for_account(management_account_id)
            .await?
            .filter(|o| o.management_account_id == management_account_id.0)
            .ok_or(StoreError::NotFound)?;
        let policies = sqlx::query_as::<_, CloudServiceControlPolicy>(
            "SELECT id, name, document, is_managed, created_at FROM cloud_service_control_policies
             WHERE organization_id = $1 ORDER BY is_managed DESC, name, id",
        )
        .bind(organization.id)
        .fetch_all(&self.pool)
        .await?;
        let attachments = sqlx::query_as::<_, CloudScpAttachment>(
            "SELECT 'unit'::text AS target_type, unit_id AS target_id, policy_id FROM cloud_scp_unit_attachments WHERE organization_id = $1
             UNION ALL SELECT 'account'::text, account_id, policy_id FROM cloud_scp_account_attachments WHERE organization_id = $1")
            .bind(organization.id).fetch_all(&self.pool).await?;
        Ok((policies, attachments))
    }

    pub async fn create_service_control_policy(
        &self,
        management_account_id: OrganizationId,
        name: &str,
        document: &PolicyDocument,
    ) -> Result<CloudServiceControlPolicy, StoreError> {
        sqlx::query_as::<_, CloudServiceControlPolicy>(
            "INSERT INTO cloud_service_control_policies (id, organization_id, name, document)
             SELECT $1, o.id, $3, $4 FROM cloud_organizations o WHERE o.management_account_id = $2
             RETURNING id, name, document, is_managed, created_at",
        )
        .bind(Uuid::now_v7())
        .bind(management_account_id.0)
        .bind(name)
        .bind(serde_json::to_value(document)?)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(StoreError::NotFound)
    }

    pub async fn change_service_control_attachment(
        &self,
        management_account_id: OrganizationId,
        target_type: &str,
        target_id: Uuid,
        policy_id: Uuid,
        attach: bool,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        // Serializing attachment changes prevents concurrent detaches from
        // removing the final policy on an entity.
        let organization_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM cloud_organizations WHERE management_account_id = $1 FOR UPDATE",
        )
        .bind(management_account_id.0)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(StoreError::NotFound)?;
        let (table, target_column, target_table, target_key) = match target_type {
            "unit" => (
                "cloud_scp_unit_attachments",
                "unit_id",
                "cloud_organizational_units",
                "id",
            ),
            "account" => (
                "cloud_scp_account_attachments",
                "account_id",
                "cloud_organization_accounts",
                "account_id",
            ),
            _ => return Err(StoreError::NotFound),
        };
        // Identifiers above are fixed constants, never user input.
        let exists_query = format!(
            "SELECT EXISTS(SELECT 1 FROM {target_table} WHERE organization_id = $1 AND {target_key} = $2) AND EXISTS(SELECT 1 FROM cloud_service_control_policies WHERE organization_id = $1 AND id = $3)"
        );
        let exists = sqlx::query_scalar::<_, bool>(sqlx::AssertSqlSafe(exists_query.as_str()))
            .bind(organization_id)
            .bind(target_id)
            .bind(policy_id)
            .fetch_one(&mut *tx)
            .await?;
        if !exists {
            return Err(StoreError::NotFound);
        }
        let count_query = format!(
            "SELECT count(*) FROM {table} WHERE organization_id = $1 AND {target_column} = $2"
        );
        let count = sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(count_query.as_str()))
            .bind(organization_id)
            .bind(target_id)
            .fetch_one(&mut *tx)
            .await?;
        if attach {
            if count >= 10 {
                let attached_query = format!(
                    "SELECT EXISTS(SELECT 1 FROM {table} WHERE organization_id = $1 AND {target_column} = $2 AND policy_id = $3)"
                );
                let already_attached =
                    sqlx::query_scalar::<_, bool>(sqlx::AssertSqlSafe(attached_query.as_str()))
                        .bind(organization_id)
                        .bind(target_id)
                        .bind(policy_id)
                        .fetch_one(&mut *tx)
                        .await?;
                if !already_attached {
                    return Err(StoreError::RequestRejected(
                        "A maximum of 10 service control policies can be attached to a target."
                            .into(),
                    ));
                }
            }
            let insert = format!(
                "INSERT INTO {table} (organization_id, {target_column}, policy_id) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING"
            );
            sqlx::query(sqlx::AssertSqlSafe(insert.as_str()))
                .bind(organization_id)
                .bind(target_id)
                .bind(policy_id)
                .execute(&mut *tx)
                .await?;
        } else {
            if count <= 1 {
                return Err(StoreError::RequestRejected(
                    "At least one service control policy must remain attached.".into(),
                ));
            }
            let delete = format!(
                "DELETE FROM {table} WHERE organization_id = $1 AND {target_column} = $2 AND policy_id = $3"
            );
            let result = sqlx::query(sqlx::AssertSqlSafe(delete.as_str()))
                .bind(organization_id)
                .bind(target_id)
                .bind(policy_id)
                .execute(&mut *tx)
                .await?;
            if result.rows_affected() != 1 {
                return Err(StoreError::NotFound);
            }
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn cloud_organization_for_account(
        &self,
        account_id: OrganizationId,
    ) -> Result<Option<CloudOrganization>, StoreError> {
        Ok(sqlx::query_as::<_, CloudOrganization>(
            "SELECT o.id, o.name, o.management_account_id,
                    m.account_number::text AS management_account_number, r.id AS root_id,
                    a.account_id AS current_account_id,
                    current_account.account_number::text AS current_account_number
             FROM cloud_organization_accounts a
             JOIN cloud_organizations o ON o.id = a.organization_id
             JOIN organizations m ON m.id = o.management_account_id
             JOIN organizations current_account ON current_account.id = a.account_id
             JOIN cloud_organizational_units r ON r.organization_id = o.id AND r.parent_id IS NULL
             WHERE a.account_id = $1",
        )
        .bind(account_id.0)
        .fetch_optional(&self.pool)
        .await?)
    }

    pub async fn cloud_organization_tree(
        &self,
        management_account_id: OrganizationId,
    ) -> Result<CloudOrganizationTree, StoreError> {
        let organization = self
            .cloud_organization_for_account(management_account_id)
            .await?
            .filter(|organization| organization.management_account_id == management_account_id.0)
            .ok_or(StoreError::NotFound)?;
        let units = sqlx::query_as::<_, CloudOrganizationalUnit>(
            "SELECT id, parent_id, name, created_at FROM cloud_organizational_units
             WHERE organization_id = $1 ORDER BY created_at, id",
        )
        .bind(organization.id)
        .fetch_all(&self.pool)
        .await?;
        let accounts = sqlx::query_as::<_, CloudAccount>(
            "SELECT a.id, a.account_number::text AS account_number, a.name, a.slug,
                    membership.parent_id, (a.id = o.management_account_id) AS is_management_account,
                    a.created_at
             FROM cloud_organization_accounts membership
             JOIN organizations a ON a.id = membership.account_id
             JOIN cloud_organizations o ON o.id = membership.organization_id
             WHERE membership.organization_id = $1 ORDER BY a.name, a.id",
        )
        .bind(organization.id)
        .fetch_all(&self.pool)
        .await?;
        Ok(CloudOrganizationTree {
            organization,
            units,
            accounts,
        })
    }

    pub async fn create_cloud_organizational_unit(
        &self,
        management_account_id: OrganizationId,
        parent_id: Uuid,
        name: &str,
    ) -> Result<CloudOrganizationalUnit, StoreError> {
        let depth = sqlx::query_scalar::<_, i64>(
            "WITH RECURSIVE ancestors AS (
                SELECT u.id, u.parent_id, u.organization_id, ARRAY[u.id] AS visited FROM cloud_organizational_units u
                JOIN cloud_organizations o ON o.id = u.organization_id
                WHERE o.management_account_id = $1 AND u.id = $2
                UNION ALL
                SELECT u.id, u.parent_id, u.organization_id, a.visited || u.id FROM ancestors a
                JOIN cloud_organizational_units u ON u.id = a.parent_id AND u.organization_id = a.organization_id
                WHERE NOT u.id = ANY(a.visited)
             ) SELECT CASE WHEN COALESCE(bool_or(parent_id IS NULL), false) THEN count(*) ELSE 0 END FROM ancestors")
            .bind(management_account_id.0).bind(parent_id).fetch_one(&self.pool).await?;
        if depth == 0 {
            return Err(StoreError::NotFound);
        }
        // The root is not an OU: its depth is one in this query.
        if depth > 5 {
            return Err(StoreError::RequestRejected(
                "Organizational units can be nested at most five levels below the root.".into(),
            ));
        }
        let row = sqlx::query_as::<_, CloudOrganizationalUnit>(
            "INSERT INTO cloud_organizational_units (id, organization_id, parent_id, name)
             SELECT $1, o.id, p.id, $4 FROM cloud_organizations o
             JOIN cloud_organizational_units p ON p.organization_id = o.id
             WHERE o.management_account_id = $2 AND p.id = $3
             RETURNING id, parent_id, name, created_at",
        )
        .bind(Uuid::now_v7())
        .bind(management_account_id.0)
        .bind(parent_id)
        .bind(name)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(StoreError::NotFound)?;
        Ok(row)
    }

    /// Creates a member account and its initial administrator atomically. The
    /// compatibility trigger's temporary organization never becomes visible.
    pub async fn create_cloud_member_account(
        &self,
        management_account_id: OrganizationId,
        parent_id: Uuid,
        user_id: UserId,
        slug: &str,
        name: &str,
    ) -> Result<Organization, StoreError> {
        let mut tx = self.pool.begin().await?;
        let cloud_organization_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT o.id FROM cloud_organizations o
             JOIN cloud_organizational_units u ON u.organization_id = o.id
             WHERE o.management_account_id = $1 AND u.id = $2 FOR UPDATE OF o",
        )
        .bind(management_account_id.0)
        .bind(parent_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(StoreError::NotFound)?;
        let display_name = sqlx::query_scalar::<_, String>(
            "SELECT display_name FROM users WHERE id = $1 AND status = 'active' FOR SHARE",
        )
        .bind(user_id.0)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(StoreError::NotFound)?;
        let account_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let row = sqlx::query_as::<_, OrganizationRow>(
            "INSERT INTO organizations (id, slug, name) VALUES ($1, $2, $3)
             RETURNING id, slug, name, created_at",
        )
        .bind(account_id.0)
        .bind(slug)
        .bind(name)
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM cloud_organizations WHERE management_account_id = $1")
            .bind(account_id.0)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO cloud_organization_accounts (account_id, organization_id, parent_id)
             VALUES ($1, $2, $3)",
        )
        .bind(account_id.0)
        .bind(cloud_organization_id)
        .bind(parent_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO principals (id, organization_id, kind, name, user_id, quota_user_id, enabled)
             VALUES ($1, $2, 'user', $3, $4, $4, true)",
        )
        .bind(principal_id.0)
        .bind(account_id.0)
        .bind(display_name)
        .bind(user_id.0)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO organization_memberships (organization_id, user_id, principal_id, role)
             VALUES ($1, $2, $3, 'owner')",
        )
        .bind(account_id.0)
        .bind(user_id.0)
        .bind(principal_id.0)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Organization::try_from(row)
    }

    pub async fn move_cloud_member_account(
        &self,
        management_account_id: OrganizationId,
        account_id: OrganizationId,
        parent_id: Uuid,
    ) -> Result<(), StoreError> {
        let result = sqlx::query(
            "UPDATE cloud_organization_accounts a SET parent_id = u.id
             FROM cloud_organizations o, cloud_organizational_units u
             WHERE o.management_account_id = $1 AND a.account_id = $2
               AND a.account_id <> o.management_account_id AND a.organization_id = o.id
               AND u.id = $3 AND u.organization_id = o.id",
        )
        .bind(management_account_id.0)
        .bind(account_id.0)
        .bind(parent_id)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }
}
