use super::{
    AppState, AuthenticatedActor, authorize_actor_impl, evaluate_flash_placement,
    flash_collection_resource, flash_service_resource,
};
use crate::{
    error::ApiError,
    flash_operations::{allowed, context},
    flash_provider::{FlashPlacementBlock, FlashPlacementResult},
};
use heterocloud_domain::{
    OrganizationId, PrincipalId, ServiceInstance, ServiceInstanceId, ServiceState,
};
use heterocloud_store::{
    FlashCapacityClaim, FlashCapacityOutcome, FlashCapacityWaitState, FlashPlacementInput,
    StoreError, flash_start_is_ready,
};
use std::{collections::HashMap, sync::Arc, time::Duration};

pub(super) async fn attach(
    state: &AppState,
    org: OrganizationId,
    instances: &mut [ServiceInstance],
) -> Result<(), ApiError> {
    if instances.is_empty() {
        return Ok(());
    }
    let ids: Vec<_> = instances.iter().map(|instance| instance.id.0).collect();
    let waits: HashMap<_, _> = state
        .store
        .flash_capacity_waits(org, &ids)
        .await
        .map_err(ApiError::from_store)?
        .into_iter()
        .map(|row| (row.service_instance_id, row.wait))
        .collect();
    for instance in instances {
        if let Some(wait) = waits.get(&instance.id.0) {
            wait.attach(instance).map_err(ApiError::from_store)?;
        }
    }
    Ok(())
}

pub fn spawn_reconciler(state: Arc<AppState>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(1));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            if let Err(error) = reconcile_one(&state).await {
                tracing::warn!(%error, "Flash capacity wait reconciliation failed");
            }
        }
    })
}

pub async fn reconcile_one(state: &AppState) -> Result<(), StoreError> {
    let Some(claim) = state.store.claim_flash_capacity_wait().await? else {
        return Ok(());
    };
    let Some(instance) = state
        .store
        .service_instance(ServiceInstanceId(claim.service_instance_id))
        .await?
    else {
        return Ok(());
    };
    let outcome = if instance.generation != claim.wait.generation
        || instance.state == ServiceState::Deleting
    {
        pending("configuration_changed")
    } else if claim.wait.state == FlashCapacityWaitState::Starting {
        observe_start(state, &instance, PrincipalId(claim.principal_id)).await?
    } else if claim.expired {
        pending("deadline_exceeded")
    } else if !can_start(state, &claim).await? {
        FlashCapacityOutcome::Reject("authorization_revoked".into())
    } else {
        check_capacity(state, &claim, instance).await
    };
    match state
        .store
        .settle_flash_capacity_wait(&claim, outcome)
        .await
    {
        // Quota and attachment may change while the provider is checking a
        // read-only snapshot. Activation validates them again under its locks.
        Err(StoreError::RequestRejected(_) | StoreError::NotFound) => {
            state
                .store
                .settle_flash_capacity_wait(
                    &claim,
                    FlashCapacityOutcome::Reject("allocation_denied".into()),
                )
                .await
        }
        other => other,
    }
}

async fn can_start(state: &AppState, claim: &FlashCapacityClaim) -> Result<bool, StoreError> {
    let org = OrganizationId(claim.organization_id);
    let principal = PrincipalId(claim.principal_id);
    let (action, resource) = if claim.created_service {
        ("flash:CreateInstance", flash_collection_resource(org.0))
    } else {
        (
            "flash:UpdateInstance",
            flash_service_resource(org.0, claim.service_instance_id),
        )
    };
    if let Some(id) = claim.role_session_id {
        let Some(session) = state
            .store
            .authenticate_browser_role_session(id)
            .await?
            .filter(|session| {
                session.principal_id == principal.0 && session.organization_id == org.0
            })
        else {
            return Ok(false);
        };
        return Ok(authorize_actor_impl(
            state,
            &AuthenticatedActor::RoleSession {
                session: Box::new(session),
                browser: None,
            },
            org,
            action,
            &resource,
            false,
            false,
        )
        .await
        .is_ok());
    }
    Ok(allowed(state, principal, org, action, &resource).await)
}

async fn check_capacity(
    state: &AppState,
    claim: &FlashCapacityClaim,
    mut instance: ServiceInstance,
) -> FlashCapacityOutcome {
    if let Some(spec) = instance.spec.as_object_mut() {
        spec.remove("stopped");
    }
    let input = FlashPlacementInput {
        organization_id: instance.organization_id,
        project_id: instance.project_id,
        principal_id: PrincipalId(claim.principal_id),
        service_instance_id: instance.id,
        existing_generation: Some(instance.generation),
        quota_user_id: None,
        spec: instance.spec,
    };
    let result = match evaluate_flash_placement(state, &instance.name, input).await {
        Ok(FlashPlacementResult::Feasible) => return FlashCapacityOutcome::Start,
        Ok(result) => result,
        Err(ApiError::BadRequest(_) | ApiError::NotFound | ApiError::Forbidden) => {
            return FlashCapacityOutcome::Reject("allocation_denied".into());
        }
        Err(_) => return pending("capacity_metrics_unavailable"),
    };
    let reason = serde_json::to_value(result)
        .ok()
        .and_then(|value| value["reason"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "capacity_metrics_unavailable".into());
    match result {
        FlashPlacementResult::Blocked { reason: block }
            if !matches!(
                block,
                FlashPlacementBlock::InsufficientComputeCapacity
                    | FlashPlacementBlock::InsufficientPodCapacity
                    | FlashPlacementBlock::InsufficientDiskCapacity
                    | FlashPlacementBlock::InsufficientInodes
                    | FlashPlacementBlock::InsufficientReplicatedStorage
                    | FlashPlacementBlock::InsufficientTopologyCapacity
                    | FlashPlacementBlock::NoEligibleNode
                    | FlashPlacementBlock::GpuUnavailable
            ) =>
        {
            FlashCapacityOutcome::Reject(reason)
        }
        _ => FlashCapacityOutcome::Pending(reason),
    }
}

async fn observe_start(
    state: &AppState,
    instance: &ServiceInstance,
    principal: PrincipalId,
) -> Result<FlashCapacityOutcome, StoreError> {
    if flash_start_is_ready(&instance.status["status"], instance.generation) {
        return Ok(FlashCapacityOutcome::Ready);
    }
    let Some(provider) = &state.flash_provider else {
        return Ok(pending("startup_pending"));
    };
    let Ok(status) = provider.get_status(context(instance, principal)).await else {
        return Ok(pending("startup_pending"));
    };
    if flash_start_is_ready(&status, instance.generation) {
        state
            .store
            .record_flash_observation(instance, &status)
            .await?;
        return Ok(FlashCapacityOutcome::Ready);
    }
    if status["phase"] == "error" {
        return Ok(FlashCapacityOutcome::Reject("startup_failed".into()));
    }
    Ok(pending("startup_pending"))
}

fn pending(reason: &str) -> FlashCapacityOutcome {
    FlashCapacityOutcome::Pending(reason.into())
}
