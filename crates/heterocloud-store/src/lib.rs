use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use heterocloud_domain::{
    FlashProtocol, FlashSpec, FlowSpec, IamPolicy, MAX_FLASH_SERVICE_PORT, MIN_FLASH_SERVICE_PORT,
    Organization, OrganizationId, PolicyDocument, PolicyId, Principal, PrincipalId, PrincipalKind,
    Project, ProjectId, ResourceQuotaLimits, ServiceInstance, ServiceInstanceId, ServiceState,
    SyouyuSpec, User, UserId, UserStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction, postgres::PgPoolOptions};
use thiserror::Error;
use uuid::Uuid;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

pub const MAX_REALTIME_METRIC_HISTORY_SAMPLES: i64 = 240;
pub const MAX_FLOW_ACCESS_CONTEXT_RECORDS_PER_SERVICE: i64 = 100;
pub const MAX_FLOW_ACCESS_CONTEXT_LIST_SIZE: i64 = MAX_FLOW_ACCESS_CONTEXT_RECORDS_PER_SERVICE;
pub const MAX_FLOW_DEVELOPER_CREDENTIALS_PER_SERVICE: i64 = 100;
pub const MAX_FLOW_DEVELOPER_CREDENTIAL_LIST_SIZE: i64 = 100;
pub const MAX_USER_LOGIN_EVENTS_PER_USER: i64 = 100;

#[derive(Clone)]
pub struct Store {
    pool: PgPool,
}

impl Store {
    pub async fn connect(database_url: &str, max_connections: u32) -> Result<Self, StoreError> {
        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .min_connections(1)
            .connect(database_url)
            .await?;
        Ok(Self { pool })
    }

    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn migrate(&self) -> Result<(), StoreError> {
        MIGRATOR.run(&self.pool).await?;
        Ok(())
    }

