use std::collections::BTreeMap;

use heterocloud_domain::{
    FlashSpec, FlowSpec, OrganizationId, PrincipalId, ResourceQuotaLimits, ServiceInstanceId,
    SyouyuSpec, UserId,
};
use serde::Serialize;
use serde_json::Value;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::{
    ResourceQuotaUsage, Store, StoreError, lock_tenant_allocations, parse_resource_quota,
    resource_quota_in_transaction, validate_resource_quota,
};

#[derive(Clone, Debug, Serialize)]
pub struct OrganizationMemberQuota {
    pub user_id: UserId,
    pub principal_id: Uuid,
    pub display_name: String,
    pub email: String,
    pub role: String,
    pub override_limits: Option<ResourceQuotaLimits>,
    pub effective_limits: ResourceQuotaLimits,
    /// Known allocations are numbers; measurements unavailable at member
    /// granularity are null, never a misleading zero.
    pub usage: Value,
}

#[derive(sqlx::FromRow)]
struct MemberRow {
    user_id: Uuid,
    principal_id: Uuid,
    display_name: String,
    email: String,
    role: String,
    override_limits: Option<Value>,
}

#[derive(sqlx::FromRow)]
struct OwnedAllocation {
    quota_user_id: Uuid,
    provider: String,
    spec: Value,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct UnassignedQuotaResource {
    pub id: Uuid,
    pub name: String,
    pub provider: String,
}

/// A member's configured ceiling cannot override the enclosing organization.
/// Applying the cap on reads as well as writes handles a later reduction of
/// the organization ceiling without silently expanding any member allocation.
fn capped_limits(
    organization: &ResourceQuotaLimits,
    member: &ResourceQuotaLimits,
) -> Result<ResourceQuotaLimits, StoreError> {
    fn cap(value: &mut Value, ceiling: &Value) -> Result<(), StoreError> {
        match (value, ceiling) {
            (Value::Object(values), Value::Object(ceilings)) if values.len() == ceilings.len() => {
                for (key, value) in values {
                    cap(
                        value,
                        ceilings
                            .get(key)
                            .ok_or(StoreError::Invariant("quota shape differs"))?,
                    )?;
                }
                Ok(())
            }
            (Value::Number(value), Value::Number(ceiling)) => {
                let value_u64 = value
                    .as_u64()
                    .ok_or(StoreError::Invariant("negative quota"))?;
                let maximum = ceiling
                    .as_u64()
                    .ok_or(StoreError::Invariant("negative quota"))?;
                *value = value_u64.min(maximum).into();
                Ok(())
            }
            _ => Err(StoreError::Invariant("quota shape differs")),
        }
    }
    let mut result = serde_json::to_value(member)?;
    cap(&mut result, &serde_json::to_value(organization)?)?;
    let mut limits = parse_resource_quota(result)?;
    // These resources are provisioned in a single shared organization scope.
    // They cannot safely be presented as independently enforced member limits.
    limits.registry = organization.registry.clone();
    limits.syouyu.max_credentials_per_bucket = organization.syouyu.max_credentials_per_bucket;
    limits.syouyu.max_total_credentials = organization.syouyu.max_total_credentials;
    Ok(limits)
}

impl Store {
    pub async fn unassigned_quota_resources(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<UnassignedQuotaResource>, StoreError> {
        Ok(sqlx::query_as::<_, UnassignedQuotaResource>(
            "SELECT id, name, provider FROM service_instances
             WHERE organization_id = $1 AND quota_user_id IS NULL AND state <> 'deleting'
               AND provider IN ('flow', 'flash', 'syouyu')
             UNION ALL
             SELECT id, name, 'service_account' AS provider FROM principals
             WHERE organization_id = $1 AND quota_user_id IS NULL AND kind = 'service_account' AND enabled
             ORDER BY provider, name, id",
        ).bind(organization_id.0).fetch_all(&self.pool).await?)
    }

    pub async fn assign_quota_resource(
        &self,
        organization_id: OrganizationId,
        resource_id: Uuid,
        user_id: UserId,
        service_account: bool,
    ) -> Result<(), StoreError> {
        let mut transaction = self.pool.begin().await?;
        lock_tenant_allocations(&mut transaction, organization_id).await?;
        let member: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM organization_memberships WHERE organization_id = $1 AND user_id = $2)")
            .bind(organization_id.0).bind(user_id.0).fetch_one(&mut *transaction).await?;
        if !member {
            return Err(StoreError::NotFound);
        }
        let changed = if service_account {
            sqlx::query("UPDATE principals SET quota_user_id = $3, updated_at = now() WHERE id = $1 AND organization_id = $2 AND kind = 'service_account' AND quota_user_id IS NULL AND enabled")
                .bind(resource_id).bind(organization_id.0).bind(user_id.0).execute(&mut *transaction).await?
        } else {
            sqlx::query("UPDATE service_instances SET quota_user_id = $3 WHERE id = $1 AND organization_id = $2 AND quota_user_id IS NULL AND state <> 'deleting' AND provider IN ('flow', 'flash', 'syouyu')")
                .bind(resource_id).bind(organization_id.0).bind(user_id.0).execute(&mut *transaction).await?
        };
        if changed.rows_affected() != 1 {
            return Err(StoreError::Conflict);
        }
        if !service_account {
            sqlx::query("UPDATE service_quota_owners SET user_id = $3 WHERE service_instance_id = $1 AND organization_id = $2 AND user_id IS NULL")
                .bind(resource_id).bind(organization_id.0).bind(user_id.0).execute(&mut *transaction).await?;
            self.enqueue_flash_quota_reconcile_tx(&mut transaction, Some(organization_id))
                .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn principal_quota_user(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<Option<UserId>, StoreError> {
        let value = sqlx::query_scalar::<_, Option<Uuid>>(
            "SELECT COALESCE(user_id, quota_user_id) FROM principals WHERE id = $1 AND organization_id = $2 AND enabled = true",
        ).bind(principal_id.0).bind(organization_id.0).fetch_optional(&self.pool).await?
            .ok_or(StoreError::NotFound)?;
        Ok(value.map(UserId))
    }

    pub async fn service_quota_user(
        &self,
        organization_id: OrganizationId,
        service_id: ServiceInstanceId,
    ) -> Result<Option<UserId>, StoreError> {
        let value = sqlx::query_scalar::<_, Option<Uuid>>(
            "SELECT quota_user_id FROM service_instances WHERE id = $1 AND organization_id = $2",
        )
        .bind(service_id.0)
        .bind(organization_id.0)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(StoreError::NotFound)?;
        Ok(value.map(UserId))
    }

    pub async fn set_organization_member_role(
        &self,
        organization_id: OrganizationId,
        user_id: UserId,
        role: &str,
    ) -> Result<(), StoreError> {
        if !matches!(role, "admin" | "member") {
            return Err(StoreError::RequestRejected(
                "role must be admin or member".into(),
            ));
        }
        let result = sqlx::query(
            "UPDATE organization_memberships SET role = $3
             WHERE organization_id = $1 AND user_id = $2 AND role <> 'owner'",
        )
        .bind(organization_id.0)
        .bind(user_id.0)
        .bind(role)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    pub async fn organization_member_quotas(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<OrganizationMemberQuota>, StoreError> {
        let organization_limits = self.effective_resource_quota(organization_id).await?;
        let rows = sqlx::query_as::<_, MemberRow>(
            "SELECT m.user_id, m.principal_id, u.display_name, u.email, m.role,
                    q.limits AS override_limits
             FROM organization_memberships m JOIN users u ON u.id = m.user_id
             LEFT JOIN organization_member_resource_quotas q
               ON q.organization_id = m.organization_id AND q.user_id = m.user_id
             WHERE m.organization_id = $1 ORDER BY lower(u.display_name), u.id",
        )
        .bind(organization_id.0)
        .fetch_all(&self.pool)
        .await?;
        let allocations = sqlx::query_as::<_, OwnedAllocation>(
            "SELECT quota_user_id, provider, spec FROM service_instances
             WHERE organization_id = $1 AND quota_user_id IS NOT NULL
               AND provider IN ('flow', 'flash', 'syouyu') AND state <> 'deleting'",
        )
        .bind(organization_id.0)
        .fetch_all(&self.pool)
        .await?;
        let mut usage_by_user = BTreeMap::<Uuid, ResourceQuotaUsage>::new();
        for allocation in allocations {
            let usage = usage_by_user.entry(allocation.quota_user_id).or_default();
            match allocation.provider.as_str() {
                "flow" => {
                    usage.add_flow_spec(&serde_json::from_value::<FlowSpec>(allocation.spec)?)
                }
                "flash" => {
                    usage.add_flash_spec(&serde_json::from_value::<FlashSpec>(allocation.spec)?)
                }
                "syouyu" => {
                    usage.add_syouyu_spec(&serde_json::from_value::<SyouyuSpec>(allocation.spec)?)
                }
                _ => return Err(StoreError::Invariant("unknown quota provider")),
            }
        }
        rows.into_iter()
            .map(|row| {
                let override_limits = row.override_limits.map(parse_resource_quota).transpose()?;
                let effective_limits = match &override_limits {
                    Some(limits) => capped_limits(&organization_limits, limits)?,
                    None => organization_limits.clone(),
                };
                let mut usage =
                    serde_json::to_value(usage_by_user.remove(&row.user_id).unwrap_or_default())?;
                for field in [
                    "flow_developer_credentials",
                    "flow_max_developer_credentials_per_service",
                    "registry_credentials",
                    "syouyu_credentials",
                ] {
                    usage[field] = Value::Null;
                }
                Ok(OrganizationMemberQuota {
                    user_id: UserId(row.user_id),
                    principal_id: row.principal_id,
                    display_name: row.display_name,
                    email: row.email,
                    role: row.role,
                    override_limits,
                    effective_limits,
                    usage,
                })
            })
            .collect()
    }

    pub async fn effective_member_quota(
        &self,
        organization_id: OrganizationId,
        user_id: UserId,
    ) -> Result<ResourceQuotaLimits, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let organization_limits =
            resource_quota_in_transaction(&mut transaction, organization_id).await?;
        let effective = member_quota_in_transaction(
            &mut transaction,
            organization_id,
            user_id,
            &organization_limits,
        )
        .await?;
        transaction.commit().await?;
        Ok(effective)
    }

    pub async fn set_member_resource_quota(
        &self,
        organization_id: OrganizationId,
        user_id: UserId,
        limits: Option<&ResourceQuotaLimits>,
    ) -> Result<ResourceQuotaLimits, StoreError> {
        let mut transaction = self.pool.begin().await?;
        lock_tenant_allocations(&mut transaction, organization_id).await?;
        let organization_limits =
            resource_quota_in_transaction(&mut transaction, organization_id).await?;
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM organization_memberships WHERE organization_id = $1 AND user_id = $2)",
        ).bind(organization_id.0).bind(user_id.0).fetch_one(&mut *transaction).await?;
        if !exists {
            return Err(StoreError::NotFound);
        }
        let effective = if let Some(limits) = limits {
            validate_resource_quota(limits)?;
            let unassigned: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM service_instances WHERE organization_id = $1 AND quota_user_id IS NULL AND state <> 'deleting' AND provider IN ('flow', 'flash', 'syouyu')) OR EXISTS (SELECT 1 FROM principals WHERE organization_id = $1 AND quota_user_id IS NULL AND kind = 'service_account' AND enabled)",
            ).bind(organization_id.0).fetch_one(&mut *transaction).await?;
            if unassigned {
                return Err(StoreError::RequestRejected("assign existing services and service accounts to a member before setting personal limits".into()));
            }
            if limits.registry != organization_limits.registry
                || limits.syouyu.max_credentials_per_bucket
                    != organization_limits.syouyu.max_credentials_per_bucket
                || limits.syouyu.max_total_credentials
                    != organization_limits.syouyu.max_total_credentials
            {
                return Err(StoreError::RequestRejected(
                    "registry and Syouyu credential limits are shared by the organization".into(),
                ));
            }
            // Reject a misleading allocation above the enclosing limit. Existing
            // overrides are still capped if the organization is later reduced.
            if &capped_limits(&organization_limits, limits)? != limits {
                return Err(StoreError::RequestRejected(
                    "member limits cannot exceed organization limits".into(),
                ));
            }
            sqlx::query(
                "INSERT INTO organization_member_resource_quotas (organization_id, user_id, limits)
                 VALUES ($1, $2, $3) ON CONFLICT (organization_id, user_id)
                 DO UPDATE SET limits = EXCLUDED.limits, updated_at = now()",
            )
            .bind(organization_id.0)
            .bind(user_id.0)
            .bind(serde_json::to_value(limits)?)
            .execute(&mut *transaction)
            .await?;
            limits.clone()
        } else {
            sqlx::query("DELETE FROM organization_member_resource_quotas WHERE organization_id = $1 AND user_id = $2")
                .bind(organization_id.0).bind(user_id.0).execute(&mut *transaction).await?;
            organization_limits
        };
        self.enqueue_flash_quota_reconcile_tx(&mut transaction, Some(organization_id))
            .await?;
        transaction.commit().await?;
        Ok(effective)
    }
}

pub(super) async fn member_quota_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: OrganizationId,
    user_id: UserId,
    organization_limits: &ResourceQuotaLimits,
) -> Result<ResourceQuotaLimits, StoreError> {
    let member = sqlx::query_scalar::<_, Option<Value>>(
        "SELECT q.limits FROM organization_memberships m
         LEFT JOIN organization_member_resource_quotas q
           ON q.organization_id = m.organization_id AND q.user_id = m.user_id
         WHERE m.organization_id = $1 AND m.user_id = $2",
    )
    .bind(organization_id.0)
    .bind(user_id.0)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(StoreError::NotFound)?;
    match member {
        Some(member) => capped_limits(organization_limits, &parse_resource_quota(member)?),
        None => Ok(organization_limits.clone()),
    }
}

