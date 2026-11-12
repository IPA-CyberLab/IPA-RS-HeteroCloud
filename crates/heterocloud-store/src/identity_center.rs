use chrono::{DateTime, Duration, Utc};
use heterocloud_domain::{OrganizationId, PolicyDocument, UserId};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{Store, StoreError};

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct IdentityCenterPermissionSet {
    pub id: Uuid,
    pub name: String,
    pub document: Value,
    pub session_duration_seconds: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct IdentityCenterGroup {
    pub id: Uuid,
    pub name: String,
    pub users: Vec<Uuid>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct IdentityCenterUser {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct IdentityCenterAssignment {
    pub id: Uuid,
    pub permission_set_id: Uuid,
    pub permission_set_name: String,
    pub account_id: Uuid,
    pub account_number: String,
    pub account_name: String,
    pub role_principal_id: Uuid,
    pub role_arn: String,
    pub subject_type: String,
    pub subject_id: Uuid,
    pub session_duration_seconds: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize)]
pub struct IdentityCenterSession {
    pub session_id: Uuid,
    pub account_id: Uuid,
    pub principal_id: Uuid,
    pub role_arn: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct SessionAssignment {
    account_id: Uuid,
    role_principal_id: Uuid,
    subject_type: String,
    subject_id: Uuid,
    generation: i64,
    session_duration_seconds: i32,
    role_name: String,
    account_number: String,
}

const ASSIGNMENT_SELECT: &str = "SELECT a.id, a.permission_set_id, ps.name AS permission_set_name,
    a.account_id, account.account_number::text AS account_number, account.name AS account_name,
    a.role_principal_id, ('arn:heterocloud:iam::' || account.account_number::text || ':role/' || r.name) AS role_arn,
    a.subject_type, a.subject_id, ps.session_duration_seconds, a.created_at
    FROM identity_center_assignments a
    JOIN identity_center_permission_sets ps ON ps.id=a.permission_set_id AND ps.organization_id=a.organization_id
    JOIN organizations account ON account.id=a.account_id
    JOIN iam_roles r ON r.principal_id=a.role_principal_id AND r.organization_id=a.account_id";

async fn management_organization(
    tx: &mut Transaction<'_, Postgres>,
    management: OrganizationId,
) -> Result<(Uuid, String), StoreError> {
    sqlx::query_as::<_, (Uuid, String)>(
        "SELECT c.id, o.account_number::text FROM cloud_organizations c
        JOIN organizations o ON o.id=c.management_account_id WHERE c.management_account_id=$1",
    )
    .bind(management.0)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(StoreError::NotFound)
}

async fn require_directory_user(
    tx: &mut Transaction<'_, Postgres>,
    management: OrganizationId,
    user: UserId,
) -> Result<Uuid, StoreError> {
    sqlx::query_scalar::<_, Uuid>("SELECT m.principal_id FROM organization_memberships m
        JOIN principals p ON p.id=m.principal_id AND p.organization_id=m.organization_id AND p.enabled
        JOIN users u ON u.id=m.user_id AND u.status='active'
        WHERE m.organization_id=$1 AND m.user_id=$2 FOR SHARE OF m,p,u")
        .bind(management.0).bind(user.0).fetch_optional(&mut **tx).await?.ok_or(StoreError::NotFound)
}

// A target-account membership gives the assigned human a quota owner without
// granting IAM permissions. Revoking the assignment retains that identity and
// any resources; only an explicit IAM policy can grant it independent access.
async fn ensure_account_user(
    tx: &mut Transaction<'_, Postgres>,
    account: Uuid,
    user: Uuid,
) -> Result<(), StoreError> {
    sqlx::query("SELECT id FROM organizations WHERE id=$1 FOR UPDATE")
        .bind(account)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(StoreError::NotFound)?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM organization_memberships WHERE organization_id=$1 AND user_id=$2)")
        .bind(account).bind(user).fetch_one(&mut **tx).await?;
    if exists {
        return Ok(());
    }
    let detached: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM principals WHERE organization_id=$1 AND user_id=$2)",
    )
    .bind(account)
    .bind(user)
    .fetch_one(&mut **tx)
    .await?;
    if detached {
        return Err(StoreError::RequestRejected(
            "The account has a detached principal for this user; reconcile it before assigning access.".into(),
        ));
    }
    let principal = Uuid::now_v7();
    let name = format!("IdentityCenter-{}", user.simple());
    sqlx::query(
        "INSERT INTO principals(id,organization_id,kind,name,user_id,quota_user_id,enabled)
        VALUES($1,$2,'user',$3,$4,$4,true)",
    )
    .bind(principal)
    .bind(account)
    .bind(name)
    .bind(user)
    .execute(&mut **tx)
    .await?;
    sqlx::query("INSERT INTO organization_memberships(organization_id,user_id,principal_id,role) VALUES($1,$2,$3,'member')")
        .bind(account).bind(user).bind(principal).execute(&mut **tx).await?;
    Ok(())
}

impl Store {
    pub async fn identity_center_enabled(&self) -> Result<bool, StoreError> {
        Ok(sqlx::query_scalar(
            "SELECT identity_center_enabled FROM cloud_security_activation WHERE singleton=true",
        )
        .fetch_one(&self.pool)
        .await?)
    }

    pub async fn enable_identity_center(&self) -> Result<(), StoreError> {
        let result = sqlx::query("UPDATE cloud_security_activation SET identity_center_enabled=true WHERE singleton=true")
            .execute(&self.pool).await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::Invariant(
                "missing cloud security activation row",
            ));
        }
        Ok(())
    }

    pub async fn identity_center_permission_sets(
        &self,
        management: OrganizationId,
    ) -> Result<Vec<IdentityCenterPermissionSet>, StoreError> {
        Ok(sqlx::query_as::<_, IdentityCenterPermissionSet>("SELECT ps.id,ps.name,ps.document,ps.session_duration_seconds,ps.created_at,ps.updated_at
            FROM identity_center_permission_sets ps JOIN cloud_organizations c ON c.id=ps.organization_id
            WHERE c.management_account_id=$1 AND ps.deleted_at IS NULL ORDER BY ps.name,ps.id")
            .bind(management.0).fetch_all(&self.pool).await?)
    }

    pub async fn create_identity_center_permission_set(
        &self,
        management: OrganizationId,
        name: &str,
        document: &PolicyDocument,
        duration: i32,
    ) -> Result<IdentityCenterPermissionSet, StoreError> {
        let mut tx = self.pool.begin().await?;
        let (organization, _) = management_organization(&mut tx, management).await?;
        let row = sqlx::query_as::<_, IdentityCenterPermissionSet>("INSERT INTO identity_center_permission_sets(id,organization_id,name,document,session_duration_seconds)
            VALUES($1,$2,$3,$4,$5) RETURNING id,name,document,session_duration_seconds,created_at,updated_at")
            .bind(Uuid::now_v7()).bind(organization).bind(name).bind(serde_json::to_value(document)?).bind(duration)
            .fetch_one(&mut *tx).await?;
        tx.commit().await?;
        Ok(row)
    }

    pub async fn update_identity_center_permission_set(
        &self,
        management: OrganizationId,
        id: Uuid,
        document: &PolicyDocument,
        duration: i32,
        digest: &str,
    ) -> Result<IdentityCenterPermissionSet, StoreError> {
        let mut tx = self.pool.begin().await?;
        let (organization, _) = management_organization(&mut tx, management).await?;
        let value = serde_json::to_value(document)?;
        let row = sqlx::query_as::<_, IdentityCenterPermissionSet>("UPDATE identity_center_permission_sets SET document=$3,session_duration_seconds=$4,updated_at=now()
            WHERE organization_id=$1 AND id=$2 AND deleted_at IS NULL RETURNING id,name,document,session_duration_seconds,created_at,updated_at")
            .bind(organization).bind(id).bind(&value).bind(duration).fetch_optional(&mut *tx).await?.ok_or(StoreError::NotFound)?;
        sqlx::query("UPDATE iam_policies p SET document=$3,semantics_digest=$4,updated_at=now() FROM identity_center_account_roles r
            WHERE r.organization_id=$1 AND r.permission_set_id=$2 AND p.id=r.policy_id AND p.organization_id=r.account_id")
            .bind(organization).bind(id).bind(&value).bind(digest).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(row)
    }

    pub async fn delete_identity_center_permission_set(
        &self,
        management: OrganizationId,
        id: Uuid,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        let (organization, _) = management_organization(&mut tx, management).await?;
        let present: Option<Uuid> = sqlx::query_scalar("SELECT id FROM identity_center_permission_sets WHERE organization_id=$1 AND id=$2 AND deleted_at IS NULL FOR UPDATE")
            .bind(organization).bind(id).fetch_optional(&mut *tx).await?;
        if present.is_none() {
            return Err(StoreError::NotFound);
        }
        let active: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM identity_center_assignments WHERE organization_id=$1 AND permission_set_id=$2 AND revoked_at IS NULL)")
            .bind(organization).bind(id).fetch_one(&mut *tx).await?;
        if active {
            return Err(StoreError::RequestRejected(
                "Remove all account assignments before deleting the permission set.".into(),
            ));
        }
        sqlx::query("UPDATE identity_center_permission_sets SET deleted_at=now(),updated_at=now() WHERE id=$1")
            .bind(id).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn identity_center_users(
        &self,
        management: OrganizationId,
    ) -> Result<Vec<IdentityCenterUser>, StoreError> {
        Ok(sqlx::query_as::<_, IdentityCenterUser>("SELECT u.id,u.email,u.display_name FROM organization_memberships m
            JOIN cloud_organizations c ON c.management_account_id=m.organization_id
            JOIN principals p ON p.id=m.principal_id AND p.organization_id=m.organization_id AND p.enabled
            JOIN users u ON u.id=m.user_id AND u.status='active' WHERE m.organization_id=$1 ORDER BY u.display_name,u.id")
            .bind(management.0).fetch_all(&self.pool).await?)
    }

    pub async fn identity_center_groups(
        &self,
        management: OrganizationId,
    ) -> Result<Vec<IdentityCenterGroup>, StoreError> {
        Ok(sqlx::query_as::<_, IdentityCenterGroup>("SELECT g.id,g.name,g.created_at, ARRAY(SELECT m.user_id FROM identity_center_group_members m WHERE m.group_id=g.id ORDER BY m.user_id) AS users
            FROM identity_center_groups g JOIN cloud_organizations c ON c.id=g.organization_id
            WHERE c.management_account_id=$1 AND g.deleted_at IS NULL ORDER BY g.name,g.id")
            .bind(management.0).fetch_all(&self.pool).await?)
    }

    pub async fn create_identity_center_group(
        &self,
        management: OrganizationId,
        name: &str,
    ) -> Result<IdentityCenterGroup, StoreError> {
        let mut tx = self.pool.begin().await?;
        let (organization, _) = management_organization(&mut tx, management).await?;
        let id = Uuid::now_v7();
        sqlx::query("INSERT INTO identity_center_groups(id,organization_id,name) VALUES($1,$2,$3)")
            .bind(id)
            .bind(organization)
            .bind(name)
            .execute(&mut *tx)
            .await?;
        let row = sqlx::query_as::<_, IdentityCenterGroup>("SELECT id,name,created_at,ARRAY[]::uuid[] AS users FROM identity_center_groups WHERE id=$1")
            .bind(id).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        Ok(row)
    }

    pub async fn change_identity_center_group_member(
        &self,
        management: OrganizationId,
        group: Uuid,
        user: UserId,
        add: bool,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        let (organization, _) = management_organization(&mut tx, management).await?;
        sqlx::query("SELECT id FROM identity_center_groups WHERE organization_id=$1 AND id=$2 AND deleted_at IS NULL FOR UPDATE")
            .bind(organization).bind(group).fetch_optional(&mut *tx).await?.ok_or(StoreError::NotFound)?;
        if add {
            require_directory_user(&mut tx, management, user).await?;
            let accounts: Vec<Uuid> = sqlx::query_scalar("SELECT DISTINCT account_id FROM identity_center_assignments
                WHERE organization_id=$1 AND subject_type='group' AND subject_id=$2 AND revoked_at IS NULL ORDER BY account_id")
                .bind(organization).bind(group).fetch_all(&mut *tx).await?;
            for account in accounts {
                ensure_account_user(&mut tx, account, user.0).await?;
            }
            let added = sqlx::query("INSERT INTO identity_center_group_members(id,organization_id,group_id,user_id) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING")
                .bind(Uuid::now_v7()).bind(organization).bind(group).bind(user.0).execute(&mut *tx).await?;
            if added.rows_affected() != 1 {
                return Err(StoreError::AlreadyExists);
            }
        } else {
            let removed = sqlx::query("DELETE FROM identity_center_group_members WHERE organization_id=$1 AND group_id=$2 AND user_id=$3")
                .bind(organization).bind(group).bind(user.0).execute(&mut *tx).await?;
            if removed.rows_affected() != 1 {
                return Err(StoreError::NotFound);
            }
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn delete_identity_center_group(
        &self,
        management: OrganizationId,
        group: Uuid,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        let (organization, _) = management_organization(&mut tx, management).await?;
        sqlx::query("SELECT id FROM identity_center_groups WHERE organization_id=$1 AND id=$2 AND deleted_at IS NULL FOR UPDATE")
            .bind(organization).bind(group).fetch_optional(&mut *tx).await?.ok_or(StoreError::NotFound)?;
        let active: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM identity_center_assignments WHERE organization_id=$1 AND subject_type='group' AND subject_id=$2 AND revoked_at IS NULL)")
            .bind(organization).bind(group).fetch_one(&mut *tx).await?;
        if active {
            return Err(StoreError::RequestRejected(
                "Remove all account assignments before deleting the group.".into(),
            ));
        }
        sqlx::query(
            "DELETE FROM identity_center_group_members WHERE organization_id=$1 AND group_id=$2",
        )
        .bind(organization)
        .bind(group)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE identity_center_groups SET deleted_at=now() WHERE organization_id=$1 AND id=$2",
        )
        .bind(organization)
        .bind(group)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn identity_center_assignments(
        &self,
        management: OrganizationId,
        user: Option<UserId>,
    ) -> Result<Vec<IdentityCenterAssignment>, StoreError> {
        let query = format!("{ASSIGNMENT_SELECT} JOIN cloud_organizations co ON co.id=a.organization_id
            WHERE co.management_account_id=$1 AND a.revoked_at IS NULL AND ps.deleted_at IS NULL
              AND ($2::uuid IS NULL OR (a.subject_type='user' AND a.subject_id=$2) OR (a.subject_type='group' AND EXISTS
                (SELECT 1 FROM identity_center_groups g JOIN identity_center_group_members gm ON gm.group_id=g.id AND gm.organization_id=g.organization_id
                 WHERE g.id=a.subject_id AND g.organization_id=a.organization_id AND g.deleted_at IS NULL AND gm.user_id=$2)))
            ORDER BY account.name,ps.name,a.id");
        Ok(
            sqlx::query_as::<_, IdentityCenterAssignment>(sqlx::AssertSqlSafe(query.as_str()))
                .bind(management.0)
                .bind(user.map(|v| v.0))
                .fetch_all(&self.pool)
                .await?,
        )
    }

    pub async fn create_identity_center_assignment(
        &self,
        management: OrganizationId,
        permission_set: Uuid,
        account: OrganizationId,
        subject_type: &str,
        subject: Uuid,
        digest: &str,
    ) -> Result<IdentityCenterAssignment, StoreError> {
        let mut tx = self.pool.begin().await?;
        let (organization, management_number) =
            management_organization(&mut tx, management).await?;
        let document: Value = sqlx::query_scalar("SELECT document FROM identity_center_permission_sets WHERE organization_id=$1 AND id=$2 AND deleted_at IS NULL FOR UPDATE")
            .bind(organization).bind(permission_set).fetch_optional(&mut *tx).await?.ok_or(StoreError::NotFound)?;
        sqlx::query("SELECT account_id FROM cloud_organization_accounts WHERE organization_id=$1 AND account_id=$2 FOR SHARE")
            .bind(organization).bind(account.0).fetch_optional(&mut *tx).await?.ok_or(StoreError::NotFound)?;
        match subject_type {
            "user" => {
                require_directory_user(&mut tx, management, UserId(subject)).await?;
                ensure_account_user(&mut tx, account.0, subject).await?;
            }
            "group" => {
                sqlx::query("SELECT id FROM identity_center_groups WHERE organization_id=$1 AND id=$2 AND deleted_at IS NULL FOR UPDATE")
                    .bind(organization).bind(subject).fetch_optional(&mut *tx).await?.ok_or(StoreError::NotFound)?;
                let users: Vec<Uuid> = sqlx::query_scalar("SELECT user_id FROM identity_center_group_members WHERE organization_id=$1 AND group_id=$2 ORDER BY user_id")
                    .bind(organization).bind(subject).fetch_all(&mut *tx).await?;
                for user in users {
                    require_directory_user(&mut tx, management, UserId(user)).await?;
                    ensure_account_user(&mut tx, account.0, user).await?;
                }
            }
            _ => {
                return Err(StoreError::RequestRejected(
                    "Subject type must be user or group.".into(),
                ));
            }
        }
        let existing: Option<Uuid> = sqlx::query_scalar("SELECT role_principal_id FROM identity_center_account_roles WHERE organization_id=$1 AND permission_set_id=$2 AND account_id=$3")
            .bind(organization).bind(permission_set).bind(account.0).fetch_optional(&mut *tx).await?;
        let role = if let Some(role) = existing {
            role
        } else {
            let role = Uuid::now_v7();
            let policy = Uuid::now_v7();
            let name = format!("HCIdentityCenter_{}", permission_set.simple());
            let trust = json!({"Version":"2012-10-17","Statement":{"Effect":"Allow","Action":"sts:AssumeRole",
                "Principal":{"HeteroCloud":format!("arn:heterocloud:iam::{management_number}:root")}}});
            sqlx::query("INSERT INTO principals(id,organization_id,kind,name,enabled) VALUES($1,$2,'service_account',$3,true)")
                .bind(role).bind(account.0).bind(&name).execute(&mut *tx).await?;
            sqlx::query("INSERT INTO iam_roles(principal_id,organization_id,name,trust_policy,max_session_seconds) VALUES($1,$2,$3,$4,43200)")
                .bind(role).bind(account.0).bind(&name).bind(trust).execute(&mut *tx).await?;
            sqlx::query("INSERT INTO iam_policies(id,organization_id,name,document,semantics_digest) VALUES($1,$2,$3,$4,$5)")
                .bind(policy).bind(account.0).bind(&name).bind(&document).bind(digest).execute(&mut *tx).await?;
            sqlx::query("INSERT INTO iam_bindings(id,organization_id,principal_id,policy_id) VALUES($1,$2,$3,$4)")
                .bind(Uuid::now_v7()).bind(account.0).bind(role).bind(policy).execute(&mut *tx).await?;
            sqlx::query("INSERT INTO identity_center_account_roles(organization_id,permission_set_id,account_id,role_principal_id,policy_id) VALUES($1,$2,$3,$4,$5)")
                .bind(organization).bind(permission_set).bind(account.0).bind(role).bind(policy).execute(&mut *tx).await?;
            role
        };
        let id: Uuid = sqlx::query_scalar("INSERT INTO identity_center_assignments(id,organization_id,permission_set_id,account_id,role_principal_id,subject_type,subject_id)
            VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(organization_id,permission_set_id,account_id,subject_type,subject_id)
            DO UPDATE SET revoked_at=NULL,generation=identity_center_assignments.generation+1
            WHERE identity_center_assignments.revoked_at IS NOT NULL RETURNING id")
            .bind(Uuid::now_v7()).bind(organization).bind(permission_set).bind(account.0).bind(role).bind(subject_type).bind(subject)
            .fetch_optional(&mut *tx).await?.ok_or(StoreError::AlreadyExists)?;
        let query = format!("{ASSIGNMENT_SELECT} WHERE a.id=$1");
        let item =
            sqlx::query_as::<_, IdentityCenterAssignment>(sqlx::AssertSqlSafe(query.as_str()))
                .bind(id)
                .fetch_one(&mut *tx)
                .await?;
        tx.commit().await?;
        Ok(item)
    }

    pub async fn revoke_identity_center_assignment(
        &self,
        management: OrganizationId,
        assignment: Uuid,
    ) -> Result<(), StoreError> {
        let result = sqlx::query("UPDATE identity_center_assignments a SET revoked_at=now() FROM cloud_organizations c
            WHERE c.id=a.organization_id AND c.management_account_id=$1 AND a.id=$2 AND a.revoked_at IS NULL")
            .bind(management.0).bind(assignment).execute(&self.pool).await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    pub async fn create_identity_center_session(
        &self,
        management: OrganizationId,
        assignment: Uuid,
        user: UserId,
        token_hash: &[u8; 32],
    ) -> Result<IdentityCenterSession, StoreError> {
        let mut tx = self.pool.begin().await?;
        let (organization, source_number) = management_organization(&mut tx, management).await?;
        let source = require_directory_user(&mut tx, management, user).await?;
        let row = sqlx::query_as::<_, SessionAssignment>("SELECT a.account_id,a.role_principal_id,a.subject_type,a.subject_id,a.generation,
            ps.session_duration_seconds,r.name AS role_name,account.account_number::text AS account_number
            FROM identity_center_assignments a JOIN identity_center_permission_sets ps ON ps.id=a.permission_set_id AND ps.organization_id=a.organization_id
            JOIN iam_roles r ON r.principal_id=a.role_principal_id AND r.organization_id=a.account_id
            JOIN principals p ON p.id=r.principal_id AND p.enabled JOIN organizations account ON account.id=a.account_id
            WHERE a.organization_id=$1 AND a.id=$2 AND a.revoked_at IS NULL AND ps.deleted_at IS NULL FOR UPDATE OF a,r")
            .bind(organization).bind(assignment).fetch_optional(&mut *tx).await?.ok_or(StoreError::NotFound)?;
        let group_membership = match row.subject_type.as_str() {
            "user" if row.subject_id == user.0 => None,
            "group" => {
                let membership: Uuid = sqlx::query_scalar("SELECT gm.id FROM identity_center_group_members gm JOIN identity_center_groups g ON g.id=gm.group_id AND g.organization_id=gm.organization_id
                    WHERE gm.organization_id=$1 AND gm.group_id=$2 AND gm.user_id=$3 AND g.deleted_at IS NULL FOR SHARE OF gm,g")
                    .bind(organization).bind(row.subject_id).bind(user.0).fetch_optional(&mut *tx).await?.ok_or(StoreError::NotFound)?;
                Some(membership)
            }
            _ => return Err(StoreError::NotFound),
        };
        let (target_principal, target_membership): (Uuid, Uuid) = sqlx::query_as("SELECT m.principal_id,m.identity_center_membership_id FROM organization_memberships m JOIN principals p ON p.id=m.principal_id AND p.organization_id=m.organization_id AND p.enabled WHERE m.organization_id=$1 AND m.user_id=$2 FOR SHARE OF m,p")
            .bind(row.account_id).bind(user.0).fetch_optional(&mut *tx).await?.ok_or(StoreError::NotFound)?;
        let source_membership: Uuid = sqlx::query_scalar("SELECT identity_center_membership_id FROM organization_memberships WHERE organization_id=$1 AND user_id=$2 AND principal_id=$3 FOR SHARE")
            .bind(management.0).bind(user.0).bind(source).fetch_optional(&mut *tx).await?.ok_or(StoreError::NotFound)?;
        sqlx::query("DELETE FROM iam_role_sessions rs WHERE role_principal_id=$1 AND (expires_at<now()-interval '7 days' OR revoked_at<now()-interval '7 days') AND NOT EXISTS(SELECT 1 FROM sessions s WHERE s.active_role_session_id=rs.id)")
            .bind(row.role_principal_id).execute(&mut *tx).await?;
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM iam_role_sessions WHERE role_principal_id=$1 AND revoked_at IS NULL AND expires_at>now()")
            .bind(row.role_principal_id).fetch_one(&mut *tx).await?;
        if count >= 1000 {
            return Err(StoreError::RequestRejected(
                "Too many active role sessions.".into(),
            ));
        }
        let id = Uuid::now_v7();
        let expires_at = Utc::now() + Duration::seconds(i64::from(row.session_duration_seconds));
        let source_arn = format!("arn:heterocloud:iam::{source_number}:user/{source}");
        let session_name = format!("identity-center-{}", user.0.simple());
        sqlx::query("INSERT INTO iam_role_sessions(id,organization_id,role_principal_id,source_organization_id,source_principal_id,source_arn,session_name,token_hash,expires_at,
            identity_center_assignment_id,identity_center_assignment_generation,identity_center_user_id,identity_center_group_membership_id,identity_center_target_principal_id,identity_center_source_membership_id,identity_center_target_membership_id)
            VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)")
            .bind(id).bind(row.account_id).bind(row.role_principal_id).bind(management.0).bind(source).bind(source_arn).bind(session_name)
            .bind(token_hash.as_slice()).bind(expires_at).bind(assignment).bind(row.generation).bind(user.0).bind(group_membership).bind(target_principal).bind(source_membership).bind(target_membership).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(IdentityCenterSession {
            session_id: id,
            account_id: row.account_id,
            principal_id: row.role_principal_id,
            role_arn: format!(
                "arn:heterocloud:iam::{}:role/{}",
                row.account_number, row.role_name
            ),
            expires_at,
        })
    }
}
