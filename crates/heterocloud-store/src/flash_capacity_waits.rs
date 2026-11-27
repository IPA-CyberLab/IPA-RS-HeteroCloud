use super::{ServiceRow, Store, StoreError, lock_flash_allocations, lock_tenant_allocations};
use chrono::{DateTime, Utc};
use heterocloud_domain::{OrganizationId, PrincipalId, ServiceInstance, ServiceInstanceId};
use serde::Serialize;
use serde_json::Value;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

#[derive(Clone, Copy)]
pub struct FlashCapacityWaitOptions {
    pub seconds: u32,
    pub role_session_id: Option<Uuid>,
}

#[derive(Clone, Copy, Eq, PartialEq, Serialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text", rename_all = "snake_case")]
pub enum FlashCapacityWaitState {
    Waiting,
    Starting,
    Ready,
    Cancelled,
    Expired,
    Rejected,
    Superseded,
}

impl FlashCapacityWaitState {
    fn pending(self) -> bool {
        matches!(self, Self::Waiting | Self::Starting)
    }
}

pub fn flash_start_is_ready(status: &Value, generation: i64) -> bool {
    status["observed_generation"].as_i64() == Some(generation)
        && status["phase"] == "ready"
        && status["stopped"] != true
        && status["desired_replicas"]
            .as_u64()
            .zip(status["ready_replicas"].as_u64())
            .is_some_and(|(desired, ready)| ready >= desired)
}

