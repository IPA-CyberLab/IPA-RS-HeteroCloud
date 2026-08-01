use chrono::{DateTime, Utc};
use heterocloud_domain::{
    IamPolicy, Organization, OrganizationId, PolicyDocument, PolicyId, Principal, PrincipalId,
    PrincipalKind, Project, ProjectId, ServiceInstance, ServiceInstanceId, ServiceState, User,
    UserId, UserStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction, postgres::PgPoolOptions};
use thiserror::Error;
use uuid::Uuid;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

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
    ) -> Result<Uuid, StoreError> {
        let id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO sessions (id, user_id, token_hash, expires_at)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(id)
        .bind(user_id.0)
        .bind(token_hash.as_slice())
        .bind(expires_at)
        .execute(&self.pool)
        .await?;
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
             SET state = 'ready', status = $3, updated_at = now()
             WHERE id = $1 AND generation = $2 AND provider = 'flow'",
        )
        .bind(id.0)
        .bind(generation)
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

#[derive(Clone, Debug, Serialize)]
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

#[derive(Clone, Debug)]
pub struct AuthorizationContext {
    pub principal_id: PrincipalId,
    pub role: String,
    pub policies: Vec<PolicyDocument>,
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
    #[error("database migration failed: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("resource was not found")]
    NotFound,
    #[error("database invariant violated: {0}")]
    Invariant(&'static str),
    #[error("invitation is invalid, expired, revoked, or exhausted")]
    InvitationUnavailable,
    #[error("database operation failed: {0}")]
    Sql(#[from] sqlx::Error),
    #[error("stored JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
}