pub(super) async fn validate_member_allocation(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: OrganizationId,
    user_id: UserId,
    excluded_service: Option<ServiceInstanceId>,
    provider: &str,
    requested: &Value,
    organization_limits: &ResourceQuotaLimits,
) -> Result<(), StoreError> {
    if !matches!(provider, "flow" | "flash" | "syouyu") {
        return Ok(());
    }
    let limits =
        member_quota_in_transaction(transaction, organization_id, user_id, organization_limits)
            .await?;
    let rows: Vec<Value> = sqlx::query_scalar(
        "SELECT spec FROM service_instances WHERE organization_id = $1 AND quota_user_id = $2
         AND provider = $3 AND state <> 'deleting' AND ($4::uuid IS NULL OR id <> $4)",
    )
    .bind(organization_id.0)
    .bind(user_id.0)
    .bind(provider)
    .bind(excluded_service.map(|id| id.0))
    .fetch_all(&mut **transaction)
    .await?;
    let mut usage = ResourceQuotaUsage::default();
    for value in rows.iter().chain(std::iter::once(requested)) {
        match provider {
            "flow" => usage.add_flow_spec(&serde_json::from_value::<FlowSpec>(value.clone())?),
            "flash" => usage.add_flash_spec(&serde_json::from_value::<FlashSpec>(value.clone())?),
            "syouyu" => {
                usage.add_syouyu_spec(&serde_json::from_value::<SyouyuSpec>(value.clone())?)
            }
            _ => unreachable!(),
        }
    }
    let exceeded = match provider {
        "flow" => {
            usage.flow_services > u64::from(limits.flow.max_services)
                || usage.flow_configured_rooms > limits.flow.max_total_rooms
                || usage.flow_max_rooms_per_service > u64::from(limits.flow.max_rooms_per_service)
                || usage.flow_max_participants_per_service
                    > u64::from(limits.flow.max_participants_per_service)
                || usage.flow_max_rate_limit_requests_per_second
                    > u64::from(limits.flow.max_rate_limit_requests_per_second)
                || usage.flow_max_rate_limit_burst > u64::from(limits.flow.max_rate_limit_burst)
        }
        "flash" => {
            usage.flash_services > u64::from(limits.flash.max_services)
                || usage.flash_replicas > limits.flash.max_total_replicas
                || usage.flash_cpu_millis > limits.flash.max_total_cpu_millis
                || usage.flash_memory_mib > limits.flash.max_total_memory_mib
                || usage.flash_disk_gib > limits.flash.max_total_disk_gib
                || usage.flash_max_replicas_per_service
                    > u64::from(limits.flash.max_replicas_per_service)
                || usage.flash_max_cpu_millis_per_vm > u64::from(limits.flash.max_cpu_millis_per_vm)
                || usage.flash_max_memory_mib_per_vm > u64::from(limits.flash.max_memory_mib_per_vm)
                || usage.flash_max_disk_gib_per_vm > u64::from(limits.flash.max_disk_gib_per_vm)
        }
        "syouyu" => {
            usage.syouyu_buckets > u64::from(limits.syouyu.max_buckets)
                || usage.syouyu_configured_bytes > limits.syouyu.max_total_bytes
                || usage.syouyu_max_bytes_per_bucket > limits.syouyu.max_bytes_per_bucket
                || usage.syouyu_max_objects_per_bucket > limits.syouyu.max_objects_per_bucket
        }
        _ => false,
    };
    if exceeded {
        return Err(StoreError::RequestRejected(format!(
            "{provider} allocation exceeds the member quota"
        )));
    }
    Ok(())
}