#[derive(Serialize, sqlx::FromRow)]
pub struct FlashCapacityWait {
    pub id: Uuid,
    pub generation: i64,
    pub state: FlashCapacityWaitState,
    pub reason: Option<String>,
    pub requested_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
pub struct ServiceCapacityWait {
    pub service_instance_id: Uuid,
    #[sqlx(flatten)]
    pub wait: FlashCapacityWait,
}

impl FlashCapacityWait {
    pub fn attach(&self, instance: &mut ServiceInstance) -> Result<(), StoreError> {
        instance.status["capacity_wait"] = serde_json::to_value(self)?;
        Ok(())
    }
}

#[derive(sqlx::FromRow)]
pub struct FlashCapacityClaim {
    #[sqlx(flatten)]
    pub wait: FlashCapacityWait,
    pub service_instance_id: Uuid,
    pub organization_id: Uuid,
    pub principal_id: Uuid,
    pub role_session_id: Option<Uuid>,
    pub created_service: bool,
    attempt: i64,
    pub expired: bool,
}

pub enum FlashCapacityOutcome {
    Pending(String),
    Start,
    Ready,
    Reject(String),
}

impl Store {
    pub async fn flash_capacity_waits(
        &self,
        org: OrganizationId,
        services: &[Uuid],
    ) -> Result<Vec<ServiceCapacityWait>, StoreError> {
        Ok(sqlx::query_as(
            "SELECT service_instance_id, id, generation, state, reason, requested_at, expires_at
             FROM flash_capacity_waits WHERE organization_id=$1 AND service_instance_id=ANY($2)",
        )
        .bind(org.0)
        .bind(services)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn enqueue_flash_capacity_wait(
        &self,
        org: OrganizationId,
        service: ServiceInstanceId,
        principal: PrincipalId,
        options: FlashCapacityWaitOptions,
    ) -> Result<ServiceInstance, StoreError> {
        let mut tx = self.pool.begin().await?;
        let row = lock_service(&mut tx, org, service).await?;
        if row.state == "deleting" {
            return Err(StoreError::Conflict);
        }
        let wait = if row.spec["stopped"] == true {
            Some(enqueue(&mut tx, &row, principal, options, false).await?)
        } else {
            current_wait(&mut tx, row.id).await?.filter(|wait| {
                wait.state == FlashCapacityWaitState::Starting && wait.generation == row.generation
            })
        };
        let mut instance = ServiceInstance::try_from(row)?;
        if let Some(wait) = wait {
            wait.attach(&mut instance)?;
        }
        tx.commit().await?;
        Ok(instance)
    }

    pub async fn claim_flash_capacity_wait(
        &self,
    ) -> Result<Option<FlashCapacityClaim>, StoreError> {
        Ok(sqlx::query_as(
            "UPDATE flash_capacity_waits SET attempt=attempt+1,
                    next_check_at=CASE WHEN expires_at <= clock_timestamp()
                        THEN clock_timestamp() + interval '30 seconds'
                        ELSE LEAST(expires_at, clock_timestamp() + interval '30 seconds') END
             WHERE service_instance_id = (
                 SELECT service_instance_id FROM flash_capacity_waits
                 WHERE state IN ('waiting', 'starting') AND next_check_at <= clock_timestamp()
                 ORDER BY next_check_at, requested_at, id
                 FOR UPDATE SKIP LOCKED LIMIT 1
             )
             RETURNING *, expires_at <= clock_timestamp() AS expired",
        )
        .fetch_optional(&self.pool)
        .await?)
    }

    pub async fn settle_flash_capacity_wait(
        &self,
        claim: &FlashCapacityClaim,
        outcome: FlashCapacityOutcome,
    ) -> Result<(), StoreError> {
        use FlashCapacityWaitState::*;
        let org = OrganizationId(claim.organization_id);
        let service = ServiceInstanceId(claim.service_instance_id);
        let mut tx = self.pool.begin().await?;
        let row = match lock_service(&mut tx, org, service).await {
            Ok(row) => row,
            Err(StoreError::NotFound) => return Ok(()),
            Err(error) => return Err(error),
        };
        let expired: Option<bool> = sqlx::query_scalar(
            "SELECT expires_at <= clock_timestamp() FROM flash_capacity_waits
             WHERE id=$1 AND attempt=$2 AND state IN ('waiting', 'starting') FOR UPDATE",
        )
        .bind(claim.wait.id)
        .bind(claim.attempt)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(expired) = expired else {
            return Ok(());
        };
        let (state, reason) = if row.generation != claim.wait.generation
            || row.state == "deleting"
            || (claim.wait.state == Waiting && row.spec["stopped"] != true)
        {
            (Superseded, Some("configuration_changed".into()))
        } else if claim.wait.state == Starting
            && (matches!(outcome, FlashCapacityOutcome::Ready)
                || flash_start_is_ready(&row.status["status"], row.generation))
        {
            (Ready, None)
        } else if expired {
            if claim.wait.state == Starting {
                Store::change_flash_execution(&mut tx, row, PrincipalId(claim.principal_id), true)
                    .await?;
            }
            (Expired, Some("deadline_exceeded".into()))
        } else {
            match outcome {
                FlashCapacityOutcome::Start if claim.wait.state == Waiting => {
                    let identity_active: bool = sqlx::query_scalar(
                        "SELECT EXISTS (SELECT 1 FROM principals
                            WHERE id=$1 AND organization_id=$2 AND enabled)
                            AND ($3::uuid IS NULL OR EXISTS (
                                SELECT 1 FROM iam_role_sessions
                                WHERE id=$3 AND role_principal_id=$1 AND organization_id=$2
                                  AND revoked_at IS NULL AND expires_at > clock_timestamp()
                                  AND identity_center_session_is_active(id)))",
                    )
                    .bind(claim.principal_id)
                    .bind(org.0)
                    .bind(claim.role_session_id)
                    .fetch_one(&mut *tx)
                    .await?;
                    if !identity_active {
                        finish(
                            &mut tx,
                            claim.wait.id,
                            Rejected,
                            Some("authorization_revoked"),
                        )
                        .await?;
                        tx.commit().await?;
                        return Ok(());
                    }
                    let started = Store::change_flash_execution(
                        &mut tx,
                        row,
                        PrincipalId(claim.principal_id),
                        false,
                    )
                    .await?;
                    let result = sqlx::query(
                        "UPDATE flash_capacity_waits SET state='starting', reason=NULL, generation=$2,
                                expires_at=clock_timestamp() + interval '420 seconds',
                                next_check_at=clock_timestamp()
                         WHERE id=$1 AND expires_at > clock_timestamp()",
                    )
                    .bind(claim.wait.id)
                    .bind(started.generation)
                    .execute(&mut *tx)
                    .await?;
                    if result.rows_affected() == 1 {
                        tx.commit().await?;
                    }
                    // The transaction also owns the desired generation and
                    // outbox event. Losing the deadline rolls both back.
                    return Ok(());
                }
                FlashCapacityOutcome::Pending(reason) => {
                    sqlx::query(
                        "UPDATE flash_capacity_waits SET reason=$2,
                                next_check_at=LEAST(expires_at, clock_timestamp() + interval '5 seconds')
                         WHERE id=$1",
                    )
                    .bind(claim.wait.id)
                    .bind(reason)
                    .execute(&mut *tx)
                    .await?;
                    tx.commit().await?;
                    return Ok(());
                }
                FlashCapacityOutcome::Reject(reason) => {
                    if claim.wait.state == Starting {
                        Store::change_flash_execution(
                            &mut tx,
                            row,
                            PrincipalId(claim.principal_id),
                            true,
                        )
                        .await?;
                    }
                    (Rejected, Some(reason))
                }
                _ => {
                    return Err(StoreError::Invariant(
                        "invalid Flash capacity wait transition",
                    ));
                }
            }
        };
        finish(&mut tx, claim.wait.id, state, reason.as_deref()).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn cancel_flash_capacity_wait(
        &self,
        org: OrganizationId,
        service: ServiceInstanceId,
        principal: PrincipalId,
        id: Uuid,
    ) -> Result<ServiceInstance, StoreError> {
        let mut tx = self.pool.begin().await?;
        let row = lock_service(&mut tx, org, service).await?;
        let mut wait = current_wait(&mut tx, service.0)
            .await?
            .filter(|wait| wait.id == id)
            .ok_or(StoreError::NotFound)?;
        if wait.state == FlashCapacityWaitState::Cancelled {
            let mut instance = ServiceInstance::try_from(row)?;
            wait.attach(&mut instance)?;
            return Ok(instance);
        }
        if !wait.state.pending() || row.generation != wait.generation || row.state == "deleting" {
            return Err(StoreError::Conflict);
        }
        if wait.state == FlashCapacityWaitState::Starting
            && flash_start_is_ready(&row.status["status"], row.generation)
        {
            return Err(StoreError::Conflict);
        }
        let row = if wait.state == FlashCapacityWaitState::Starting {
            Store::change_flash_execution(&mut tx, row, principal, true).await?
        } else {
            row
        };
        finish(&mut tx, id, FlashCapacityWaitState::Cancelled, None).await?;
        wait.state = FlashCapacityWaitState::Cancelled;
        wait.reason = None;
        let mut instance = ServiceInstance::try_from(row)?;
        wait.attach(&mut instance)?;
        tx.commit().await?;
        Ok(instance)
    }
}

pub(super) async fn lock_service(
    tx: &mut Transaction<'_, Postgres>,
    org: OrganizationId,
    id: ServiceInstanceId,
) -> Result<ServiceRow, StoreError> {
    lock_tenant_allocations(tx, org).await?;
    lock_flash_allocations(tx).await?;
    sqlx::query_as(
        "SELECT id, organization_id, project_id, provider, name, generation,
                state, spec, status, created_at, updated_at
         FROM service_instances
         WHERE id=$1 AND organization_id=$2 AND provider='flash' FOR UPDATE",
    )
    .bind(id.0)
    .bind(org.0)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(StoreError::NotFound)
}

pub(super) async fn current_wait(
    tx: &mut Transaction<'_, Postgres>,
    service: Uuid,
) -> Result<Option<FlashCapacityWait>, StoreError> {
    Ok(
        sqlx::query_as("SELECT * FROM flash_capacity_waits WHERE service_instance_id=$1")
            .bind(service)
            .fetch_optional(&mut **tx)
            .await?,
    )
}

pub(super) async fn enqueue(
    tx: &mut Transaction<'_, Postgres>,
    row: &ServiceRow,
    principal: PrincipalId,
    options: FlashCapacityWaitOptions,
    created_service: bool,
) -> Result<FlashCapacityWait, StoreError> {
    if let Some(wait) = current_wait(tx, row.id).await?
        && wait.state.pending()
        && wait.generation == row.generation
    {
        return Ok(wait);
    }
    Ok(sqlx::query_as(
        "INSERT INTO flash_capacity_waits
            (service_instance_id, id, organization_id, principal_id, role_session_id, created_service,
             generation, state, reason, expires_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,'waiting','checking_capacity',
                 clock_timestamp() + make_interval(secs => $8))
         ON CONFLICT (service_instance_id) DO UPDATE SET
             id=excluded.id, principal_id=excluded.principal_id, role_session_id=excluded.role_session_id,
             created_service=excluded.created_service, generation=excluded.generation,
             state=excluded.state, reason=excluded.reason, requested_at=excluded.requested_at,
             expires_at=excluded.expires_at, next_check_at=excluded.next_check_at, attempt=0
         RETURNING *",
    )
    .bind(row.id)
    .bind(Uuid::now_v7())
    .bind(row.organization_id)
    .bind(principal.0)
    .bind(options.role_session_id)
    .bind(created_service)
    .bind(row.generation)
    .bind(f64::from(options.seconds))
    .fetch_one(&mut **tx)
    .await?)
}

pub(super) async fn supersede(
    tx: &mut Transaction<'_, Postgres>,
    service: ServiceInstanceId,
    stopped: bool,
) -> Result<(), StoreError> {
    sqlx::query(
        "UPDATE flash_capacity_waits SET state=$2, reason=$3
         WHERE service_instance_id=$1 AND state IN ('waiting', 'starting')",
    )
    .bind(service.0)
    .bind(if stopped {
        FlashCapacityWaitState::Cancelled
    } else {
        FlashCapacityWaitState::Superseded
    })
    .bind(if stopped {
        None
    } else {
        Some("configuration_changed")
    })
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn finish(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    state: FlashCapacityWaitState,
    reason: Option<&str>,
) -> Result<(), StoreError> {
    sqlx::query("UPDATE flash_capacity_waits SET state=$2, reason=$3 WHERE id=$1")
        .bind(id)
        .bind(state)
        .bind(reason)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