    pub async fn ping(&self) -> Result<(), StoreError> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }

    pub async fn list_owner_accounts(&self) -> Result<Vec<OwnerAccountRecord>, StoreError> {
        let rows = sqlx::query_as::<_, OwnerAccountRow>(
            "SELECT u.id, u.email, u.display_name, u.status, u.created_at,
                    (u.password_hash IS NOT NULL) AS has_local_password,
                    COALESCE(identities.items, '[]'::jsonb) AS external_identities,
                    COALESCE(memberships.items, '[]'::jsonb) AS memberships,
                    latest.id AS last_login_id,
                    host(latest.source_ip) AS last_login_ip,
                    latest.authentication_method AS last_login_authentication_method,
                    latest.occurred_at AS last_login_occurred_at,
                    COALESCE(logins.login_count, 0) AS login_count
             FROM users u
             LEFT JOIN LATERAL (
                 SELECT jsonb_agg(
                     jsonb_build_object(
                         'issuer', i.issuer,
                         'subject', i.subject,
                         'created_at', i.created_at
                     ) ORDER BY i.created_at, i.issuer
                 ) AS items
                 FROM user_external_identities i
                 WHERE i.user_id = u.id
             ) identities ON true
             LEFT JOIN LATERAL (
                 SELECT jsonb_agg(
                     jsonb_build_object(
                         'organization_id', m.organization_id,
                         'organization_slug', o.slug,
                         'organization_name', o.name,
                         'principal_id', m.principal_id,
                         'role', m.role
                     ) ORDER BY o.name, o.id
                 ) AS items
                 FROM organization_memberships m
                 JOIN organizations o ON o.id = m.organization_id
                 WHERE m.user_id = u.id
             ) memberships ON true
             LEFT JOIN LATERAL (
                 SELECT l.id, l.source_ip, l.authentication_method, l.occurred_at
                 FROM user_login_events l
                 WHERE l.user_id = u.id
                 ORDER BY l.occurred_at DESC, l.id DESC
                 LIMIT 1
             ) latest ON true
             LEFT JOIN LATERAL (
                 SELECT count(*) AS login_count
                 FROM user_login_events l
                 WHERE l.user_id = u.id
             ) logins ON true
             ORDER BY u.created_at DESC, u.id DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(owner_account_from_row).collect()
    }

    pub async fn list_user_login_events(
        &self,
        user_id: UserId,
        limit: i64,
    ) -> Result<Vec<UserLoginEventRecord>, StoreError> {
        let limit = limit.clamp(1, MAX_USER_LOGIN_EVENTS_PER_USER);
        let events = sqlx::query_as::<_, UserLoginEventRecord>(
            "SELECT id, user_id, host(source_ip) AS source_ip,
                    authentication_method, occurred_at
             FROM user_login_events
             WHERE user_id = $1
             ORDER BY occurred_at DESC, id DESC
             LIMIT $2",
        )
        .bind(user_id.0)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(events)
    }

    pub async fn resource_quota_defaults(&self) -> Result<ResourceQuotaLimits, StoreError> {
        let limits = sqlx::query_scalar::<_, Value>(
            "SELECT limits FROM resource_quota_defaults WHERE singleton = true",
        )
        .fetch_one(&self.pool)
        .await?;
        parse_resource_quota(limits)
    }

    pub async fn effective_resource_quota(
        &self,
        organization_id: OrganizationId,
    ) -> Result<ResourceQuotaLimits, StoreError> {
        let limits = sqlx::query_scalar::<_, Value>(
            "SELECT COALESCE(q.limits, d.limits)
             FROM organizations o
             CROSS JOIN resource_quota_defaults d
             LEFT JOIN organization_resource_quotas q ON q.organization_id = o.id
             WHERE o.id = $1 AND d.singleton = true",
        )
        .bind(organization_id.0)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(StoreError::NotFound)?;
        parse_resource_quota(limits)
    }

    pub async fn update_resource_quota_defaults(
        &self,
        limits: &ResourceQuotaLimits,
    ) -> Result<ResourceQuotaLimits, StoreError> {
        validate_resource_quota(limits)?;
        let value = serde_json::to_value(limits)?;
        let stored = sqlx::query_scalar::<_, Value>(
            "UPDATE resource_quota_defaults
             SET limits = $1, updated_at = now()
             WHERE singleton = true
             RETURNING limits",
        )
        .bind(value)
        .fetch_one(&self.pool)
        .await?;
        parse_resource_quota(stored)
    }

    pub async fn set_organization_resource_quota(
        &self,
        organization_id: OrganizationId,
        limits: &ResourceQuotaLimits,
    ) -> Result<ResourceQuotaLimits, StoreError> {
        validate_resource_quota(limits)?;
        let value = serde_json::to_value(limits)?;
        let stored = sqlx::query_scalar::<_, Value>(
            "INSERT INTO organization_resource_quotas (organization_id, limits)
             SELECT id, $2 FROM organizations WHERE id = $1
             ON CONFLICT (organization_id) DO UPDATE
             SET limits = EXCLUDED.limits, updated_at = now()
             RETURNING limits",
        )
        .bind(organization_id.0)
        .bind(value)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(StoreError::NotFound)?;
        parse_resource_quota(stored)
    }

    pub async fn clear_organization_resource_quota(
        &self,
        organization_id: OrganizationId,
    ) -> Result<ResourceQuotaLimits, StoreError> {
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM organizations WHERE id = $1)",
        )
        .bind(organization_id.0)
        .fetch_one(&self.pool)
        .await?;
        if !exists {
            return Err(StoreError::NotFound);
        }
        sqlx::query("DELETE FROM organization_resource_quotas WHERE organization_id = $1")
            .bind(organization_id.0)
            .execute(&self.pool)
            .await?;
        self.resource_quota_defaults().await
    }

    pub async fn list_resource_quota_tenants(
        &self,
    ) -> Result<Vec<ResourceQuotaTenant>, StoreError> {
        let defaults = self.resource_quota_defaults().await?;
        let rows = sqlx::query_as::<_, ResourceQuotaTenantRow>(
            "SELECT o.id, o.slug, o.name, o.created_at, q.limits AS override_limits
             FROM organizations o
             LEFT JOIN organization_resource_quotas q ON q.organization_id = o.id
             ORDER BY lower(o.name), o.id",
        )
        .fetch_all(&self.pool)
        .await?;
        let allocations = sqlx::query_as::<_, TenantServiceAllocationRow>(
            "SELECT organization_id, provider, spec
             FROM service_instances
             WHERE provider IN ('flow', 'flash', 'syouyu') AND state <> 'deleting'",
        )
        .fetch_all(&self.pool)
        .await?;
        let flow_credentials = sqlx::query_as::<_, TenantCredentialUsageRow>(
            "SELECT c.organization_id, count(*) AS active_credentials
             FROM flow_developer_credentials c
             JOIN service_instances s ON s.id = c.service_instance_id
             WHERE c.revoked_at IS NULL
               AND c.expires_at > now()
               AND s.provider = 'flow'
               AND s.state <> 'deleting'
             GROUP BY c.organization_id, c.service_instance_id",
        )
        .fetch_all(&self.pool)
        .await?;
        let registry_credentials = sqlx::query_as::<_, TenantCredentialUsageRow>(
            "SELECT organization_id, count(*) AS active_credentials
             FROM registry_credentials
             WHERE status IN ('provisioning', 'active')
             GROUP BY organization_id",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut usage_by_organization = BTreeMap::<Uuid, ResourceQuotaUsage>::new();
        for allocation in allocations {
            let usage = usage_by_organization
                .entry(allocation.organization_id)
                .or_default();
            match allocation.provider.as_str() {
                "flow" => {
                    let spec: FlowSpec = serde_json::from_value(allocation.spec)?;
                    usage.add_flow_spec(&spec);
                }
                "flash" => {
                    let spec: FlashSpec = serde_json::from_value(allocation.spec)?;
                    usage.add_flash_spec(&spec);
                }
                "syouyu" => {
                    let spec: SyouyuSpec = serde_json::from_value(allocation.spec)?;
                    usage.add_syouyu_spec(&spec);
                }
                _ => return Err(StoreError::Invariant("unknown quota provider")),
            }
        }
        for row in flow_credentials {
            let count = u64::try_from(row.active_credentials)
                .map_err(|_| StoreError::Invariant("negative Flow credential count"))?;
            let usage = usage_by_organization
                .entry(row.organization_id)
                .or_default();
            usage.flow_developer_credentials =
                usage.flow_developer_credentials.saturating_add(count);
            usage.flow_max_developer_credentials_per_service =
                usage.flow_max_developer_credentials_per_service.max(count);
        }
        for row in registry_credentials {
            let count = u64::try_from(row.active_credentials)
                .map_err(|_| StoreError::Invariant("negative Registry credential count"))?;
            usage_by_organization
                .entry(row.organization_id)
                .or_default()
                .registry_credentials = count;
        }

        rows.into_iter()
            .map(|row| {
                let override_limits = row.override_limits.map(parse_resource_quota).transpose()?;
                let effective_limits = override_limits.clone().unwrap_or_else(|| defaults.clone());
                let usage = usage_by_organization.remove(&row.id).unwrap_or_default();
                Ok(ResourceQuotaTenant {
                    organization: Organization {
                        id: OrganizationId(row.id),
                        slug: row.slug,
                        name: row.name,
                        created_at: row.created_at,
                    },
                    override_limits,
                    effective_limits,
                    usage,
                })
            })
            .collect()
    }

    pub async fn bootstrap_admin(
        &self,
        input: BootstrapAdmin<'_>,
    ) -> Result<SessionUser, StoreError> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('heterocloud-bootstrap', 0))")
            .execute(&mut *transaction)
            .await?;
        if let Some(user) = lookup_user_by_email(&mut transaction, input.email).await? {
            transaction.commit().await?;
            return self
                .session_user(user.id)
                .await?
                .ok_or(StoreError::Invariant(
                    "bootstrap user exists without an organization membership",
                ));
        }

        let user_id = UserId::new();
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        sqlx::query(
            "INSERT INTO users (id, email, display_name, password_hash, status)
             VALUES ($1, lower($2), $3, $4, 'active')",
        )
        .bind(user_id.0)
        .bind(input.email)
        .bind(input.display_name)
        .bind(input.password_hash)
        .execute(&mut *transaction)
        .await?;
        sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1, $2, $3)")
            .bind(organization_id.0)
            .bind(input.organization_slug)
            .bind(input.organization_name)
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "INSERT INTO principals
                (id, organization_id, kind, name, user_id, enabled)
             VALUES ($1, $2, 'user', $3, $4, true)",
        )
        .bind(principal_id.0)
        .bind(organization_id.0)
        .bind(input.display_name)
        .bind(user_id.0)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO organization_memberships
                (organization_id, user_id, principal_id, role)
             VALUES ($1, $2, $3, 'owner')",
        )
        .bind(organization_id.0)
        .bind(user_id.0)
        .bind(principal_id.0)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;

        self.session_user(user_id)
            .await?
            .ok_or(StoreError::Invariant(
                "bootstrap transaction did not produce a session user",
            ))
    }

    pub async fn password_user_by_email(
        &self,
        email: &str,
    ) -> Result<Option<PasswordUser>, StoreError> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT id, email, display_name, password_hash, status, created_at
             FROM users
             WHERE lower(email) = lower($1) AND password_hash IS NOT NULL",
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await?;
        row.map(PasswordUser::try_from).transpose()
    }

    pub async fn create_session(
        &self,
        user_id: UserId,
        token_hash: &[u8; 32],
        expires_at: DateTime<Utc>,
        source_ip: Option<&str>,
        authentication_method: &str,
    ) -> Result<Uuid, StoreError> {
        if !matches!(authentication_method, "local" | "oidc") {
            return Err(StoreError::RequestRejected(
                "unsupported authentication method".to_owned(),
            ));
        }
        let id = Uuid::now_v7();
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO sessions (id, user_id, token_hash, expires_at)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(id)
        .bind(user_id.0)
        .bind(token_hash.as_slice())
        .bind(expires_at)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO user_login_events
                (user_id, session_id, source_ip, authentication_method)
             VALUES ($1, $2, $3::inet, $4)",
        )
        .bind(user_id.0)
        .bind(id)
        .bind(source_ip)
        .bind(authentication_method)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "DELETE FROM user_login_events
             WHERE user_id = $1
               AND id NOT IN (
                   SELECT id
                   FROM user_login_events
                   WHERE user_id = $1
                   ORDER BY occurred_at DESC, id DESC
                   LIMIT $2
               )",
        )
        .bind(user_id.0)
        .bind(MAX_USER_LOGIN_EVENTS_PER_USER)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(id)
    }

    pub async fn create_invitation(
        &self,
        organization_id: OrganizationId,
        created_by: UserId,
        code_hash: &[u8; 32],
        expires_at: DateTime<Utc>,
    ) -> Result<Uuid, StoreError> {
        let id = Uuid::now_v7();
        let result = sqlx::query(
            "INSERT INTO invitations
                (id, code_hash, created_by, organization_id, max_uses, expires_at)
             SELECT $1, $2, $3, m.organization_id, 1, $5
             FROM organization_memberships m
             WHERE m.organization_id = $4 AND m.user_id = $3 AND m.role = 'owner'",
        )
        .bind(id)
        .bind(code_hash.as_slice())
        .bind(created_by.0)
        .bind(organization_id.0)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::NotFound);
        }
        Ok(id)
    }

    pub async fn invitation_available(&self, code_hash: &[u8; 32]) -> Result<bool, StoreError> {
        sqlx::query_scalar(
            "SELECT EXISTS (
                 SELECT 1
                 FROM invitations
                 WHERE code_hash = $1
                   AND revoked_at IS NULL
                   AND expires_at > now()
                   AND max_uses = 1
                   AND used_count = 0
             )",
        )
        .bind(code_hash.as_slice())
        .fetch_one(&self.pool)
        .await
        .map_err(StoreError::from)
    }

    pub async fn register_with_invitation(
        &self,
        input: RegisterWithInvitation<'_>,
    ) -> Result<SessionUser, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let invitation = sqlx::query_as::<_, (Uuid, Option<Uuid>)>(
            "SELECT id, organization_id
             FROM invitations
             WHERE code_hash = $1
               AND revoked_at IS NULL
               AND expires_at > now()
               AND max_uses = 1
               AND used_count = 0
             FOR UPDATE",
        )
        .bind(input.code_hash.as_slice())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(StoreError::InvitationUnavailable)?;
        let organization_id = invitation.1.ok_or(StoreError::InvitationUnavailable)?;
        let consumed = sqlx::query(
            "UPDATE invitations
             SET used_count = 1
             WHERE id = $1
               AND revoked_at IS NULL
               AND expires_at > now()
               AND max_uses = 1
               AND used_count = 0",
        )
        .bind(invitation.0)
        .execute(&mut *transaction)
        .await?;
        if consumed.rows_affected() != 1 {
            return Err(StoreError::InvitationUnavailable);
        }
        if lookup_user_by_email(&mut transaction, input.email)
            .await?
            .is_some()
        {
            return Err(StoreError::AlreadyExists);
        }

        let user_id = UserId::new();
        let principal_id = PrincipalId::new();
        sqlx::query(
            "INSERT INTO users (id, email, display_name, password_hash, status)
             VALUES ($1, lower($2), $3, $4, 'active')",
        )
        .bind(user_id.0)
        .bind(input.email)
        .bind(input.display_name)
        .bind(input.password_hash)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO principals
                (id, organization_id, kind, name, user_id, enabled)
             VALUES ($1, $2, 'user', $3, $4, true)",
        )
        .bind(principal_id.0)
        .bind(organization_id)
        .bind(input.display_name)
        .bind(user_id.0)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO organization_memberships
                (organization_id, user_id, principal_id, role)
             VALUES ($1, $2, $3, 'member')",
        )
        .bind(organization_id)
        .bind(user_id.0)
        .bind(principal_id.0)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        self.session_user(user_id)
            .await?
            .ok_or(StoreError::Invariant(
                "registration transaction did not produce a session user",
            ))
    }

    pub async fn find_or_create_oidc_user(
        &self,
        input: OidcUser<'_>,
    ) -> Result<SessionUser, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let lock_key = format!(
            "heterocloud-oidc:{}:{}:{}",
            input.issuer.len(),
            input.issuer,
            input.subject
        );
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(lock_key)
            .execute(&mut *transaction)
            .await?;

        let existing_user_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT user_id
             FROM user_external_identities
             WHERE issuer = $1 AND subject = $2",
        )
        .bind(input.issuer)
        .bind(input.subject)
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some(user_id) = existing_user_id {
            transaction.commit().await?;
            return self
                .session_user(UserId(user_id))
                .await?
                .ok_or(StoreError::Invariant(
                    "OIDC identity exists without a complete user membership",
                ));
        }

        if lookup_user_by_email(&mut transaction, input.email)
            .await?
            .is_some()
        {
            return Err(StoreError::AlreadyExists);
        }

        let user_id = UserId::new();
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let organization_slug = format!("user-{}", user_id.0.simple());
        sqlx::query(
            "INSERT INTO users (id, email, display_name, password_hash, status)
             VALUES ($1, lower($2), $3, NULL, 'active')",
        )
        .bind(user_id.0)
        .bind(input.email)
        .bind(input.display_name)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO user_external_identities (issuer, subject, user_id)
             VALUES ($1, $2, $3)",
        )
        .bind(input.issuer)
        .bind(input.subject)
        .bind(user_id.0)
        .execute(&mut *transaction)
        .await?;
        sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1, $2, $3)")
            .bind(organization_id.0)
            .bind(organization_slug)
            .bind(input.display_name)
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "INSERT INTO principals
                (id, organization_id, kind, name, user_id, enabled)
             VALUES ($1, $2, 'user', $3, $4, true)",
        )
        .bind(principal_id.0)
        .bind(organization_id.0)
        .bind(input.display_name)
        .bind(user_id.0)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO organization_memberships
                (organization_id, user_id, principal_id, role)
             VALUES ($1, $2, $3, 'owner')",
        )
        .bind(organization_id.0)
        .bind(user_id.0)
        .bind(principal_id.0)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;

        self.session_user(user_id)
            .await?
            .ok_or(StoreError::Invariant(
                "OIDC registration transaction did not produce a session user",
            ))
    }

    pub async fn session_user_by_token_hash(
        &self,
        token_hash: &[u8; 32],
    ) -> Result<Option<SessionUser>, StoreError> {
        let user_id = sqlx::query_scalar::<_, Uuid>(
            "UPDATE sessions
             SET last_seen_at = now()
             WHERE token_hash = $1 AND expires_at > now()
             RETURNING user_id",
        )
        .bind(token_hash.as_slice())
        .fetch_optional(&self.pool)
        .await?;
        match user_id {
            Some(id) => self.session_user(UserId(id)).await,
            None => Ok(None),
        }
    }

    pub async fn delete_session(&self, token_hash: &[u8; 32]) -> Result<(), StoreError> {
        sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
            .bind(token_hash.as_slice())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn session_user(&self, user_id: UserId) -> Result<Option<SessionUser>, StoreError> {
        let user = sqlx::query_as::<_, UserRow>(
            "SELECT id, email, display_name, password_hash, status, created_at
             FROM users WHERE id = $1",
        )
        .bind(user_id.0)
        .fetch_optional(&self.pool)
        .await?
        .map(user_from_row)
        .transpose()?;
        let Some(user) = user else {
            return Ok(None);
        };
        let memberships = sqlx::query_as::<_, MembershipRow>(
            "SELECT m.organization_id, m.principal_id, m.role,
                    o.slug AS organization_slug, o.name AS organization_name
             FROM organization_memberships m
             JOIN organizations o ON o.id = m.organization_id
             WHERE m.user_id = $1
             ORDER BY o.name, o.id",
        )
        .bind(user_id.0)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(Membership::try_from)
        .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(SessionUser { user, memberships }))
    }

    pub async fn authorization_context(
        &self,
        user_id: UserId,
        organization_id: OrganizationId,
    ) -> Result<Option<AuthorizationContext>, StoreError> {
        let membership = sqlx::query_as::<_, MembershipAuthRow>(
            "SELECT principal_id, role
             FROM organization_memberships
             WHERE user_id = $1 AND organization_id = $2",
        )
        .bind(user_id.0)
        .bind(organization_id.0)
        .fetch_optional(&self.pool)
        .await?;
        let Some(membership) = membership else {
            return Ok(None);
        };
        let policies = sqlx::query_scalar::<_, Value>(
            "SELECT p.document
             FROM iam_bindings b
             JOIN iam_policies p ON p.id = b.policy_id
             WHERE b.organization_id = $1 AND b.principal_id = $2
             ORDER BY p.id",
        )
        .bind(organization_id.0)
        .bind(membership.principal_id)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(serde_json::from_value)
        .collect::<Result<Vec<PolicyDocument>, _>>()?;
        Ok(Some(AuthorizationContext {
            principal_id: PrincipalId(membership.principal_id),
            role: membership.role,
            policies,
        }))
    }

    pub async fn list_organizations(
        &self,
        user_id: UserId,
    ) -> Result<Vec<Organization>, StoreError> {
        sqlx::query_as::<_, OrganizationRow>(
            "SELECT o.id, o.slug, o.name, o.created_at
             FROM organizations o
             JOIN organization_memberships m ON m.organization_id = o.id
             WHERE m.user_id = $1
             ORDER BY o.name, o.id",
        )
        .bind(user_id.0)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(Organization::try_from)
        .collect()
    }

    pub async fn organization(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Option<Organization>, StoreError> {
        sqlx::query_as::<_, OrganizationRow>(
            "SELECT id, slug, name, created_at FROM organizations WHERE id = $1",
        )
        .bind(organization_id.0)
        .fetch_optional(&self.pool)
        .await?
        .map(Organization::try_from)
        .transpose()
    }

    pub async fn reserve_registry_credential(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        name: &str,
    ) -> Result<RegistryCredentialRecord, StoreError> {
        let mut transaction = self.pool.begin().await?;
        lock_tenant_allocations(&mut transaction, organization_id).await?;
        let quota = resource_quota_in_transaction(&mut transaction, organization_id).await?;
        let active = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM registry_credentials
             WHERE organization_id = $1 AND status IN ('provisioning', 'active')",
        )
        .bind(organization_id.0)
        .fetch_one(&mut *transaction)
        .await?;
        if active >= i64::from(quota.registry.max_credentials) {
            transaction.rollback().await?;
            return Err(StoreError::RequestRejected(format!(
                "registry credential limit exceeded: limit is {}",
                quota.registry.max_credentials
            )));
        }
        let record = sqlx::query_as::<_, RegistryCredentialRecord>(
            "INSERT INTO registry_credentials
                (id, organization_id, created_by, name, status)
             SELECT $1, $2, p.id, $4, 'provisioning'
             FROM principals p
             WHERE p.id = $3 AND p.organization_id = $2 AND p.enabled = true
             RETURNING id, name, username, harbor_robot_id, status, created_at",
        )
        .bind(Uuid::now_v7())
        .bind(organization_id.0)
        .bind(principal_id.0)
        .bind(name)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(StoreError::NotFound)?;
        transaction.commit().await?;
        Ok(record)
    }

    pub async fn activate_registry_credential(
        &self,
        organization_id: OrganizationId,
        credential_id: Uuid,
        harbor_robot_id: i64,
        username: &str,
    ) -> Result<RegistryCredentialRecord, StoreError> {
        sqlx::query_as::<_, RegistryCredentialRecord>(
            "UPDATE registry_credentials
             SET harbor_robot_id = $3, username = $4, status = 'active'
             WHERE id = $1 AND organization_id = $2 AND status = 'provisioning'
             RETURNING id, name, username, harbor_robot_id, status, created_at",
        )
        .bind(credential_id)
        .bind(organization_id.0)
        .bind(harbor_robot_id)
        .bind(username)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(StoreError::Conflict)
    }

    pub async fn cancel_registry_credential_reservation(
        &self,
        organization_id: OrganizationId,
        credential_id: Uuid,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "DELETE FROM registry_credentials
             WHERE id = $1 AND organization_id = $2 AND status = 'provisioning'",
        )
        .bind(credential_id)
        .bind(organization_id.0)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_registry_credentials(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<RegistryCredentialRecord>, StoreError> {
        sqlx::query_as::<_, RegistryCredentialRecord>(
            "SELECT id, name, username, harbor_robot_id, status, created_at
             FROM registry_credentials
             WHERE organization_id = $1 AND status = 'active'
             ORDER BY created_at DESC, id DESC
             LIMIT 100",
        )
        .bind(organization_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(StoreError::from)
    }

    pub async fn registry_credential_for_delete(
        &self,
        organization_id: OrganizationId,
        credential_id: Uuid,
    ) -> Result<RegistryCredentialRecord, StoreError> {
        sqlx::query_as::<_, RegistryCredentialRecord>(
            "SELECT id, name, username, harbor_robot_id, status, created_at
             FROM registry_credentials
             WHERE id = $1 AND organization_id = $2 AND status = 'active'",
        )
        .bind(credential_id)
        .bind(organization_id.0)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(StoreError::NotFound)
    }

    pub async fn delete_registry_credential(
        &self,
        organization_id: OrganizationId,
        credential_id: Uuid,
    ) -> Result<(), StoreError> {
        let deleted = sqlx::query(
            "DELETE FROM registry_credentials
             WHERE id = $1 AND organization_id = $2 AND status = 'active'",
        )
        .bind(credential_id)
        .bind(organization_id.0)
        .execute(&self.pool)
        .await?;
        if deleted.rows_affected() == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    pub async fn list_projects(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<Project>, StoreError> {
        sqlx::query_as::<_, ProjectRow>(
            "SELECT id, organization_id, slug, name, created_at
             FROM projects
             WHERE organization_id = $1
             ORDER BY name, id",
        )
        .bind(organization_id.0)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(Project::try_from)
        .collect()
    }

    pub async fn create_project(
        &self,
        organization_id: OrganizationId,
        slug: &str,
        name: &str,
    ) -> Result<Project, StoreError> {
        let row = sqlx::query_as::<_, ProjectRow>(
            "INSERT INTO projects (id, organization_id, slug, name)
             VALUES ($1, $2, $3, $4)
             RETURNING id, organization_id, slug, name, created_at",
        )
        .bind(ProjectId::new().0)
        .bind(organization_id.0)
        .bind(slug)
        .bind(name)
        .fetch_one(&self.pool)
        .await?;
        Project::try_from(row)
    }

    pub async fn list_principals(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<Principal>, StoreError> {
        sqlx::query_as::<_, PrincipalRow>(
            "SELECT id, organization_id, kind, name, user_id, enabled, created_at
             FROM principals WHERE organization_id = $1
             ORDER BY name, id",
        )
        .bind(organization_id.0)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(Principal::try_from)
        .collect()
    }

    pub async fn create_service_account(
        &self,
        organization_id: OrganizationId,
        name: &str,
    ) -> Result<Principal, StoreError> {
        let row = sqlx::query_as::<_, PrincipalRow>(
            "INSERT INTO principals
                (id, organization_id, kind, name, enabled)
             VALUES ($1, $2, 'service_account', $3, true)
             RETURNING id, organization_id, kind, name, user_id, enabled, created_at",
        )
        .bind(PrincipalId::new().0)
        .bind(organization_id.0)
        .bind(name)
        .fetch_one(&self.pool)
        .await?;
        Principal::try_from(row)
    }

    pub async fn create_api_key(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        name: &str,
        prefix: &str,
        secret_hash: &[u8; 32],
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<Uuid, StoreError> {
        let id = Uuid::now_v7();
        let result = sqlx::query(
            "INSERT INTO api_keys
                (id, organization_id, principal_id, name, prefix, secret_hash, expires_at)
             SELECT $1, $2, p.id, $4, $5, $6, $7
             FROM principals p
             WHERE p.id = $3 AND p.organization_id = $2
               AND p.kind = 'service_account' AND p.enabled = true",
        )
        .bind(id)
        .bind(organization_id.0)
        .bind(principal_id.0)
        .bind(name)
        .bind(prefix)
        .bind(secret_hash.as_slice())
        .bind(expires_at)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::NotFound);
        }
        Ok(id)
    }

    pub async fn list_api_keys(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<Vec<ApiKeyRecord>, StoreError> {
        sqlx::query_as::<_, ApiKeyRecord>(
            "SELECT id, organization_id, principal_id, name, prefix, expires_at,
                    last_used_at, revoked_at, created_at
             FROM api_keys
             WHERE organization_id = $1 AND principal_id = $2
             ORDER BY created_at DESC, id DESC",
        )
        .bind(organization_id.0)
        .bind(principal_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(StoreError::from)
    }

    pub async fn authenticate_api_key(
        &self,
        prefix: &str,
        secret_hash: &[u8; 32],
    ) -> Result<Option<ApiKeyPrincipal>, StoreError> {
        sqlx::query_as::<_, ApiKeyPrincipal>(
            "UPDATE api_keys k
             SET last_used_at = now()
             FROM principals p
             WHERE k.prefix = $1
               AND k.secret_hash = $2
               AND k.revoked_at IS NULL
               AND (k.expires_at IS NULL OR k.expires_at > now())
               AND p.id = k.principal_id
               AND p.organization_id = k.organization_id
               AND p.enabled = true
             RETURNING k.id AS api_key_id, k.organization_id, k.principal_id",
        )
        .bind(prefix)
        .bind(secret_hash.as_slice())
        .fetch_optional(&self.pool)
        .await
        .map_err(StoreError::from)
    }

    pub async fn authorization_context_for_principal(
        &self,
        principal_id: PrincipalId,
        organization_id: OrganizationId,
    ) -> Result<Option<AuthorizationContext>, StoreError> {
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(
                SELECT 1 FROM principals
                WHERE id = $1 AND organization_id = $2 AND enabled = true
             )",
        )
        .bind(principal_id.0)
        .bind(organization_id.0)
        .fetch_one(&self.pool)
        .await?;
        if !exists {
            return Ok(None);
        }
        let policies = sqlx::query_scalar::<_, Value>(
            "SELECT p.document
             FROM iam_bindings b
             JOIN iam_policies p ON p.id = b.policy_id
             WHERE b.organization_id = $1 AND b.principal_id = $2
             ORDER BY p.id",
        )
        .bind(organization_id.0)
        .bind(principal_id.0)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(serde_json::from_value)
        .collect::<Result<Vec<PolicyDocument>, _>>()?;
        Ok(Some(AuthorizationContext {
            principal_id,
            role: "service_account".into(),
            policies,
        }))
    }

    pub async fn list_policies(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<IamPolicy>, StoreError> {
        sqlx::query_as::<_, PolicyRow>(
            "SELECT id, organization_id, name, document, semantics_digest,
                    created_at, updated_at
             FROM iam_policies WHERE organization_id = $1
             ORDER BY name, id",
        )
        .bind(organization_id.0)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(IamPolicy::try_from)
        .collect()
    }

    pub async fn create_policy(
        &self,
        organization_id: OrganizationId,
        name: &str,
        document: &PolicyDocument,
        semantics_digest: &str,
    ) -> Result<IamPolicy, StoreError> {
        let row = sqlx::query_as::<_, PolicyRow>(
            "INSERT INTO iam_policies
                (id, organization_id, name, document, semantics_digest)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING id, organization_id, name, document, semantics_digest,
                       created_at, updated_at",
        )
        .bind(PolicyId::new().0)
        .bind(organization_id.0)
        .bind(name)
        .bind(serde_json::to_value(document)?)
        .bind(semantics_digest)
        .fetch_one(&self.pool)
        .await?;
        IamPolicy::try_from(row)
    }

    pub async fn create_binding(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        policy_id: PolicyId,
    ) -> Result<Uuid, StoreError> {
        let id = Uuid::now_v7();
        let result = sqlx::query(
            "INSERT INTO iam_bindings
                (id, organization_id, principal_id, policy_id)
             SELECT $1, $2, p.id, i.id
             FROM principals p, iam_policies i
             WHERE p.id = $3 AND p.organization_id = $2
               AND i.id = $4 AND i.organization_id = $2",
        )
        .bind(id)
        .bind(organization_id.0)
        .bind(principal_id.0)
        .bind(policy_id.0)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::NotFound);
        }
        Ok(id)
    }

    pub async fn list_service_instances(
        &self,
        organization_id: OrganizationId,
        project_id: Option<ProjectId>,
        provider: Option<&str>,
    ) -> Result<Vec<ServiceInstance>, StoreError> {
        sqlx::query_as::<_, ServiceRow>(
            "SELECT id, organization_id, project_id, provider, name, generation,
                    state, spec, status, created_at, updated_at
             FROM service_instances
             WHERE organization_id = $1
               AND ($2::uuid IS NULL OR project_id = $2)
               AND ($3::text IS NULL OR provider = $3)
             ORDER BY created_at DESC, id DESC",
        )
        .bind(organization_id.0)
        .bind(project_id.map(|id| id.0))
        .bind(provider)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(ServiceInstance::try_from)
        .collect()
    }

    pub async fn list_ready_flow_metric_targets(
        &self,
    ) -> Result<Vec<RealtimeMetricCollectionTarget>, StoreError> {
        let rows = sqlx::query_as::<_, RealtimeMetricCollectionTargetRow>(
            "SELECT id, organization_id, project_id
             FROM service_instances
             WHERE provider = 'flow' AND state = 'ready'
             ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(RealtimeMetricCollectionTarget::from)
            .collect())
    }

    pub async fn record_realtime_metric_sample(
        &self,
        service_instance_id: ServiceInstanceId,
        sampled_at: DateTime<Utc>,
        sample: &NewRealtimeMetricSample,
    ) -> Result<(), StoreError> {
        let result = sqlx::query(
            "INSERT INTO realtime_metric_samples
                (service_instance_id, sampled_at, measured_at, active_rooms,
                 concurrent_connections, sfu_participants, p2p_connections,
                 ingress_bytes, egress_bytes, transferred_bytes, turn_allocations,
                 room_limit)
             SELECT $1,
                    date_bin(INTERVAL '15 seconds', $2,
                             TIMESTAMPTZ '1970-01-01 00:00:00+00'),
                    $3, $4, $5, $6, $7, $8, $9, $10, $11, $12
             FROM service_instances
             WHERE id = $1 AND provider = 'flow'
             ON CONFLICT (service_instance_id, sampled_at) DO UPDATE
             SET measured_at = EXCLUDED.measured_at,
                 active_rooms = EXCLUDED.active_rooms,
                 concurrent_connections = EXCLUDED.concurrent_connections,
                 sfu_participants = EXCLUDED.sfu_participants,
                 p2p_connections = EXCLUDED.p2p_connections,
                 ingress_bytes = EXCLUDED.ingress_bytes,
                 egress_bytes = EXCLUDED.egress_bytes,
                 transferred_bytes = EXCLUDED.transferred_bytes,
                 turn_allocations = EXCLUDED.turn_allocations,
                 room_limit = EXCLUDED.room_limit
             WHERE EXCLUDED.measured_at >= realtime_metric_samples.measured_at",
        )
        .bind(service_instance_id.0)
        .bind(sampled_at)
        .bind(sample.measured_at)
        .bind(sample.active_rooms)
        .bind(sample.concurrent_connections)
        .bind(sample.sfu_participants)
        .bind(sample.p2p_connections)
        .bind(sample.ingress_bytes)
        .bind(sample.egress_bytes)
        .bind(sample.transferred_bytes)
        .bind(sample.turn_allocations)
        .bind(sample.room_limit)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS (
                     SELECT 1 FROM service_instances
                     WHERE id = $1 AND provider = 'flow'
                 )",
            )
            .bind(service_instance_id.0)
            .fetch_one(&self.pool)
            .await?;
            if !exists {
                return Err(StoreError::NotFound);
            }
        }
        Ok(())
    }

    pub async fn realtime_metric_history(
        &self,
        service_instance_id: ServiceInstanceId,
        since: DateTime<Utc>,
        bucket_seconds: i64,
    ) -> Result<Vec<RealtimeMetricHistorySample>, StoreError> {
        if bucket_seconds <= 0 {
            return Err(StoreError::Invariant(
                "metric history bucket must be positive",
            ));
        }
        sqlx::query_as::<_, RealtimeMetricHistorySample>(
            "WITH bucketed AS (
                 SELECT date_bin(
                            make_interval(secs => $3::double precision),
                            sampled_at,
                            TIMESTAMPTZ '1970-01-01 00:00:00+00'
                        ) AS bucket_at,
                        sampled_at AS recorded_at,
                        measured_at, active_rooms, concurrent_connections,
                        sfu_participants, p2p_connections, ingress_bytes,
                        egress_bytes, transferred_bytes, turn_allocations,
                        room_limit
                 FROM realtime_metric_samples
                 WHERE service_instance_id = $1 AND sampled_at >= $2
             ), latest AS (
                 SELECT DISTINCT ON (bucket_at)
                        bucket_at AS sampled_at, measured_at, active_rooms,
                        concurrent_connections, sfu_participants, p2p_connections,
                        ingress_bytes, egress_bytes, transferred_bytes,
                        turn_allocations, room_limit
                 FROM bucketed
                 ORDER BY bucket_at, recorded_at DESC
             ), bounded AS (
                 SELECT *
                 FROM latest
                 ORDER BY sampled_at DESC
                 LIMIT $4
             )
             SELECT sampled_at, measured_at, active_rooms,
                    concurrent_connections, sfu_participants, p2p_connections,
                    ingress_bytes, egress_bytes, transferred_bytes,
                    turn_allocations, room_limit
             FROM bounded
             ORDER BY sampled_at",
        )
        .bind(service_instance_id.0)
        .bind(since)
        .bind(bucket_seconds)
        .bind(MAX_REALTIME_METRIC_HISTORY_SAMPLES)
        .fetch_all(&self.pool)
        .await
        .map_err(StoreError::from)
    }

    pub async fn create_flow_developer_credential(
        &self,
        input: NewFlowDeveloperCredential<'_>,
    ) -> Result<FlowDeveloperCredentialRecord, StoreError> {
        if input.permissions.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(StoreError::Conflict);
        }
        let mut transaction = self.pool.begin().await?;
        lock_tenant_allocations(&mut transaction, input.organization_id).await?;
        let quota = resource_quota_in_transaction(&mut transaction, input.organization_id).await?;
        let service_exists = sqlx::query_scalar::<_, Uuid>(
            "SELECT id
             FROM service_instances
             WHERE id = $1 AND organization_id = $2 AND provider = 'flow'
             FOR UPDATE",
        )
        .bind(input.service_instance_id.0)
        .bind(input.organization_id.0)
        .fetch_optional(&mut *transaction)
        .await?
        .is_some();
        if !service_exists {
            return Err(StoreError::NotFound);
        }
        let active_count = sqlx::query_scalar::<_, i64>(
            "SELECT count(*)
             FROM flow_developer_credentials
             WHERE service_instance_id = $1
               AND revoked_at IS NULL
               AND expires_at > now()",
        )
        .bind(input.service_instance_id.0)
        .fetch_one(&mut *transaction)
        .await?;
        if active_count >= i64::from(quota.flow.max_developer_credentials_per_service) {
            transaction.rollback().await?;
            return Err(StoreError::RequestRejected(format!(
                "Flow developer credential limit exceeded: limit is {} per service",
                quota.flow.max_developer_credentials_per_service
            )));
        }
        let row = sqlx::query_as::<_, FlowDeveloperCredentialRecord>(
            "INSERT INTO flow_developer_credentials
                (id, organization_id, service_instance_id, created_by, name,
                 prefix, secret_hash, permissions, expires_at, created_at)
             SELECT $1, $2, $3, p.id, $5, $6, $7, $8, $9, $10
             FROM principals p
             WHERE p.id = $4 AND p.organization_id = $2 AND p.enabled = true
             RETURNING id, name, prefix, permissions, expires_at, last_used_at,
                       revoked_at, created_at",
        )
        .bind(Uuid::now_v7())
        .bind(input.organization_id.0)
        .bind(input.service_instance_id.0)
        .bind(input.created_by.0)
        .bind(input.name)
        .bind(input.prefix)
        .bind(input.secret_hash.as_slice())
        .bind(input.permissions.to_vec())
        .bind(input.expires_at)
        .bind(input.created_at)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(StoreError::NotFound)?;
        transaction.commit().await?;
        Ok(row)
    }

    pub async fn list_flow_developer_credentials(
        &self,
        organization_id: OrganizationId,
        service_instance_id: ServiceInstanceId,
        limit: i64,
    ) -> Result<Vec<FlowDeveloperCredentialRecord>, StoreError> {
        sqlx::query_as::<_, FlowDeveloperCredentialRecord>(
            "SELECT c.id, c.name, c.prefix, c.permissions, c.expires_at,
                    c.last_used_at, c.revoked_at, c.created_at
             FROM flow_developer_credentials c
             JOIN service_instances s ON s.id = c.service_instance_id
             WHERE c.organization_id = $1
               AND c.service_instance_id = $2
               AND s.organization_id = $1
               AND s.provider = 'flow'
             ORDER BY c.created_at DESC, c.id DESC
             LIMIT $3",
        )
        .bind(organization_id.0)
        .bind(service_instance_id.0)
        .bind(limit.clamp(1, MAX_FLOW_DEVELOPER_CREDENTIAL_LIST_SIZE))
        .fetch_all(&self.pool)
        .await
        .map_err(StoreError::from)
    }

    pub async fn flow_developer_credential(
        &self,
        organization_id: OrganizationId,
        service_instance_id: ServiceInstanceId,
        credential_id: Uuid,
    ) -> Result<Option<FlowDeveloperCredentialRecord>, StoreError> {
        sqlx::query_as::<_, FlowDeveloperCredentialRecord>(
            "SELECT id, name, prefix, permissions, expires_at, last_used_at,
                    revoked_at, created_at
             FROM flow_developer_credentials
             WHERE id = $1 AND organization_id = $2 AND service_instance_id = $3",
        )
        .bind(credential_id)
        .bind(organization_id.0)
        .bind(service_instance_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(StoreError::from)
    }

    pub async fn revoke_flow_developer_credential(
        &self,
        organization_id: OrganizationId,
        service_instance_id: ServiceInstanceId,
        credential_id: Uuid,
        principal_id: PrincipalId,
    ) -> Result<FlowDeveloperCredentialRevocation, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let scope = sqlx::query_as::<_, FlowDeveloperCredentialMutationScopeRow>(
            "SELECT c.id AS credential_id, c.revoked_at, c.organization_id,
                    c.service_instance_id, s.project_id, s.generation
             FROM flow_developer_credentials c
             JOIN service_instances s ON s.id = c.service_instance_id
             WHERE c.id = $1 AND c.organization_id = $2
               AND c.service_instance_id = $3 AND s.provider = 'flow'
             FOR UPDATE OF c",
        )
        .bind(credential_id)
        .bind(organization_id.0)
        .bind(service_instance_id.0)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(scope) = scope else {
            transaction.commit().await?;
            return Ok(FlowDeveloperCredentialRevocation {
                credential_revoked: false,
                contexts_revoked: 0,
            });
        };
        let credential_revoked = scope.revoked_at.is_none();
        if credential_revoked {
            sqlx::query("UPDATE flow_developer_credentials SET revoked_at = now() WHERE id = $1")
                .bind(credential_id)
                .execute(&mut *transaction)
                .await?;
        }
        let contexts_revoked =
            cascade_flow_developer_credential_contexts(&mut transaction, &scope, principal_id)
                .await?;
        transaction.commit().await?;
        Ok(FlowDeveloperCredentialRevocation {
            credential_revoked,
            contexts_revoked,
        })
    }

    pub async fn rotate_flow_developer_credential(
        &self,
        organization_id: OrganizationId,
        service_instance_id: ServiceInstanceId,
        credential_id: Uuid,
        prefix: &str,
        secret_hash: &[u8; 32],
        principal_id: PrincipalId,
    ) -> Result<FlowDeveloperCredentialRotation, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let scope = sqlx::query_as::<_, FlowDeveloperCredentialMutationScopeRow>(
            "SELECT c.id AS credential_id, c.revoked_at, c.organization_id,
                    c.service_instance_id, s.project_id, s.generation
             FROM flow_developer_credentials c
             JOIN service_instances s ON s.id = c.service_instance_id
             WHERE c.id = $1 AND c.organization_id = $2
               AND c.service_instance_id = $3 AND c.revoked_at IS NULL
               AND c.expires_at > now() AND s.provider = 'flow'
             FOR UPDATE OF c",
        )
        .bind(credential_id)
        .bind(organization_id.0)
        .bind(service_instance_id.0)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(StoreError::Conflict)?;
        let row = sqlx::query_as::<_, FlowDeveloperCredentialRecord>(
            "UPDATE flow_developer_credentials
             SET prefix = $4, secret_hash = $5, last_used_at = NULL
             WHERE id = $1 AND organization_id = $2 AND service_instance_id = $3
             RETURNING id, name, prefix, permissions, expires_at, last_used_at,
                       revoked_at, created_at",
        )
        .bind(credential_id)
        .bind(organization_id.0)
        .bind(service_instance_id.0)
        .bind(prefix)
        .bind(secret_hash.as_slice())
        .fetch_one(&mut *transaction)
        .await?;
        let contexts_revoked =
            cascade_flow_developer_credential_contexts(&mut transaction, &scope, principal_id)
                .await?;
        transaction.commit().await?;
        Ok(FlowDeveloperCredentialRotation {
            credential: row,
            contexts_revoked,
        })
    }

    pub async fn record_flow_access_context(
        &self,
        input: &NewFlowAccessContext<'_>,
    ) -> Result<(), StoreError> {
        let mut transaction = self.pool.begin().await?;
        lock_flow_access_context_retention(&mut transaction, input.service_instance_id).await?;
        let result = sqlx::query(
            "INSERT INTO flow_access_contexts
                (context_id, organization_id, project_id, service_instance_id,
                 credential_id, principal_id, permissions, issued_at, expires_at)
             SELECT $1, $2, $3, $4, $5, $6, $7, $8, $9
             FROM service_instances s
             WHERE s.id = $4 AND s.organization_id = $2 AND s.project_id = $3
               AND s.provider = 'flow' AND s.state = 'ready'",
        )
        .bind(input.context_id)
        .bind(input.organization_id.0)
        .bind(input.project_id.0)
        .bind(input.service_instance_id.0)
        .bind(input.credential_id)
        .bind(input.principal_id.0)
        .bind(input.permissions.to_vec())
        .bind(input.issued_at)
        .bind(input.expires_at)
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::Conflict);
        }
        prune_flow_access_context_history(&mut transaction, input.service_instance_id).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn mint_flow_access_context_with_developer_credential(
        &self,
        input: &DeveloperCredentialMint<'_>,
    ) -> Result<DeveloperCredentialMintOutcome, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let row = sqlx::query_as::<_, DeveloperCredentialMintRow>(
            "SELECT c.id AS credential_id, c.created_by, c.organization_id,
                    c.service_instance_id, c.permissions, s.project_id,
                    s.state AS service_state, s.spec AS service_spec
             FROM flow_developer_credentials c
             JOIN service_instances s ON s.id = c.service_instance_id
             WHERE c.prefix = $1 AND c.secret_hash = $2
               AND c.revoked_at IS NULL AND c.expires_at > now()
               AND s.provider = 'flow'
             FOR UPDATE OF c",
        )
        .bind(input.prefix)
        .bind(input.secret_hash.as_slice())
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(row) = row else {
            transaction.rollback().await?;
            return Ok(DeveloperCredentialMintOutcome::InvalidCredential);
        };
        if row.service_state != "ready" {
            transaction.rollback().await?;
            return Ok(DeveloperCredentialMintOutcome::ServiceInstanceNotReady);
        }
        if !input
            .permissions
            .iter()
            .all(|permission| row.permissions.contains(permission))
        {
            transaction.rollback().await?;
            return Ok(DeveloperCredentialMintOutcome::PermissionDenied);
        }
        let service_instance_id = ServiceInstanceId(row.service_instance_id);
        lock_flow_access_context_retention(&mut transaction, service_instance_id).await?;
        sqlx::query(
            "INSERT INTO flow_access_contexts
                (context_id, organization_id, project_id, service_instance_id,
                 credential_id, principal_id, permissions, issued_at, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(input.context_id)
        .bind(row.organization_id)
        .bind(row.project_id)
        .bind(row.service_instance_id)
        .bind(row.credential_id)
        .bind(input.principal_id.0)
        .bind(input.permissions.to_vec())
        .bind(input.issued_at)
        .bind(input.expires_at)
        .execute(&mut *transaction)
        .await?;
        prune_flow_access_context_history(&mut transaction, service_instance_id).await?;
        sqlx::query("UPDATE flow_developer_credentials SET last_used_at = now() WHERE id = $1")
            .bind(row.credential_id)
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "INSERT INTO audit_events
                (organization_id, principal_id, request_id, action, resource,
                 decision, reason, metadata)
             VALUES ($1, $2, $3, 'realtime:MintAccessCredential', $4,
                     'allow', 'developer_credential_scope', $5)",
        )
        .bind(row.organization_id)
        .bind(row.created_by)
        .bind(input.context_id.to_string())
        .bind(format!(
            "hc:org:{}:realtime/service/{}",
            row.organization_id, row.service_instance_id
        ))
        .bind(serde_json::json!({
            "authentication": {
                "actor": "flow_developer_credential",
                "credential_id": row.credential_id,
            },
            "context_id": input.context_id,
            "service_instance_id": ServiceInstanceId(row.service_instance_id),
            "issued_principal_id": input.principal_id,
            "permissions": input.permissions,
            "expires_at": input.expires_at,
        }))
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(DeveloperCredentialMintOutcome::Issued(
            DeveloperCredentialMintScope {
                credential_id: row.credential_id,
                organization_id: OrganizationId(row.organization_id),
                project_id: ProjectId(row.project_id),
                service_instance_id: ServiceInstanceId(row.service_instance_id),
                service_spec: row.service_spec,
            },
        ))
    }

    pub async fn list_flow_access_contexts(
        &self,
        organization_id: OrganizationId,
        service_instance_id: ServiceInstanceId,
        limit: i64,
    ) -> Result<Vec<FlowAccessContextRecord>, StoreError> {
        sqlx::query_as::<_, FlowAccessContextRecord>(
            "SELECT c.context_id, c.credential_id, c.principal_id, c.permissions,
                    c.issued_at, c.expires_at, c.revoked_at
             FROM flow_access_contexts c
             JOIN service_instances s ON s.id = c.service_instance_id
             WHERE c.organization_id = $1 AND c.service_instance_id = $2
               AND s.organization_id = $1 AND s.provider = 'flow'
             ORDER BY c.issued_at DESC, c.context_id DESC
             LIMIT $3",
        )
        .bind(organization_id.0)
        .bind(service_instance_id.0)
        .bind(limit.clamp(1, MAX_FLOW_ACCESS_CONTEXT_LIST_SIZE))
        .fetch_all(&self.pool)
        .await
        .map_err(StoreError::from)
    }

    pub async fn list_flow_access_contexts_for_developer_credential(
        &self,
        prefix: &str,
        secret_hash: &[u8; 32],
        limit: i64,
    ) -> Result<Option<Vec<FlowAccessContextRecord>>, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let credential_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT c.id
             FROM flow_developer_credentials c
             JOIN service_instances s ON s.id = c.service_instance_id
             WHERE c.prefix = $1 AND c.secret_hash = $2
               AND c.revoked_at IS NULL AND c.expires_at > now()
               AND s.provider = 'flow'
             FOR UPDATE OF c",
        )
        .bind(prefix)
        .bind(secret_hash.as_slice())
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(credential_id) = credential_id else {
            transaction.rollback().await?;
            return Ok(None);
        };
        let items = sqlx::query_as::<_, FlowAccessContextRecord>(
            "SELECT context_id, credential_id, principal_id, permissions,
                    issued_at, expires_at, revoked_at
             FROM flow_access_contexts
             WHERE credential_id = $1
             ORDER BY issued_at DESC, context_id DESC
             LIMIT $2",
        )
        .bind(credential_id)
        .bind(limit.clamp(1, MAX_FLOW_ACCESS_CONTEXT_LIST_SIZE))
        .fetch_all(&mut *transaction)
        .await?;
        sqlx::query("UPDATE flow_developer_credentials SET last_used_at = now() WHERE id = $1")
            .bind(credential_id)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(Some(items))
    }

    pub async fn revoke_flow_access_context_with_developer_credential(
        &self,
        prefix: &str,
        secret_hash: &[u8; 32],
        context_id: Uuid,
    ) -> Result<Option<bool>, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let credential = sqlx::query_as::<_, DeveloperCredentialContextScopeRow>(
            "SELECT c.id AS credential_id, c.created_by, c.organization_id,
                    c.service_instance_id, s.project_id, s.generation
             FROM flow_developer_credentials c
             JOIN service_instances s ON s.id = c.service_instance_id
             WHERE c.prefix = $1 AND c.secret_hash = $2
               AND c.revoked_at IS NULL AND c.expires_at > now()
               AND s.provider = 'flow'
             FOR UPDATE OF c",
        )
        .bind(prefix)
        .bind(secret_hash.as_slice())
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(credential) = credential else {
            transaction.rollback().await?;
            return Ok(None);
        };
        let context = sqlx::query_as::<_, DeveloperCredentialContextRevocationRow>(
            "SELECT principal_id, expires_at, revoked_at
             FROM flow_access_contexts
             WHERE context_id = $1 AND credential_id = $2
               AND organization_id = $3 AND service_instance_id = $4
             FOR UPDATE",
        )
        .bind(context_id)
        .bind(credential.credential_id)
        .bind(credential.organization_id)
        .bind(credential.service_instance_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let changed = context
            .as_ref()
            .is_some_and(|context| context.revoked_at.is_none());
        if let Some(context) = context
            .as_ref()
            .filter(|context| context.revoked_at.is_none())
        {
            sqlx::query("UPDATE flow_access_contexts SET revoked_at = now() WHERE context_id = $1")
                .bind(context_id)
                .execute(&mut *transaction)
                .await?;
            sqlx::query(
                "INSERT INTO outbox_events (id, topic, aggregate_id, payload)
                 VALUES ($1, 'principal-context.revoke', $2, $3)",
            )
            .bind(Uuid::now_v7())
            .bind(context_id)
            .bind(serde_json::json!({
                "context_id": context_id,
                "service_instance_id": ServiceInstanceId(credential.service_instance_id),
                "organization_id": OrganizationId(credential.organization_id),
                "project_id": ProjectId(credential.project_id),
                "principal_id": PrincipalId(credential.created_by),
                "provider": "flow",
                "generation": credential.generation,
                "expires_at": context.expires_at.timestamp(),
            }))
            .execute(&mut *transaction)
            .await?;
        }
        sqlx::query("UPDATE flow_developer_credentials SET last_used_at = now() WHERE id = $1")
            .bind(credential.credential_id)
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "INSERT INTO audit_events
                (organization_id, principal_id, request_id, action, resource,
                 decision, reason, metadata)
             VALUES ($1, $2, $3, 'realtime:RevokeAccessContext', $4,
                     'allow', 'developer_credential_scope', $5)",
        )
        .bind(credential.organization_id)
        .bind(credential.created_by)
        .bind(Uuid::now_v7().to_string())
        .bind(format!(
            "hc:org:{}:realtime/service/{}",
            credential.organization_id, credential.service_instance_id
        ))
        .bind(serde_json::json!({
            "authentication": {
                "actor": "flow_developer_credential",
                "credential_id": credential.credential_id,
            },
            "context_id": context_id,
            "service_instance_id": ServiceInstanceId(credential.service_instance_id),
            "context_principal_id": context
                .as_ref()
                .map(|context| PrincipalId(context.principal_id)),
            "revoked_now": changed,
        }))
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(Some(changed))
    }

    pub async fn revoke_flow_access_context(
        &self,
        organization_id: OrganizationId,
        service_instance_id: ServiceInstanceId,
        context_id: Uuid,
        principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let row = sqlx::query_as::<_, FlowAccessContextRevocationRow>(
            "SELECT c.expires_at, c.revoked_at, s.project_id, s.generation
             FROM flow_access_contexts c
             JOIN service_instances s ON s.id = c.service_instance_id
             WHERE c.context_id = $1 AND c.organization_id = $2
               AND c.service_instance_id = $3 AND s.provider = 'flow'
             FOR UPDATE OF c",
        )
        .bind(context_id)
        .bind(organization_id.0)
        .bind(service_instance_id.0)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(row) = row else {
            transaction.commit().await?;
            return Ok(false);
        };
        if row.revoked_at.is_some() {
            transaction.commit().await?;
            return Ok(false);
        }
        sqlx::query("UPDATE flow_access_contexts SET revoked_at = now() WHERE context_id = $1")
            .bind(context_id)
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "INSERT INTO outbox_events (id, topic, aggregate_id, payload)
             VALUES ($1, 'principal-context.revoke', $2, $3)",
        )
        .bind(Uuid::now_v7())
        .bind(context_id)
        .bind(serde_json::json!({
            "context_id": context_id,
            "service_instance_id": service_instance_id,
            "organization_id": organization_id,
            "project_id": ProjectId(row.project_id),
            "principal_id": principal_id,
            "provider": "flow",
            "generation": row.generation,
            "expires_at": row.expires_at.timestamp(),
        }))
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(true)
    }

    pub async fn create_service_instance(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        principal_id: PrincipalId,
        provider: &str,
        name: &str,
        spec: Value,
    ) -> Result<ServiceInstance, StoreError> {
        let id = ServiceInstanceId::new();
        let mut transaction = self.pool.begin().await?;
        let prepared = match provider {
            "flow" => {
                lock_tenant_allocations(&mut transaction, organization_id).await?;
                let quota =
                    resource_quota_in_transaction(&mut transaction, organization_id).await?;
                prepare_flow_spec(&mut transaction, organization_id, None, spec, &quota).await
            }
            "flash" => {
                lock_tenant_allocations(&mut transaction, organization_id).await?;
                lock_flash_allocations(&mut transaction).await?;
                let quota =
                    resource_quota_in_transaction(&mut transaction, organization_id).await?;
                prepare_flash_spec(&mut transaction, organization_id, None, None, spec, &quota)
                    .await
            }
            "syouyu" => {
                lock_tenant_allocations(&mut transaction, organization_id).await?;
                lock_syouyu_bucket_names(&mut transaction).await?;
                let quota =
                    resource_quota_in_transaction(&mut transaction, organization_id).await?;
                prepare_syouyu_spec(&mut transaction, organization_id, None, spec, &quota).await
            }
            _ => Ok(spec),
        };
        let spec = match prepared {
            Ok(spec) => spec,
            Err(error) => {
                transaction.rollback().await?;
                return Err(error);
            }
        };
        let row = sqlx::query_as::<_, ServiceRow>(
            "INSERT INTO service_instances
                (id, organization_id, project_id, provider, name, state, spec)
             SELECT $1, $2, p.id, $4, $5, 'provisioning', $6
             FROM projects p
             WHERE p.id = $3 AND p.organization_id = $2
             RETURNING id, organization_id, project_id, provider, name,
                       generation, state, spec, status, created_at, updated_at",
        )
        .bind(id.0)
        .bind(organization_id.0)
        .bind(project_id.0)
        .bind(provider)
        .bind(name)
        .bind(spec.clone())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(StoreError::NotFound)?;
        sqlx::query(
            "INSERT INTO outbox_events (id, topic, aggregate_id, payload)
             VALUES ($1, 'service-instance.reconcile', $2, $3)",
        )
        .bind(Uuid::now_v7())
        .bind(id.0)
        .bind(serde_json::json!({
            "service_instance_id": id,
            "organization_id": organization_id,
            "project_id": project_id,
            "principal_id": principal_id,
            "provider": provider,
            "generation": 1,
        }))
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        ServiceInstance::try_from(row)
    }

    pub async fn update_service_instance(
        &self,
        organization_id: OrganizationId,
        id: ServiceInstanceId,
        provider: &str,
        principal_id: PrincipalId,
        name: &str,
        spec: Value,
    ) -> Result<ServiceInstance, StoreError> {
        let mut transaction = self.pool.begin().await?;
        if matches!(provider, "flow" | "flash" | "syouyu") {
            lock_tenant_allocations(&mut transaction, organization_id).await?;
        }
        if provider == "flash" {
            lock_flash_allocations(&mut transaction).await?;
        }
        if provider == "syouyu" {
            lock_syouyu_bucket_names(&mut transaction).await?;
        }
        let existing = sqlx::query_as::<_, ServiceRow>(
            "SELECT id, organization_id, project_id, provider, name, generation,
                    state, spec, status, created_at, updated_at
             FROM service_instances
             WHERE id = $1 AND organization_id = $2 AND provider = $3
             FOR UPDATE",
        )
        .bind(id.0)
        .bind(organization_id.0)
        .bind(provider)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(existing) = existing else {
            transaction.rollback().await?;
            return Err(StoreError::NotFound);
        };
        if existing.state == "deleting" {
            transaction.rollback().await?;
            return Err(StoreError::Conflict);
        }
        let prepared = match provider {
            "flow" => {
                let quota =
                    resource_quota_in_transaction(&mut transaction, organization_id).await?;
                prepare_flow_spec(&mut transaction, organization_id, Some(id), spec, &quota).await
            }
            "flash" => {
                let quota =
                    resource_quota_in_transaction(&mut transaction, organization_id).await?;
                prepare_flash_spec(
                    &mut transaction,
                    organization_id,
                    Some(id),
                    Some(&existing.spec),
                    spec,
                    &quota,
                )
                .await
            }
            "syouyu" => {
                let quota =
                    resource_quota_in_transaction(&mut transaction, organization_id).await?;
                prepare_syouyu_spec(&mut transaction, organization_id, Some(id), spec, &quota).await
            }
            _ => Ok(spec),
        };
        let spec = match prepared {
            Ok(spec) => spec,
            Err(error) => {
                transaction.rollback().await?;
                return Err(error);
            }
        };
        let generation = existing
            .generation
            .checked_add(1)
            .ok_or(StoreError::Invariant("service generation overflow"))?;
        let row = sqlx::query_as::<_, ServiceRow>(
            "UPDATE service_instances
             SET name = $4, spec = $5, generation = $6, state = 'updating',
                 status = '{}'::jsonb, updated_at = now()
             WHERE id = $1 AND organization_id = $2 AND provider = $3
             RETURNING id, organization_id, project_id, provider, name, generation,
                       state, spec, status, created_at, updated_at",
        )
        .bind(id.0)
        .bind(organization_id.0)
        .bind(provider)
        .bind(name)
        .bind(spec)
        .bind(generation)
        .fetch_one(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO outbox_events (id, topic, aggregate_id, payload)
             VALUES ($1, 'service-instance.reconcile', $2, $3)",
        )
        .bind(Uuid::now_v7())
        .bind(id.0)
        .bind(serde_json::json!({
            "service_instance_id": id,
            "organization_id": organization_id,
            "project_id": ProjectId(existing.project_id),
            "principal_id": principal_id,
            "provider": provider,
            "generation": generation,
        }))
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        ServiceInstance::try_from(row)
    }

    pub async fn begin_delete_service_instance(
        &self,
        organization_id: OrganizationId,
        id: ServiceInstanceId,
        provider: &str,
        principal_id: PrincipalId,
    ) -> Result<ServiceInstance, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let existing = sqlx::query_as::<_, ServiceRow>(
            "SELECT id, organization_id, project_id, provider, name, generation,
                    state, spec, status, created_at, updated_at
             FROM service_instances
             WHERE id = $1 AND organization_id = $2 AND provider = $3
             FOR UPDATE",
        )
        .bind(id.0)
        .bind(organization_id.0)
        .bind(provider)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(existing) = existing else {
            transaction.rollback().await?;
            return Err(StoreError::NotFound);
        };
        if existing.state == "deleting" {
            transaction.commit().await?;
            return ServiceInstance::try_from(existing);
        }
        let generation = existing
            .generation
            .checked_add(1)
            .ok_or(StoreError::Invariant("service generation overflow"))?;
        let row = sqlx::query_as::<_, ServiceRow>(
            "UPDATE service_instances
             SET generation = $4, state = 'deleting', status = '{}'::jsonb,
                 updated_at = now()
             WHERE id = $1 AND organization_id = $2 AND provider = $3
             RETURNING id, organization_id, project_id, provider, name, generation,
                       state, spec, status, created_at, updated_at",
        )
        .bind(id.0)
        .bind(organization_id.0)
        .bind(provider)
        .bind(generation)
        .fetch_one(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO outbox_events (id, topic, aggregate_id, payload)
             VALUES ($1, 'service-instance.delete', $2, $3)",
        )
        .bind(Uuid::now_v7())
        .bind(id.0)
        .bind(serde_json::json!({
            "service_instance_id": id,
            "organization_id": organization_id,
            "project_id": ProjectId(existing.project_id),
            "principal_id": principal_id,
            "provider": provider,
            "generation": generation,
        }))
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        ServiceInstance::try_from(row)
    }

    pub async fn complete_delete_service_instance(
        &self,
        id: ServiceInstanceId,
        provider: &str,
        generation: i64,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            "DELETE FROM service_instances
             WHERE id = $1 AND generation = $2 AND provider = $3
               AND state = 'deleting'",
        )
        .bind(id.0)
        .bind(generation)
        .bind(provider)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn service_instance(
        &self,
        id: ServiceInstanceId,
    ) -> Result<Option<ServiceInstance>, StoreError> {
        sqlx::query_as::<_, ServiceRow>(
            "SELECT id, organization_id, project_id, provider, name, generation,
                    state, spec, status, created_at, updated_at
             FROM service_instances WHERE id = $1",
        )
        .bind(id.0)
        .fetch_optional(&self.pool)
        .await?
        .map(ServiceInstance::try_from)
        .transpose()
    }

    pub async fn mark_service_instance_ready(
        &self,
        id: ServiceInstanceId,
        provider: &str,
        generation: i64,
        operation_id: Uuid,
        provider_status: Value,
    ) -> Result<bool, StoreError> {
        let status = serde_json::json!({
            "operation_id": operation_id,
            "status": provider_status,
        });
        let result = sqlx::query(
            "UPDATE service_instances
             SET state = 'ready', status = $4, updated_at = now()
             WHERE id = $1 AND generation = $2 AND provider = $3",
        )
        .bind(id.0)
        .bind(generation)
        .bind(provider)
        .bind(status)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn mark_service_instance_error(
        &self,
        id: ServiceInstanceId,
        provider: &str,
        generation: i64,
        operation_id: Uuid,
        provider_status: Value,
    ) -> Result<bool, StoreError> {
        let status = serde_json::json!({
            "operation_id": operation_id,
            "status": provider_status,
        });
        let result = sqlx::query(
            "UPDATE service_instances
             SET state = 'error', status = $4, updated_at = now()
             WHERE id = $1 AND generation = $2 AND provider = $3",
        )
        .bind(id.0)
        .bind(generation)
        .bind(provider)
        .bind(status)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn claim_outbox_event(&self) -> Result<Option<OutboxEvent>, StoreError> {
        sqlx::query_as::<_, OutboxEvent>(
            "UPDATE outbox_events
             SET locked_at = now(), attempts = attempts + 1
             WHERE id = (
                 SELECT id
                 FROM outbox_events
                 WHERE delivered_at IS NULL
                   AND available_at <= now()
                   AND (locked_at IS NULL OR locked_at < now() - interval '2 minutes')
                 ORDER BY available_at, id
                 FOR UPDATE SKIP LOCKED
                 LIMIT 1
             )
             RETURNING id, topic, aggregate_id, payload, attempts",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(StoreError::from)
    }

    pub async fn mark_outbox_delivered(&self, id: Uuid) -> Result<(), StoreError> {
        sqlx::query(
            "UPDATE outbox_events
             SET delivered_at = now(), locked_at = NULL
             WHERE id = $1",
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn retry_outbox_event(&self, id: Uuid, attempts: i32) -> Result<(), StoreError> {
        let delay_seconds =
            i64::from(2_i32.saturating_pow(u32::try_from(attempts.clamp(1, 8)).unwrap_or(8)))
                .min(300);
        sqlx::query(
            "UPDATE outbox_events
             SET locked_at = NULL,
                 available_at = now() + make_interval(secs => $2)
             WHERE id = $1 AND delivered_at IS NULL",
        )
        .bind(id)
        .bind(delay_seconds as f64)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn append_audit(&self, event: &AuditEvent<'_>) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO audit_events
                (organization_id, principal_id, user_id, request_id, source_ip,
                 action, resource, decision, reason, metadata)
             VALUES ($1, $2, $3, $4, $5::inet, $6, $7, $8, $9, $10)",
        )
        .bind(event.organization_id.map(|id| id.0))
        .bind(event.principal_id.map(|id| id.0))
        .bind(event.user_id.map(|id| id.0))
        .bind(event.request_id)
        .bind(event.source_ip)
        .bind(event.action)
        .bind(event.resource)
        .bind(event.decision)
        .bind(event.reason)
        .bind(&event.metadata)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_audit_events(
        &self,
        organization_id: OrganizationId,
        limit: i64,
    ) -> Result<Vec<AuditEventRecord>, StoreError> {
        sqlx::query_as::<_, AuditEventRecord>(
            "SELECT id, occurred_at, organization_id, principal_id, user_id,
                    request_id, host(source_ip) AS source_ip, action, resource,
                    decision, reason, metadata
             FROM audit_events
             WHERE organization_id = $1
             ORDER BY occurred_at DESC, id DESC
             LIMIT $2",
        )
        .bind(organization_id.0)
        .bind(limit.clamp(1, 1000))
        .fetch_all(&self.pool)
        .await
        .map_err(StoreError::from)
    }
}

fn parse_resource_quota(value: Value) -> Result<ResourceQuotaLimits, StoreError> {
    let limits: ResourceQuotaLimits = serde_json::from_value(value)?;
    validate_resource_quota(&limits)?;
    Ok(limits)
}

fn validate_resource_quota(limits: &ResourceQuotaLimits) -> Result<(), StoreError> {
    limits
        .validate()
        .map_err(|error| StoreError::RequestRejected(error.to_string()))
}

async fn lock_tenant_allocations(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: OrganizationId,
) -> Result<(), StoreError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::uuid::text, 1))")
        .bind(organization_id.0)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn resource_quota_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: OrganizationId,
) -> Result<ResourceQuotaLimits, StoreError> {
    let limits = sqlx::query_scalar::<_, Value>(
        "SELECT COALESCE(q.limits, d.limits)
         FROM organizations o
         CROSS JOIN resource_quota_defaults d
         LEFT JOIN organization_resource_quotas q ON q.organization_id = o.id
         WHERE o.id = $1 AND d.singleton = true",
    )
    .bind(organization_id.0)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(StoreError::NotFound)?;
    parse_resource_quota(limits)
}

async fn prepare_flow_spec(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: OrganizationId,
    excluded_service_id: Option<ServiceInstanceId>,
    spec: Value,
    quota: &ResourceQuotaLimits,
) -> Result<Value, StoreError> {
    let requested: FlowSpec = serde_json::from_value(spec)?;
    if requested.max_rooms > quota.flow.max_rooms_per_service {
        return Err(StoreError::RequestRejected(format!(
            "Flow room limit exceeded: {} requested, limit is {} per service",
            requested.max_rooms, quota.flow.max_rooms_per_service
        )));
    }
    if requested.max_participants > quota.flow.max_participants_per_service {
        return Err(StoreError::RequestRejected(format!(
            "Flow participant limit exceeded: {} requested, limit is {} per service",
            requested.max_participants, quota.flow.max_participants_per_service
        )));
    }
    if requested.rate_limit.requests_per_second > quota.flow.max_rate_limit_requests_per_second
        || requested.rate_limit.burst > quota.flow.max_rate_limit_burst
    {
        return Err(StoreError::RequestRejected(format!(
            "Flow API rate limit exceeds the tenant ceiling of {} RPS and {} burst",
            quota.flow.max_rate_limit_requests_per_second, quota.flow.max_rate_limit_burst
        )));
    }
    let rows = sqlx::query_as::<_, FlowAllocationRow>(
        "SELECT id, spec
         FROM service_instances
         WHERE organization_id = $1 AND provider = 'flow' AND state <> 'deleting'",
    )
    .bind(organization_id.0)
    .fetch_all(&mut **transaction)
    .await?;
    let mut services = 1_u64;
    let mut rooms = u64::from(requested.max_rooms);
    for row in rows {
        if excluded_service_id.is_some_and(|id| id.0 == row.id) {
            continue;
        }
        let stored: FlowSpec = serde_json::from_value(row.spec)?;
        services += 1;
        rooms = rooms.saturating_add(u64::from(stored.max_rooms));
    }
    if services > u64::from(quota.flow.max_services) {
        return Err(StoreError::RequestRejected(format!(
            "Flow service limit exceeded: {services} requested, limit is {}",
            quota.flow.max_services
        )));
    }
    if rooms > quota.flow.max_total_rooms {
        return Err(StoreError::RequestRejected(format!(
            "Flow tenant room limit exceeded: {rooms} requested, limit is {}",
            quota.flow.max_total_rooms
        )));
    }
    Ok(serde_json::to_value(requested)?)
}

async fn lock_syouyu_bucket_names(
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<(), StoreError> {
    sqlx::query(
        "SELECT pg_advisory_xact_lock(hashtextextended('heterocloud-syouyu-bucket-names', 0))",
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn prepare_syouyu_spec(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: OrganizationId,
    excluded_service_id: Option<ServiceInstanceId>,
    spec: Value,
    quota: &ResourceQuotaLimits,
) -> Result<Value, StoreError> {
    let requested: SyouyuSpec = serde_json::from_value(spec)?;
    requested
        .validate()
        .map_err(|error| StoreError::RequestRejected(error.to_string()))?;
    if requested.quota_bytes > quota.syouyu.max_bytes_per_bucket {
        return Err(StoreError::RequestRejected(format!(
            "Syouyu bucket byte quota exceeded: {} requested, limit is {}",
            requested.quota_bytes, quota.syouyu.max_bytes_per_bucket
        )));
    }
    if requested.quota_objects > quota.syouyu.max_objects_per_bucket {
        return Err(StoreError::RequestRejected(format!(
            "Syouyu bucket object quota exceeded: {} requested, limit is {}",
            requested.quota_objects, quota.syouyu.max_objects_per_bucket
        )));
    }

    let name_owner = sqlx::query_scalar::<_, Uuid>(
        "SELECT id
         FROM service_instances
         WHERE provider = 'syouyu' AND spec->>'bucket_name' = $1
         LIMIT 1",
    )
    .bind(&requested.bucket_name)
    .fetch_optional(&mut **transaction)
    .await?;
    if name_owner.is_some_and(|id| excluded_service_id.is_none_or(|excluded| excluded.0 != id)) {
        return Err(StoreError::Conflict);
    }

    let rows = sqlx::query_as::<_, SyouyuAllocationRow>(
        "SELECT id, spec
         FROM service_instances
         WHERE organization_id = $1 AND provider = 'syouyu' AND state <> 'deleting'",
    )
    .bind(organization_id.0)
    .fetch_all(&mut **transaction)
    .await?;
    let mut buckets = 1_u64;
    let mut bytes = requested.quota_bytes;
    for row in rows {
        if excluded_service_id.is_some_and(|id| id.0 == row.id) {
            let stored: SyouyuSpec = serde_json::from_value(row.spec)?;
            if requested.bucket_name != stored.bucket_name || requested.region != stored.region {
                return Err(StoreError::RequestRejected(
                    "Syouyu bucket_name and region cannot be changed after creation".into(),
                ));
            }
            continue;
        }
        let stored: SyouyuSpec = serde_json::from_value(row.spec)?;
        buckets = buckets.saturating_add(1);
        bytes = bytes.saturating_add(stored.quota_bytes);
    }
    if buckets > u64::from(quota.syouyu.max_buckets) {
        return Err(StoreError::RequestRejected(format!(
            "Syouyu bucket limit exceeded: {buckets} requested, limit is {}",
            quota.syouyu.max_buckets
        )));
    }
    if bytes > quota.syouyu.max_total_bytes {
        return Err(StoreError::RequestRejected(format!(
            "Syouyu tenant byte quota exceeded: {bytes} requested, limit is {}",
            quota.syouyu.max_total_bytes
        )));
    }
    Ok(serde_json::to_value(requested)?)
}

async fn lock_flash_allocations(
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<(), StoreError> {
    sqlx::query(
        "SELECT pg_advisory_xact_lock(hashtextextended('heterocloud-flash-allocation', 0))",
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn prepare_flash_spec(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: OrganizationId,
    excluded_service_id: Option<ServiceInstanceId>,
    existing_spec: Option<&Value>,
    spec: Value,
    quota: &ResourceQuotaLimits,
) -> Result<Value, StoreError> {
    let mut requested: FlashSpec = serde_json::from_value(spec)?;
    requested
        .validate_request()
        .map_err(|error| StoreError::RequestRejected(error.to_string()))?;
    if requested.replicas > quota.flash.max_replicas_per_service {
        return Err(StoreError::RequestRejected(format!(
            "Flash replica limit exceeded: {} requested, limit is {} per service",
            requested.replicas, quota.flash.max_replicas_per_service
        )));
    }
    if requested.cpu_millis > quota.flash.max_cpu_millis_per_vm
        || requested.memory_mib > quota.flash.max_memory_mib_per_vm
        || requested.ephemeral_storage_gib > quota.flash.max_disk_gib_per_vm
    {
        return Err(StoreError::RequestRejected(format!(
            "Flash VM resources exceed the tenant ceiling of {} millicores, {} MiB memory, and {} GiB disk",
            quota.flash.max_cpu_millis_per_vm,
            quota.flash.max_memory_mib_per_vm,
            quota.flash.max_disk_gib_per_vm
        )));
    }

    let rows = sqlx::query_as::<_, FlashAllocationRow>(
        "SELECT id, organization_id, spec
         FROM service_instances
         WHERE provider = 'flash' AND state <> 'deleting'",
    )
    .fetch_all(&mut **transaction)
    .await?;
    let mut occupied_ports = BTreeSet::new();
    let mut organization_cpu_millis = 0_u64;
    let mut organization_memory_mib = 0_u64;
    let mut organization_ephemeral_storage_gib = 0_u64;
    let mut organization_replicas = 0_u64;
    let mut organization_services = 1_u64;
    for row in &rows {
        if excluded_service_id.is_some_and(|id| id.0 == row.id) {
            continue;
        }
        let stored: FlashSpec = serde_json::from_value(row.spec.clone())?;
        occupied_ports.extend(
            stored
                .ports
                .iter()
                .map(|port| (port.protocol, port.service_port)),
        );
        if row.organization_id == organization_id.0 {
            organization_services += 1;
            organization_replicas += u64::from(stored.replicas);
            organization_cpu_millis += u64::from(stored.replicas) * u64::from(stored.cpu_millis);
            organization_memory_mib += u64::from(stored.replicas) * u64::from(stored.memory_mib);
            organization_ephemeral_storage_gib +=
                u64::from(stored.replicas) * u64::from(stored.ephemeral_storage_gib);
        }
    }

    let existing_ports = existing_spec
        .map(|value| serde_json::from_value::<FlashSpec>(value.clone()))
        .transpose()?
        .map(|stored| {
            stored
                .ports
                .into_iter()
                .map(|port| ((port.protocol, port.name), port.service_port))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    for port in &mut requested.ports {
        let preserved = existing_ports
            .get(&(port.protocol, port.name.clone()))
            .copied()
            .filter(|value| (MIN_FLASH_SERVICE_PORT..=MAX_FLASH_SERVICE_PORT).contains(value))
            .filter(|value| !occupied_ports.contains(&(port.protocol, *value)));
        let assigned = preserved.or_else(|| {
            (MIN_FLASH_SERVICE_PORT..=MAX_FLASH_SERVICE_PORT)
                .find(|value| !occupied_ports.contains(&(port.protocol, *value)))
        });
        let Some(assigned) = assigned else {
            return Err(StoreError::RequestRejected(format!(
                "no free {} service ports remain in {MIN_FLASH_SERVICE_PORT}..={MAX_FLASH_SERVICE_PORT}",
                flash_protocol_name(port.protocol)
            )));
        };
        port.service_port = assigned;
        occupied_ports.insert((port.protocol, assigned));
    }

    organization_cpu_millis += u64::from(requested.replicas) * u64::from(requested.cpu_millis);
    organization_memory_mib += u64::from(requested.replicas) * u64::from(requested.memory_mib);
    organization_replicas += u64::from(requested.replicas);
    organization_ephemeral_storage_gib +=
        u64::from(requested.replicas) * u64::from(requested.ephemeral_storage_gib);
    if organization_services > u64::from(quota.flash.max_services) {
        return Err(StoreError::RequestRejected(format!(
            "Flash service limit exceeded: {organization_services} requested, limit is {}",
            quota.flash.max_services
        )));
    }
    if organization_replicas > quota.flash.max_total_replicas {
        return Err(StoreError::RequestRejected(format!(
            "Flash tenant replica limit exceeded: {organization_replicas} requested, limit is {}",
            quota.flash.max_total_replicas
        )));
    }
    if organization_cpu_millis > quota.flash.max_total_cpu_millis {
        return Err(StoreError::RequestRejected(format!(
            "Flash tenant CPU limit exceeded: {organization_cpu_millis} millicores requested, limit is {}",
            quota.flash.max_total_cpu_millis
        )));
    }
    if organization_memory_mib > quota.flash.max_total_memory_mib {
        return Err(StoreError::RequestRejected(format!(
            "Flash tenant memory limit exceeded: {organization_memory_mib} MiB requested, limit is {} MiB",
            quota.flash.max_total_memory_mib
        )));
    }
    if organization_ephemeral_storage_gib > quota.flash.max_total_disk_gib {
        return Err(StoreError::RequestRejected(format!(
            "Flash tenant disk limit exceeded: {organization_ephemeral_storage_gib} GiB requested, limit is {} GiB",
            quota.flash.max_total_disk_gib
        )));
    }
    requested
        .validate()
        .map_err(|error| StoreError::RequestRejected(error.to_string()))?;
    Ok(serde_json::to_value(requested)?)
}

const fn flash_protocol_name(protocol: FlashProtocol) -> &'static str {
    match protocol {
        FlashProtocol::Tcp => "TCP",
        FlashProtocol::Udp => "UDP",
    }
}

async fn lock_flow_access_context_retention(
    transaction: &mut Transaction<'_, Postgres>,
    service_instance_id: ServiceInstanceId,
) -> Result<(), StoreError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::uuid::text, 0))")
        .bind(service_instance_id.0)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn prune_flow_access_context_history(
    transaction: &mut Transaction<'_, Postgres>,
    service_instance_id: ServiceInstanceId,
) -> Result<(), StoreError> {
    sqlx::query(
        "DELETE FROM flow_access_contexts
         WHERE service_instance_id = $1
           AND context_id IN (
               SELECT context_id
               FROM flow_access_contexts
               WHERE service_instance_id = $1
               ORDER BY issued_at DESC, context_id DESC
               OFFSET $2
           )
           AND (revoked_at IS NOT NULL OR expires_at <= now())",
    )
    .bind(service_instance_id.0)
    .bind(MAX_FLOW_ACCESS_CONTEXT_RECORDS_PER_SERVICE)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn cascade_flow_developer_credential_contexts(
    transaction: &mut Transaction<'_, Postgres>,
    scope: &FlowDeveloperCredentialMutationScopeRow,
    principal_id: PrincipalId,
) -> Result<u64, StoreError> {
    let contexts = sqlx::query_as::<_, FlowCredentialActiveContextRow>(
        "SELECT context_id, expires_at
         FROM flow_access_contexts
         WHERE credential_id = $1 AND organization_id = $2
           AND service_instance_id = $3 AND revoked_at IS NULL
           AND expires_at > now()
         ORDER BY context_id
         FOR UPDATE",
    )
    .bind(scope.credential_id)
    .bind(scope.organization_id)
    .bind(scope.service_instance_id)
    .fetch_all(&mut **transaction)
    .await?;
    if contexts.is_empty() {
        return Ok(0);
    }
    let context_ids = contexts
        .iter()
        .map(|context| context.context_id)
        .collect::<Vec<_>>();
    let context_count = u64::try_from(contexts.len())
        .map_err(|_| StoreError::Invariant("developer credential cascade count overflow"))?;
    let updated = sqlx::query(
        "UPDATE flow_access_contexts
         SET revoked_at = now()
         WHERE context_id = ANY($1) AND revoked_at IS NULL",
    )
    .bind(&context_ids)
    .execute(&mut **transaction)
    .await?;
    if updated.rows_affected() != context_count {
        return Err(StoreError::Invariant(
            "developer credential context cascade lost a locked row",
        ));
    }
    for context in &contexts {
        sqlx::query(
            "INSERT INTO outbox_events (id, topic, aggregate_id, payload)
             VALUES ($1, 'principal-context.revoke', $2, $3)",
        )
        .bind(Uuid::now_v7())
        .bind(context.context_id)
        .bind(serde_json::json!({
            "context_id": context.context_id,
            "service_instance_id": ServiceInstanceId(scope.service_instance_id),
            "organization_id": OrganizationId(scope.organization_id),
            "project_id": ProjectId(scope.project_id),
            "principal_id": principal_id,
            "provider": "flow",
            "generation": scope.generation,
            "expires_at": context.expires_at.timestamp(),
        }))
        .execute(&mut **transaction)
        .await?;
    }
    Ok(context_count)
}

async fn lookup_user_by_email(
    transaction: &mut Transaction<'_, Postgres>,
    email: &str,
) -> Result<Option<User>, StoreError> {
    sqlx::query_as::<_, UserRow>(
        "SELECT id, email, display_name, password_hash, status, created_at
         FROM users WHERE lower(email) = lower($1)",
    )
    .bind(email)
    .fetch_optional(&mut **transaction)
    .await?
    .map(user_from_row)
    .transpose()
}

pub struct BootstrapAdmin<'a> {
    pub email: &'a str,
    pub display_name: &'a str,
    pub password_hash: &'a str,
    pub organization_slug: &'a str,
    pub organization_name: &'a str,
}

pub struct RegisterWithInvitation<'a> {
    pub code_hash: &'a [u8; 32],
    pub email: &'a str,
    pub display_name: &'a str,
    pub password_hash: &'a str,
}

pub struct OidcUser<'a> {
    pub issuer: &'a str,
    pub subject: &'a str,
    pub email: &'a str,
    pub display_name: &'a str,
}

#[derive(Clone, Debug)]
pub struct PasswordUser {
    pub user: User,
    pub password_hash: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Membership {
    pub organization_id: OrganizationId,
    pub organization_slug: String,
    pub organization_name: String,
    pub principal_id: PrincipalId,
    pub role: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct SessionUser {
    pub user: User,
    pub memberships: Vec<Membership>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UserExternalIdentityRecord {
    pub issuer: String,
    pub subject: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct UserLoginEventRecord {
    pub id: i64,
    pub user_id: Uuid,
    pub source_ip: Option<String>,
    pub authentication_method: String,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize)]
pub struct OwnerAccountRecord {
    pub user: User,
    pub has_local_password: bool,
    pub external_identities: Vec<UserExternalIdentityRecord>,
    pub memberships: Vec<Membership>,
    pub last_login: Option<UserLoginEventRecord>,
    pub login_count: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ResourceQuotaUsage {
    pub flow_services: u64,
    pub flow_max_rooms_per_service: u64,
    pub flow_configured_rooms: u64,
    pub flow_max_participants_per_service: u64,
    pub flow_max_rate_limit_requests_per_second: u64,
    pub flow_max_rate_limit_burst: u64,
    pub flow_developer_credentials: u64,
    pub flow_max_developer_credentials_per_service: u64,
    pub flash_services: u64,
    pub flash_max_replicas_per_service: u64,
    pub flash_max_cpu_millis_per_vm: u64,
    pub flash_max_memory_mib_per_vm: u64,
    pub flash_max_disk_gib_per_vm: u64,
    pub flash_replicas: u64,
    pub flash_cpu_millis: u64,
    pub flash_memory_mib: u64,
    pub flash_disk_gib: u64,
    pub registry_storage_bytes: Option<u64>,
    pub registry_credentials: u64,
    pub syouyu_buckets: u64,
    pub syouyu_max_bytes_per_bucket: u64,
    pub syouyu_max_objects_per_bucket: u64,
    pub syouyu_configured_bytes: u64,
    pub syouyu_storage_bytes: Option<u64>,
    pub syouyu_credentials: u64,
}

impl ResourceQuotaUsage {
    fn add_flow_spec(&mut self, spec: &FlowSpec) {
        self.flow_services = self.flow_services.saturating_add(1);
        self.flow_max_rooms_per_service = self
            .flow_max_rooms_per_service
            .max(u64::from(spec.max_rooms));
        self.flow_configured_rooms = self
            .flow_configured_rooms
            .saturating_add(u64::from(spec.max_rooms));
        self.flow_max_participants_per_service = self
            .flow_max_participants_per_service
            .max(u64::from(spec.max_participants));
        self.flow_max_rate_limit_requests_per_second = self
            .flow_max_rate_limit_requests_per_second
            .max(u64::from(spec.rate_limit.requests_per_second));
        self.flow_max_rate_limit_burst = self
            .flow_max_rate_limit_burst
            .max(u64::from(spec.rate_limit.burst));
    }

    fn add_flash_spec(&mut self, spec: &FlashSpec) {
        let replicas = u64::from(spec.replicas);
        self.flash_services = self.flash_services.saturating_add(1);
        self.flash_max_replicas_per_service = self.flash_max_replicas_per_service.max(replicas);
        self.flash_max_cpu_millis_per_vm = self
            .flash_max_cpu_millis_per_vm
            .max(u64::from(spec.cpu_millis));
        self.flash_max_memory_mib_per_vm = self
            .flash_max_memory_mib_per_vm
            .max(u64::from(spec.memory_mib));
        self.flash_max_disk_gib_per_vm = self
            .flash_max_disk_gib_per_vm
            .max(u64::from(spec.ephemeral_storage_gib));
        self.flash_replicas = self.flash_replicas.saturating_add(replicas);
        self.flash_cpu_millis = self
            .flash_cpu_millis
            .saturating_add(replicas.saturating_mul(u64::from(spec.cpu_millis)));
        self.flash_memory_mib = self
            .flash_memory_mib
            .saturating_add(replicas.saturating_mul(u64::from(spec.memory_mib)));
        self.flash_disk_gib = self
            .flash_disk_gib
            .saturating_add(replicas.saturating_mul(u64::from(spec.ephemeral_storage_gib)));
    }

    fn add_syouyu_spec(&mut self, spec: &SyouyuSpec) {
        self.syouyu_buckets = self.syouyu_buckets.saturating_add(1);
        self.syouyu_max_bytes_per_bucket = self.syouyu_max_bytes_per_bucket.max(spec.quota_bytes);
        self.syouyu_max_objects_per_bucket =
            self.syouyu_max_objects_per_bucket.max(spec.quota_objects);
        self.syouyu_configured_bytes = self
            .syouyu_configured_bytes
            .saturating_add(spec.quota_bytes);
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ResourceQuotaTenant {
    pub organization: Organization,
    pub override_limits: Option<ResourceQuotaLimits>,
    pub effective_limits: ResourceQuotaLimits,
    pub usage: ResourceQuotaUsage,
}

#[derive(Clone, Debug, Deserialize, Serialize, sqlx::FromRow)]
pub struct RegistryCredentialRecord {
    pub id: Uuid,
    pub name: String,
    pub username: Option<String>,
    #[serde(skip_serializing)]
    pub harbor_robot_id: Option<i64>,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct AuthorizationContext {
    pub principal_id: PrincipalId,
    pub role: String,
    pub policies: Vec<PolicyDocument>,
}

#[derive(Clone, Debug)]
pub struct RealtimeMetricCollectionTarget {
    pub service_instance_id: ServiceInstanceId,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
}

#[derive(Clone, Debug)]
pub struct NewRealtimeMetricSample {
    pub measured_at: DateTime<Utc>,
    pub active_rooms: i64,
    pub concurrent_connections: i64,
    pub sfu_participants: i64,
    pub p2p_connections: i64,
    pub ingress_bytes: i64,
    pub egress_bytes: i64,
    pub transferred_bytes: i64,
    pub turn_allocations: Option<i64>,
    pub room_limit: Option<i64>,
}

pub struct NewFlowDeveloperCredential<'a> {
    pub organization_id: OrganizationId,
    pub service_instance_id: ServiceInstanceId,
    pub created_by: PrincipalId,
    pub name: &'a str,
    pub prefix: &'a str,
    pub secret_hash: &'a [u8; 32],
    pub permissions: &'a [String],
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, sqlx::FromRow)]
pub struct FlowDeveloperCredentialRecord {
    pub id: Uuid,
    pub name: String,
    pub prefix: String,
    pub permissions: Vec<String>,
    pub expires_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FlowDeveloperCredentialRevocation {
    pub credential_revoked: bool,
    pub contexts_revoked: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FlowDeveloperCredentialRotation {
    pub credential: FlowDeveloperCredentialRecord,
    pub contexts_revoked: u64,
}

pub struct NewFlowAccessContext<'a> {
    pub context_id: Uuid,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub service_instance_id: ServiceInstanceId,
    pub credential_id: Option<Uuid>,
    pub principal_id: PrincipalId,
    pub permissions: &'a [String],
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

pub struct DeveloperCredentialMint<'a> {
    pub prefix: &'a str,
    pub secret_hash: &'a [u8; 32],
    pub context_id: Uuid,
    pub principal_id: PrincipalId,
    pub permissions: &'a [String],
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub enum DeveloperCredentialMintOutcome {
    Issued(DeveloperCredentialMintScope),
    InvalidCredential,
    PermissionDenied,
    ServiceInstanceNotReady,
}

#[derive(Clone, Debug)]
pub struct DeveloperCredentialMintScope {
    pub credential_id: Uuid,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub service_instance_id: ServiceInstanceId,
    pub service_spec: Value,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, sqlx::FromRow)]
pub struct FlowAccessContextRecord {
    pub context_id: Uuid,
    pub credential_id: Option<Uuid>,
    pub principal_id: Uuid,
    pub permissions: Vec<String>,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, sqlx::FromRow)]
pub struct RealtimeMetricHistorySample {
    pub sampled_at: DateTime<Utc>,
    pub measured_at: DateTime<Utc>,
    pub active_rooms: i64,
    pub concurrent_connections: i64,
    pub sfu_participants: i64,
    pub p2p_connections: i64,
    pub ingress_bytes: i64,
    pub egress_bytes: i64,
    pub transferred_bytes: i64,
    pub turn_allocations: Option<i64>,
    pub room_limit: Option<i64>,
}

pub struct AuditEvent<'a> {
    pub organization_id: Option<OrganizationId>,
    pub principal_id: Option<PrincipalId>,
    pub user_id: Option<UserId>,
    pub request_id: &'a str,
    pub source_ip: Option<&'a str>,
    pub action: &'a str,
    pub resource: &'a str,
    pub decision: &'a str,
    pub reason: &'a str,
    pub metadata: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize, sqlx::FromRow)]
pub struct OutboxEvent {
    pub id: Uuid,
    pub topic: String,
    pub aggregate_id: Uuid,
    pub payload: Value,
    pub attempts: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize, sqlx::FromRow)]
pub struct ApiKeyRecord {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub principal_id: Uuid,
    pub name: String,
    pub prefix: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct ApiKeyPrincipal {
    pub api_key_id: Uuid,
    pub organization_id: Uuid,
    pub principal_id: Uuid,
}

#[derive(Clone, Debug, Deserialize, Serialize, sqlx::FromRow)]
pub struct AuditEventRecord {
    pub id: i64,
    pub occurred_at: DateTime<Utc>,
    pub organization_id: Option<Uuid>,
    pub principal_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
    pub request_id: String,
    pub source_ip: Option<String>,
    pub action: String,
    pub resource: String,
    pub decision: String,
    pub reason: String,
    pub metadata: Value,
}

#[derive(sqlx::FromRow)]
struct UserRow {
    id: Uuid,
    email: String,
    display_name: String,
    password_hash: Option<String>,
    status: String,
    created_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct OwnerAccountRow {
    id: Uuid,
    email: String,
    display_name: String,
    status: String,
    created_at: DateTime<Utc>,
    has_local_password: bool,
    external_identities: Value,
    memberships: Value,
    last_login_id: Option<i64>,
    last_login_ip: Option<String>,
    last_login_authentication_method: Option<String>,
    last_login_occurred_at: Option<DateTime<Utc>>,
    login_count: i64,
}

impl TryFrom<UserRow> for PasswordUser {
    type Error = StoreError;

    fn try_from(row: UserRow) -> Result<Self, Self::Error> {
        let password_hash = row
            .password_hash
            .clone()
            .ok_or(StoreError::Invariant("password user has no password hash"))?;
        Ok(Self {
            user: user_from_row(row)?,
            password_hash,
        })
    }
}

fn user_from_row(row: UserRow) -> Result<User, StoreError> {
    let status = match row.status.as_str() {
        "active" => UserStatus::Active,
        "suspended" => UserStatus::Suspended,
        _ => return Err(StoreError::Invariant("unknown user status")),
    };
    Ok(User {
        id: UserId(row.id),
        email: row.email,
        display_name: row.display_name,
        status,
        created_at: row.created_at,
    })
}

fn owner_account_from_row(row: OwnerAccountRow) -> Result<OwnerAccountRecord, StoreError> {
    let external_identities = serde_json::from_value(row.external_identities)?;
    let memberships = serde_json::from_value(row.memberships)?;
    let last_login = match (
        row.last_login_id,
        row.last_login_authentication_method,
        row.last_login_occurred_at,
    ) {
        (Some(id), Some(authentication_method), Some(occurred_at)) => Some(UserLoginEventRecord {
            id,
            user_id: row.id,
            source_ip: row.last_login_ip,
            authentication_method,
            occurred_at,
        }),
        (None, None, None) => None,
        _ => return Err(StoreError::Invariant("incomplete latest login event")),
    };
    let login_count = u64::try_from(row.login_count)
        .map_err(|_| StoreError::Invariant("negative login event count"))?;
    Ok(OwnerAccountRecord {
        user: user_from_row(UserRow {
            id: row.id,
            email: row.email,
            display_name: row.display_name,
            password_hash: None,
            status: row.status,
            created_at: row.created_at,
        })?,
        has_local_password: row.has_local_password,
        external_identities,
        memberships,
        last_login,
        login_count,
    })
}

#[derive(sqlx::FromRow)]
struct MembershipRow {
    organization_id: Uuid,
    principal_id: Uuid,
    role: String,
    organization_slug: String,
    organization_name: String,
}

impl TryFrom<MembershipRow> for Membership {
    type Error = StoreError;

    fn try_from(row: MembershipRow) -> Result<Self, Self::Error> {
        if row.role != "owner" && row.role != "member" {
            return Err(StoreError::Invariant("unknown membership role"));
        }
        Ok(Self {
            organization_id: OrganizationId(row.organization_id),
            organization_slug: row.organization_slug,
            organization_name: row.organization_name,
            principal_id: PrincipalId(row.principal_id),
            role: row.role,
        })
    }
}

#[derive(sqlx::FromRow)]
struct MembershipAuthRow {
    principal_id: Uuid,
    role: String,
}

#[derive(sqlx::FromRow)]
struct OrganizationRow {
    id: Uuid,
    slug: String,
    name: String,
    created_at: DateTime<Utc>,
}

impl TryFrom<OrganizationRow> for Organization {
    type Error = StoreError;

    fn try_from(row: OrganizationRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: OrganizationId(row.id),
            slug: row.slug,
            name: row.name,
            created_at: row.created_at,
        })
    }
}

#[derive(sqlx::FromRow)]
struct ProjectRow {
    id: Uuid,
    organization_id: Uuid,
    slug: String,
    name: String,
    created_at: DateTime<Utc>,
}

impl TryFrom<ProjectRow> for Project {
    type Error = StoreError;

    fn try_from(row: ProjectRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: ProjectId(row.id),
            organization_id: OrganizationId(row.organization_id),
            slug: row.slug,
            name: row.name,
            created_at: row.created_at,
        })
    }
}

#[derive(sqlx::FromRow)]
struct PrincipalRow {
    id: Uuid,
    organization_id: Uuid,
    kind: String,
    name: String,
    user_id: Option<Uuid>,
    enabled: bool,
    created_at: DateTime<Utc>,
}

impl TryFrom<PrincipalRow> for Principal {
    type Error = StoreError;

    fn try_from(row: PrincipalRow) -> Result<Self, Self::Error> {
        let kind = match row.kind.as_str() {
            "user" => PrincipalKind::User,
            "service_account" => PrincipalKind::ServiceAccount,
            _ => return Err(StoreError::Invariant("unknown principal kind")),
        };
        Ok(Self {
            id: PrincipalId(row.id),
            organization_id: OrganizationId(row.organization_id),
            kind,
            name: row.name,
            user_id: row.user_id.map(UserId),
            enabled: row.enabled,
            created_at: row.created_at,
        })
    }
}

#[derive(sqlx::FromRow)]
struct PolicyRow {
    id: Uuid,
    organization_id: Uuid,
    name: String,
    document: Value,
    semantics_digest: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<PolicyRow> for IamPolicy {
    type Error = StoreError;

    fn try_from(row: PolicyRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: PolicyId(row.id),
            organization_id: OrganizationId(row.organization_id),
            name: row.name,
            document: serde_json::from_value(row.document)?,
            semantics_digest: row.semantics_digest,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

#[derive(sqlx::FromRow)]
struct ServiceRow {
    id: Uuid,
    organization_id: Uuid,
    project_id: Uuid,
    provider: String,
    name: String,
    generation: i64,
    state: String,
    spec: Value,
    status: Value,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct FlashAllocationRow {
    id: Uuid,
    organization_id: Uuid,
    spec: Value,
}

#[derive(sqlx::FromRow)]
struct FlowAllocationRow {
    id: Uuid,
    spec: Value,
}

#[derive(sqlx::FromRow)]
struct SyouyuAllocationRow {
    id: Uuid,
    spec: Value,
}

#[derive(sqlx::FromRow)]
struct ResourceQuotaTenantRow {
    id: Uuid,
    slug: String,
    name: String,
    created_at: DateTime<Utc>,
    override_limits: Option<Value>,
}

#[derive(sqlx::FromRow)]
struct TenantServiceAllocationRow {
    organization_id: Uuid,
    provider: String,
    spec: Value,
}

#[derive(sqlx::FromRow)]
struct TenantCredentialUsageRow {
    organization_id: Uuid,
    active_credentials: i64,
}

#[derive(sqlx::FromRow)]
struct RealtimeMetricCollectionTargetRow {
    id: Uuid,
    organization_id: Uuid,
    project_id: Uuid,
}

#[derive(sqlx::FromRow)]
struct DeveloperCredentialMintRow {
    credential_id: Uuid,
    created_by: Uuid,
    organization_id: Uuid,
    project_id: Uuid,
    service_instance_id: Uuid,
    permissions: Vec<String>,
    service_state: String,
    service_spec: Value,
}

#[derive(sqlx::FromRow)]
struct FlowAccessContextRevocationRow {
    expires_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
    project_id: Uuid,
    generation: i64,
}

#[derive(sqlx::FromRow)]
struct DeveloperCredentialContextScopeRow {
    credential_id: Uuid,
    created_by: Uuid,
    organization_id: Uuid,
    project_id: Uuid,
    service_instance_id: Uuid,
    generation: i64,
}

#[derive(sqlx::FromRow)]
struct DeveloperCredentialContextRevocationRow {
    principal_id: Uuid,
    expires_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
}

#[derive(sqlx::FromRow)]
struct FlowDeveloperCredentialMutationScopeRow {
    credential_id: Uuid,
    revoked_at: Option<DateTime<Utc>>,
    organization_id: Uuid,
    project_id: Uuid,
    service_instance_id: Uuid,
    generation: i64,
}

#[derive(sqlx::FromRow)]
struct FlowCredentialActiveContextRow {
    context_id: Uuid,
    expires_at: DateTime<Utc>,
}

impl From<RealtimeMetricCollectionTargetRow> for RealtimeMetricCollectionTarget {
    fn from(row: RealtimeMetricCollectionTargetRow) -> Self {
        Self {
            service_instance_id: ServiceInstanceId(row.id),
            organization_id: OrganizationId(row.organization_id),
            project_id: ProjectId(row.project_id),
        }
    }
}

impl TryFrom<ServiceRow> for ServiceInstance {
    type Error = StoreError;

    fn try_from(row: ServiceRow) -> Result<Self, Self::Error> {
        let state = match row.state.as_str() {
            "provisioning" => ServiceState::Provisioning,
            "ready" => ServiceState::Ready,
            "updating" => ServiceState::Updating,
            "deleting" => ServiceState::Deleting,
            "error" => ServiceState::Error,
            _ => return Err(StoreError::Invariant("unknown service state")),
        };
        Ok(Self {
            id: ServiceInstanceId(row.id),
            organization_id: OrganizationId(row.organization_id),
            project_id: ProjectId(row.project_id),
            provider: row.provider,
            name: row.name,
            generation: row.generation,
            state,
            spec: row.spec,
            status: row.status,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("resource already exists")]
    AlreadyExists,
    #[error("resource state conflicts with the requested operation")]
    Conflict,
    #[error("database migration failed: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("resource was not found")]
    NotFound,
    #[error("request was rejected: {0}")]
    RequestRejected(String),
    #[error("database invariant violated: {0}")]
    Invariant(&'static str),
    #[error("invitation is invalid, expired, revoked, or exhausted")]
    InvitationUnavailable,
    #[error("database operation failed: {0}")]
    Sql(#[from] sqlx::Error),
    #[error("stored JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::ResourceQuotaUsage;
    use heterocloud_domain::{FlashSpec, FlowRateLimit, FlowSpec};
    use serde_json::json;

    #[test]
    fn resource_usage_tracks_totals_and_per_service_maxima()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut usage = ResourceQuotaUsage::default();
        usage.add_flow_spec(&FlowSpec {
            region: "heteronet-global".to_owned(),
            max_participants: 30,
            max_rooms: 10,
            rate_limit: FlowRateLimit {
                requests_per_second: 5,
                burst: 10,
            },
            metadata: json!({}),
        });
        usage.add_flow_spec(&FlowSpec {
            region: "heteronet-global".to_owned(),
            max_participants: 25,
            max_rooms: 20,
            rate_limit: FlowRateLimit {
                requests_per_second: 8,
                burst: 16,
            },
            metadata: json!({}),
        });
        for spec in [
            json!({
                "region": "heteronet-global",
                "image": "registry.example.test/game:v1",
                "replicas": 2,
                "cpu_millis": 500,
                "memory_mib": 512,
                "ephemeral_storage_gib": 3,
                "ports": [],
                "exposure": {"type": "public", "traffic_mode": "forwarded"},
                "env": {}, "command": [], "args": [], "metadata": {}
            }),
            json!({
                "region": "heteronet-global",
                "image": "registry.example.test/game:v2",
                "replicas": 3,
                "cpu_millis": 1000,
                "memory_mib": 256,
                "ephemeral_storage_gib": 2,
                "ports": [],
                "exposure": {"type": "public", "traffic_mode": "forwarded"},
                "env": {}, "command": [], "args": [], "metadata": {}
            }),
        ] {
            usage.add_flash_spec(&serde_json::from_value::<FlashSpec>(spec)?);
        }

        assert_eq!(usage.flow_services, 2);
        assert_eq!(usage.flow_configured_rooms, 30);
        assert_eq!(usage.flow_max_rooms_per_service, 20);
        assert_eq!(usage.flow_max_participants_per_service, 30);
        assert_eq!(usage.flow_max_rate_limit_requests_per_second, 8);
        assert_eq!(usage.flow_max_rate_limit_burst, 16);
        assert_eq!(usage.flash_services, 2);
        assert_eq!(usage.flash_replicas, 5);
        assert_eq!(usage.flash_max_replicas_per_service, 3);
        assert_eq!(usage.flash_max_cpu_millis_per_vm, 1_000);
        assert_eq!(usage.flash_max_memory_mib_per_vm, 512);
        assert_eq!(usage.flash_max_disk_gib_per_vm, 3);
        assert_eq!(usage.flash_cpu_millis, 4_000);
        assert_eq!(usage.flash_memory_mib, 1_792);
        assert_eq!(usage.flash_disk_gib, 12);
        Ok(())
    }
}
