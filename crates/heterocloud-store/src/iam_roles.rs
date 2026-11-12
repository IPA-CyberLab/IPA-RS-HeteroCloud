use chrono::{DateTime, Utc};
use heterocloud_domain::{OrganizationId, PolicyDocument, PrincipalId, UserId};
use serde::{Serialize, Serializer};
use serde_json::Value;
use uuid::Uuid;

use crate::{Store, StoreError};

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct IamRole {
    pub principal_id: Uuid,
    pub organization_id: Uuid,
    pub name: String,
    pub arn: String,
    #[serde(serialize_with = "serialize_trust_policy")]
    pub trust_policy: Value,
    pub max_session_seconds: i32,
    pub managed_by_identity_center: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct IamRoleSession {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub principal_id: Uuid,
    pub source_organization_id: Uuid,
    pub source_principal_id: Uuid,
    pub source_arn: String,
    pub session_name: String,
    pub expires_at: DateTime<Utc>,
    pub session_policy: Option<PolicyDocument>,
    pub identity_center_user_id: Option<Uuid>,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct IamRoleSessionInfo {
    pub id: Uuid,
    #[serde(serialize_with = "serialize_principal_identifier")]
    pub source_arn: String,
    pub session_name: String,
    pub expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct BrowserRoleSession {
    pub session_id: Uuid,
    pub organization_id: Uuid,
    pub account_number: String,
    pub account_name: String,
    pub principal_id: Uuid,
    pub role_arn: String,
    pub session_name: String,
    pub expires_at: DateTime<Utc>,
    pub active: bool,
}

#[derive(sqlx::FromRow)]
struct RoleSessionRow {
    id: Uuid,
    organization_id: Uuid,
    principal_id: Uuid,
    source_organization_id: Uuid,
    source_principal_id: Uuid,
    source_arn: String,
    session_name: String,
    expires_at: DateTime<Utc>,
    session_policy: Option<Value>,
    identity_center_user_id: Option<Uuid>,
}

pub struct NewIamRoleSession<'a> {
    pub organization_id: OrganizationId,
    pub role_principal_id: PrincipalId,
    pub source_organization_id: OrganizationId,
    pub source_principal_id: PrincipalId,
    pub source_arn: &'a str,
    pub session_name: &'a str,
    pub session_policy: Option<&'a PolicyDocument>,
    pub token_hash: &'a [u8; 32],
    pub expires_at: DateTime<Utc>,
    pub duration_seconds: i32,
    pub expected_trust_policy: &'a Value,
}

const ROLE_SELECT: &str = "SELECT r.principal_id, r.organization_id, r.name,
    ('arn:heterocloud:iam::' || a.account_number::text || ':role/' || r.name) AS arn,
    r.trust_policy, r.max_session_seconds, r.created_at,
    EXISTS(SELECT 1 FROM identity_center_account_roles ic WHERE ic.account_id=r.organization_id AND ic.role_principal_id=r.principal_id) AS managed_by_identity_center
    FROM iam_roles r JOIN organizations a ON a.id = r.organization_id";

// Preserve the stored policy (including the value checked in the role-session
// transaction), while presenting older platform-generated identifiers under
// the platform's own namespace. Unsupported or ambiguous documents are not
// rewritten: validation continues to reject them.
fn branded_principal(value: &str) -> String {
    value.strip_prefix("arn:aws:iam::").map_or_else(
        || value.to_owned(),
        |suffix| format!("arn:heterocloud:iam::{suffix}"),
    )
}

fn serialize_principal_identifier<S: Serializer>(
    value: &str,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    branded_principal(value).serialize(serializer)
}

fn branded_trust_policy(value: &Value) -> Value {
    fn rewrite_statement(statement: &mut Value) {
        let Some(principal) = statement
            .get_mut("Principal")
            .and_then(Value::as_object_mut)
        else {
            return;
        };
        if principal.contains_key("AWS") && principal.contains_key("HeteroCloud") {
            return;
        }
        if let Some(old) = principal.remove("AWS") {
            principal.insert("HeteroCloud".into(), old);
        }
        if let Some(accounts) = principal.get_mut("HeteroCloud") {
            match accounts {
                Value::String(account) => *account = branded_principal(account),
                Value::Array(values) => {
                    for value in values {
                        if let Value::String(account) = value {
                            *account = branded_principal(account);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    let mut output = value.clone();
    if let Some(statements) = output.get_mut("Statement") {
        match statements {
            Value::Array(statements) => statements.iter_mut().for_each(rewrite_statement),
            statement => rewrite_statement(statement),
        }
    }
    output
}

fn serialize_trust_policy<S: Serializer>(value: &Value, serializer: S) -> Result<S::Ok, S::Error> {
    branded_trust_policy(value).serialize(serializer)
}

impl Store {
    pub async fn browser_roles_enabled(&self) -> Result<bool, StoreError> {
        Ok(sqlx::query_scalar::<_, bool>(
            "SELECT browser_roles_enabled FROM cloud_security_activation WHERE singleton = true",
        )
        .fetch_one(&self.pool)
        .await?)
    }

    pub async fn browser_active_role(
        &self,
        browser_hash: &[u8; 32],
    ) -> Result<Option<BrowserRoleSession>, StoreError> {
        Ok(sqlx::query_as::<_, BrowserRoleSession>("SELECT rs.id AS session_id, rs.organization_id,
            a.account_number::text AS account_number, a.name AS account_name, rs.role_principal_id AS principal_id,
            ('arn:heterocloud:iam::' || a.account_number::text || ':role/' || r.name) AS role_arn,
            rs.session_name, rs.expires_at, (rs.revoked_at IS NULL AND rs.expires_at > now() AND p.enabled AND identity_center_session_is_active(rs.id)) AS active
            FROM sessions s JOIN iam_role_sessions rs ON rs.id = s.active_role_session_id
            JOIN iam_roles r ON r.principal_id = rs.role_principal_id AND r.organization_id = rs.organization_id
            JOIN principals p ON p.id = r.principal_id AND p.organization_id = r.organization_id
            JOIN organizations a ON a.id = rs.organization_id
            WHERE s.token_hash = $1 AND s.expires_at > now()")
            .bind(browser_hash.as_slice()).fetch_optional(&self.pool).await?)
    }

    /// The expected previous role prevents concurrent browser switches from
    /// silently overwriting a newer identity.
    pub async fn switch_browser_role(
        &self,
        browser_hash: &[u8; 32],
        expected: Option<Uuid>,
        new_role_session: Uuid,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        let changed = sqlx::query("UPDATE sessions s SET active_role_session_id = $3
            WHERE s.token_hash = $1 AND s.expires_at > now() AND s.active_role_session_id IS NOT DISTINCT FROM $2
            AND ($2::uuid IS NULL OR EXISTS (SELECT 1 FROM iam_role_sessions previous JOIN principals previous_principal
                ON previous_principal.id=previous.role_principal_id AND previous_principal.organization_id=previous.organization_id
                WHERE previous.id=$2 AND previous.revoked_at IS NULL AND previous.expires_at > now() AND previous_principal.enabled))
            AND EXISTS (SELECT 1 FROM iam_role_sessions rs JOIN principals p ON p.id=rs.role_principal_id AND p.organization_id=rs.organization_id
                WHERE rs.id=$3 AND rs.revoked_at IS NULL AND rs.expires_at > now() AND p.enabled)")
            .bind(browser_hash.as_slice()).bind(expected).bind(new_role_session).execute(&mut *tx).await?;
        if changed.rows_affected() != 1 {
            return Err(StoreError::Conflict);
        }
        if let Some(previous) = expected {
            sqlx::query(
                "UPDATE iam_role_sessions SET revoked_at=now() WHERE id=$1 AND revoked_at IS NULL",
            )
            .bind(previous)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn clear_browser_role(&self, browser_hash: &[u8; 32]) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        let current = sqlx::query_scalar::<_, Option<Uuid>>("SELECT active_role_session_id FROM sessions WHERE token_hash=$1 AND expires_at > now() FOR UPDATE")
            .bind(browser_hash.as_slice()).fetch_optional(&mut *tx).await?.ok_or(StoreError::NotFound)?;
        sqlx::query("UPDATE sessions SET active_role_session_id=NULL WHERE token_hash=$1")
            .bind(browser_hash.as_slice())
            .execute(&mut *tx)
            .await?;
        if let Some(id) = current {
            sqlx::query(
                "UPDATE iam_role_sessions SET revoked_at=COALESCE(revoked_at, now()) WHERE id=$1",
            )
            .bind(id)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }
    pub async fn roles_enabled(&self) -> Result<bool, StoreError> {
        Ok(sqlx::query_scalar::<_, bool>(
            "SELECT roles_enabled FROM cloud_security_activation WHERE singleton = true",
        )
        .fetch_one(&self.pool)
        .await?)
    }

    pub async fn create_iam_role(
        &self,
        account: OrganizationId,
        name: &str,
        trust: &Value,
        max_session_seconds: i32,
        quota_user: Option<UserId>,
    ) -> Result<IamRole, StoreError> {
        let mut tx = self.pool.begin().await?;
        let principal = PrincipalId::new();
        let inserted = sqlx::query("INSERT INTO principals (id, organization_id, kind, name, enabled, quota_user_id)
            SELECT $1, $2, 'service_account', $3, true, $4
            WHERE $4::uuid IS NULL OR EXISTS (SELECT 1 FROM organization_memberships WHERE organization_id = $2 AND user_id = $4)")
            .bind(principal.0).bind(account.0).bind(name).bind(quota_user.map(|id| id.0)).execute(&mut *tx).await?;
        if inserted.rows_affected() != 1 {
            return Err(StoreError::NotFound);
        }
        sqlx::query("INSERT INTO iam_roles (principal_id, organization_id, name, trust_policy, max_session_seconds) VALUES ($1, $2, $3, $4, $5)")
            .bind(principal.0).bind(account.0).bind(name).bind(trust).bind(max_session_seconds).execute(&mut *tx).await?;
        let query = format!("{ROLE_SELECT} WHERE r.principal_id = $1 AND r.organization_id = $2");
        let role = sqlx::query_as::<_, IamRole>(sqlx::AssertSqlSafe(query.as_str()))
            .bind(principal.0)
            .bind(account.0)
            .fetch_one(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(role)
    }

    /// Whether this principal is an IAM role. Used to avoid persisting an
    /// assumed role's ephemeral permissions as an unbounded background grant.
    pub async fn is_iam_role_principal(
        &self,
        account: OrganizationId,
        principal: PrincipalId,
    ) -> Result<bool, StoreError> {
        Ok(sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM iam_roles WHERE organization_id = $1 AND principal_id = $2)",
        )
        .bind(account.0)
        .bind(principal.0)
        .fetch_one(&self.pool)
        .await?)
    }

    pub async fn is_identity_center_role(
        &self,
        account: OrganizationId,
        principal: PrincipalId,
    ) -> Result<bool, StoreError> {
        Ok(sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM identity_center_account_roles WHERE account_id=$1 AND role_principal_id=$2)")
            .bind(account.0).bind(principal.0).fetch_one(&self.pool).await?)
    }

    pub async fn list_iam_roles(
        &self,
        account: OrganizationId,
    ) -> Result<Vec<IamRole>, StoreError> {
        let query =
            format!("{ROLE_SELECT} WHERE r.organization_id = $1 ORDER BY r.name, r.principal_id");
        Ok(
            sqlx::query_as::<_, IamRole>(sqlx::AssertSqlSafe(query.as_str()))
                .bind(account.0)
                .fetch_all(&self.pool)
                .await?,
        )
    }

    pub async fn find_iam_role(
        &self,
        account_number: &str,
        name: &str,
    ) -> Result<Option<IamRole>, StoreError> {
        let query = format!("{ROLE_SELECT} WHERE a.account_number::text = $1 AND r.name = $2");
        Ok(
            sqlx::query_as::<_, IamRole>(sqlx::AssertSqlSafe(query.as_str()))
                .bind(account_number)
                .bind(name)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    pub async fn update_iam_role_trust(
        &self,
        account: OrganizationId,
        principal: PrincipalId,
        trust: &Value,
    ) -> Result<(), StoreError> {
        let changed = sqlx::query("UPDATE iam_roles SET trust_policy = $3, updated_at = now() WHERE organization_id = $1 AND principal_id = $2 AND NOT EXISTS (SELECT 1 FROM identity_center_account_roles WHERE account_id=$1 AND role_principal_id=$2)")
            .bind(account.0).bind(principal.0).bind(trust).execute(&self.pool).await?;
        if changed.rows_affected() != 1 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    pub async fn delete_iam_role(
        &self,
        account: OrganizationId,
        role: PrincipalId,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        let present = sqlx::query_scalar::<_, Uuid>("SELECT r.principal_id FROM iam_roles r JOIN principals p ON p.id = r.principal_id AND p.organization_id = r.organization_id WHERE r.organization_id = $1 AND r.principal_id = $2 FOR UPDATE OF r, p")
            .bind(account.0).bind(role.0).fetch_optional(&mut *tx).await?;
        if present.is_none() {
            return Err(StoreError::NotFound);
        }
        if sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM identity_center_account_roles WHERE account_id=$1 AND role_principal_id=$2)")
            .bind(account.0).bind(role.0).fetch_one(&mut *tx).await? {
            return Err(StoreError::RequestRejected("Account access roles are managed by their permission sets.".into()));
        }
        let in_use = sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM iam_bindings WHERE organization_id = $1 AND principal_id = $2) OR
            EXISTS(SELECT 1 FROM iam_role_sessions WHERE organization_id = $1 AND role_principal_id = $2 AND revoked_at IS NULL AND expires_at > now()) OR
            EXISTS(SELECT 1 FROM sessions s JOIN iam_role_sessions rs ON rs.id=s.active_role_session_id WHERE rs.organization_id=$1 AND rs.role_principal_id=$2)")
            .bind(account.0).bind(role.0).fetch_one(&mut *tx).await?;
        if in_use {
            return Err(StoreError::RequestRejected(
                "Detach role policies and revoke active sessions before deleting the role.".into(),
            ));
        }
        sqlx::query("DELETE FROM principals WHERE organization_id = $1 AND id = $2")
            .bind(account.0)
            .bind(role.0)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Returns canonical account and principal ARNs without exposing secrets.
    pub async fn iam_principal_arn(
        &self,
        account: OrganizationId,
        principal: PrincipalId,
    ) -> Result<Option<(String, String)>, StoreError> {
        Ok(sqlx::query_as::<_, (String, String)>(
            "SELECT a.account_number::text,
                CASE WHEN r.principal_id IS NOT NULL
                    THEN 'arn:heterocloud:iam::' || a.account_number::text || ':role/' || r.name
                    ELSE 'arn:heterocloud:iam::' || a.account_number::text || ':user/' || p.id::text END
             FROM principals p JOIN organizations a ON a.id = p.organization_id
             LEFT JOIN iam_roles r ON r.principal_id = p.id AND r.organization_id = p.organization_id
             WHERE p.organization_id = $1 AND p.id = $2 AND p.enabled")
            .bind(account.0).bind(principal.0).fetch_optional(&self.pool).await?)
    }

    pub async fn create_iam_role_session(
        &self,
        session: NewIamRoleSession<'_>,
    ) -> Result<Uuid, StoreError> {
        let mut tx = self.pool.begin().await?;
        let role = sqlx::query_as::<_, (Value, i32)>(
            "SELECT r.trust_policy, r.max_session_seconds FROM iam_roles r
            JOIN principals p ON p.id = r.principal_id AND p.organization_id = r.organization_id
            WHERE r.organization_id = $1 AND r.principal_id = $2 AND p.enabled FOR UPDATE OF r",
        )
        .bind(session.organization_id.0)
        .bind(session.role_principal_id.0)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(StoreError::NotFound)?;
        if role.0 != *session.expected_trust_policy {
            return Err(StoreError::Conflict);
        }
        if session.duration_seconds > role.1 {
            return Err(StoreError::RequestRejected(
                "Session duration exceeds the role maximum.".into(),
            ));
        }
        sqlx::query("DELETE FROM iam_role_sessions rs WHERE role_principal_id = $1 AND (expires_at < now() - interval '7 days' OR revoked_at < now() - interval '7 days') AND NOT EXISTS (SELECT 1 FROM sessions s WHERE s.active_role_session_id=rs.id)")
            .bind(session.role_principal_id.0).execute(&mut *tx).await?;
        let count = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM iam_role_sessions WHERE role_principal_id = $1 AND revoked_at IS NULL AND expires_at > now()")
            .bind(session.role_principal_id.0).fetch_one(&mut *tx).await?;
        if count >= 1000 {
            return Err(StoreError::RequestRejected(
                "Too many active role sessions.".into(),
            ));
        }
        let id = Uuid::now_v7();
        let policy = session
            .session_policy
            .map(serde_json::to_value)
            .transpose()?;
        sqlx::query("INSERT INTO iam_role_sessions (id, organization_id, role_principal_id, source_organization_id,
            source_principal_id, source_arn, session_name, session_policy, token_hash, expires_at)
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
            .bind(id).bind(session.organization_id.0).bind(session.role_principal_id.0)
            .bind(session.source_organization_id.0).bind(session.source_principal_id.0).bind(session.source_arn)
            .bind(session.session_name).bind(policy).bind(session.token_hash.as_slice()).bind(session.expires_at)
            .execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(id)
    }

    pub async fn authenticate_iam_role_session(
        &self,
        token_hash: &[u8; 32],
    ) -> Result<Option<IamRoleSession>, StoreError> {
        let row = sqlx::query_as::<_, RoleSessionRow>("SELECT s.id, s.organization_id, s.role_principal_id AS principal_id,
            s.source_organization_id, s.source_principal_id, s.source_arn, s.session_name, s.expires_at, s.session_policy, s.identity_center_user_id
            FROM iam_role_sessions s JOIN principals p ON p.id=s.role_principal_id AND p.organization_id=s.organization_id
            WHERE s.token_hash=$1 AND s.revoked_at IS NULL AND s.expires_at > now() AND p.enabled AND identity_center_session_is_active(s.id)")
            .bind(token_hash.as_slice()).fetch_optional(&self.pool).await?;
        row.map(|row| {
            Ok(IamRoleSession {
                id: row.id,
                organization_id: row.organization_id,
                principal_id: row.principal_id,
                source_organization_id: row.source_organization_id,
                source_principal_id: row.source_principal_id,
                source_arn: row.source_arn,
                session_name: row.session_name,
                expires_at: row.expires_at,
                session_policy: row.session_policy.map(serde_json::from_value).transpose()?,
                identity_center_user_id: row.identity_center_user_id,
            })
        })
        .transpose()
    }

    pub async fn authenticate_browser_role_session(
        &self,
        session_id: Uuid,
    ) -> Result<Option<IamRoleSession>, StoreError> {
        let row = sqlx::query_as::<_, RoleSessionRow>("SELECT s.id, s.organization_id, s.role_principal_id AS principal_id,
            s.source_organization_id, s.source_principal_id, s.source_arn, s.session_name, s.expires_at, s.session_policy, s.identity_center_user_id
            FROM iam_role_sessions s JOIN principals p ON p.id=s.role_principal_id AND p.organization_id=s.organization_id
            WHERE s.id=$1 AND s.revoked_at IS NULL AND s.expires_at > now() AND p.enabled AND identity_center_session_is_active(s.id)")
            .bind(session_id).fetch_optional(&self.pool).await?;
        row.map(|row| {
            Ok(IamRoleSession {
                id: row.id,
                organization_id: row.organization_id,
                principal_id: row.principal_id,
                source_organization_id: row.source_organization_id,
                source_principal_id: row.source_principal_id,
                source_arn: row.source_arn,
                session_name: row.session_name,
                expires_at: row.expires_at,
                session_policy: row.session_policy.map(serde_json::from_value).transpose()?,
                identity_center_user_id: row.identity_center_user_id,
            })
        })
        .transpose()
    }

    pub async fn revoke_iam_role_session(
        &self,
        account: OrganizationId,
        role: PrincipalId,
        session: Uuid,
    ) -> Result<(), StoreError> {
        let result = sqlx::query("UPDATE iam_role_sessions SET revoked_at = now() WHERE organization_id = $1 AND role_principal_id = $2 AND id = $3 AND revoked_at IS NULL")
            .bind(account.0).bind(role.0).bind(session).execute(&self.pool).await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    pub async fn list_iam_role_sessions(
        &self,
        account: OrganizationId,
        role: PrincipalId,
    ) -> Result<Vec<IamRoleSessionInfo>, StoreError> {
        Ok(sqlx::query_as::<_, IamRoleSessionInfo>("SELECT id, source_arn, session_name, expires_at, revoked_at, created_at
            FROM iam_role_sessions WHERE organization_id = $1 AND role_principal_id = $2 ORDER BY created_at DESC LIMIT 1000")
            .bind(account.0).bind(role.0).fetch_all(&self.pool).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::{branded_principal, branded_trust_policy};
    use serde_json::json;

    #[test]
    fn legacy_trust_is_presented_with_platform_names_without_mutating_the_source() {
        let original = json!({"Version":"2012-10-17","Statement":[
            {"Effect":"Allow","Action":"sts:AssumeRole","Principal":{"AWS":["arn:aws:iam::111111111111:root","222222222222"]}},
            {"Effect":"Deny","Action":"sts:AssumeRole","Principal":{"AWS":"arn:aws:iam::111111111111:user/blocked"}},
            {"Effect":"Deny","Action":"sts:AssumeRole","Principal":"*"}
        ]});
        let shown = branded_trust_policy(&original);
        assert_eq!(
            shown["Statement"][0]["Principal"]["HeteroCloud"][0],
            "arn:heterocloud:iam::111111111111:root"
        );
        assert_eq!(
            shown["Statement"][0]["Principal"]["HeteroCloud"][1],
            "222222222222"
        );
        assert_eq!(
            shown["Statement"][1]["Principal"]["HeteroCloud"],
            "arn:heterocloud:iam::111111111111:user/blocked"
        );
        assert_eq!(shown["Statement"][2]["Principal"], "*");
        assert!(shown["Statement"][0]["Principal"].get("AWS").is_none());
        assert!(original["Statement"][0]["Principal"].get("AWS").is_some());
        assert_eq!(
            branded_principal("arn:aws:iam::111111111111:user/alice"),
            "arn:heterocloud:iam::111111111111:user/alice"
        );
        let ambiguous = json!({"Statement":{"Principal":{"AWS":"*","HeteroCloud":"*"}}});
        assert_eq!(branded_trust_policy(&ambiguous), ambiguous);
    }
}
