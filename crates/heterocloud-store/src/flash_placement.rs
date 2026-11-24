use super::{
    Store, StoreError, organizations, prepare_flash_spec, principal_user_id,
    resource_quota_in_transaction, validate_flash_disk_update, validate_vpc_attachment,
};
use heterocloud_domain::{OrganizationId, PrincipalId, ProjectId, ServiceInstanceId, UserId};
use serde_json::Value;
use uuid::Uuid;

pub struct FlashPlacementInput {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub principal_id: PrincipalId,
    pub service_instance_id: ServiceInstanceId,
    pub existing_generation: Option<i64>,
    pub quota_user_id: Option<UserId>,
    pub spec: Value,
}

impl Store {
    /// Validate a proposed Flash allocation using a consistent, read-only snapshot.
    pub async fn preview_flash_placement(
        &self,
        input: FlashPlacementInput,
    ) -> Result<(Value, Option<UserId>), StoreError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await?;
        let org = input.organization_id;
        let id = input.service_instance_id;
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM projects WHERE id = $1 AND organization_id = $2)",
        )
        .bind(input.project_id.0)
        .bind(org.0)
        .fetch_one(&mut *tx)
        .await?;
        if !exists {
            return Err(StoreError::NotFound);
        }
        let existing: Option<(Value, Value, Option<Uuid>)> = if let Some(generation) =
            input.existing_generation
        {
            Some(sqlx::query_as(
                "SELECT spec, status, quota_user_id FROM service_instances WHERE id = $1 AND organization_id = $2
                 AND provider = 'flash' AND state <> 'deleting' AND generation = $3",
            )
            .bind(id.0)
            .bind(org.0)
            .bind(generation)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(StoreError::Conflict)?)
        } else {
            None
        };
        let quota_user = match &existing {
            Some((_, _, owner)) => owner.map(UserId),
            None => input.quota_user_id,
        };
        if existing.is_none() {
            if let Some(user_id) = quota_user {
                let member: bool = sqlx::query_scalar(
                    "SELECT EXISTS (SELECT 1 FROM organization_memberships WHERE organization_id = $1 AND user_id = $2)",
                )
                .bind(org.0)
                .bind(user_id.0)
                .fetch_one(&mut *tx)
                .await?;
                if !member {
                    return Err(StoreError::NotFound);
                }
            } else {
                let has_member_limits: bool = sqlx::query_scalar(
                    "SELECT EXISTS (SELECT 1 FROM organization_member_resource_quotas WHERE organization_id = $1)",
                )
                .bind(org.0)
                .fetch_one(&mut *tx)
                .await?;
                if has_member_limits {
                    return Err(StoreError::RequestRejected(
                        "a service account must have an allocation owner before creating resources"
                            .into(),
                    ));
                }
            }
        }
        let excluded = input.existing_generation.map(|_| id);
        let allow_reduction = existing
            .as_ref()
            .is_some_and(|(spec, _, _)| spec.get("stopped").and_then(Value::as_bool) != Some(true));
        let quota = resource_quota_in_transaction(&mut tx, org).await?;
        let requested = prepare_flash_spec(
            &mut tx,
            org,
            input.principal_id,
            excluded,
            existing.as_ref().map(|(spec, _, _)| spec),
            input.spec,
            &quota,
        )
        .await?;
        organizations::validate_cloud_organization_allocation(
            &mut tx,
            org,
            excluded,
            "flash",
            &requested,
            allow_reduction,
        )
        .await?;
        if let Some(user) = quota_user {
            organizations::validate_member_allocation(
                &mut tx,
                org,
                user,
                excluded,
                "flash",
                &requested,
                &quota,
                allow_reduction,
            )
            .await?;
        }
        if let Some((spec, status, _)) = &existing {
            validate_flash_disk_update(spec, status, &requested)?;
        }
        validate_vpc_attachment(&mut tx, org, input.project_id, id, &requested, false).await?;
        let subject = principal_user_id(&mut tx, org, input.principal_id)
            .await?
            .map(UserId);
        tx.commit().await?;
        Ok((requested, subject))
    }
}
