use std::{
    collections::BTreeSet,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use chrono::{DateTime, Utc};
use heterocloud_domain::{MAX_FLOW_ROOMS, PrincipalId};
use heterocloud_store::{NewRealtimeMetricSample, RealtimeMetricCollectionTarget};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::{task::JoinSet, time::MissedTickBehavior};
use tracing::warn;
use uuid::Uuid;

use crate::{error::ApiError, flow_access::FlowAccessInput, routes::AppState};

const METRIC_COLLECTION_INTERVAL: Duration = Duration::from_secs(15);
const METRIC_COLLECTION_CONCURRENCY: usize = 16;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FlowServiceOverview {
    pub measured_at: DateTime<Utc>,
    pub active_rooms: u64,
    pub concurrent_connections: u64,
    pub sfu_participants: u64,
    pub p2p_connections: u64,
    pub ingress_bytes: i64,
    pub egress_bytes: i64,
    pub transferred_bytes: i64,
    pub turn_allocations: Option<u64>,
    pub endpoints: Value,
    pub room_limit: Option<u64>,
}

pub async fn fetch_and_record_realtime_metrics(
    state: &AppState,
    target: &RealtimeMetricCollectionTarget,
    principal_id: PrincipalId,
) -> Result<FlowServiceOverview, ApiError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ApiError::Internal)?
        .as_secs();
    let expires_at = now.checked_add(30).ok_or(ApiError::Internal)?;
    let signed = state
        .config
        .flow_access_signer
        .sign(
            FlowAccessInput {
                organization_id: target.organization_id,
                project_id: target.project_id,
                service_instance_id: target.service_instance_id,
                principal_id,
                permissions: BTreeSet::from(["flow.metrics.read".to_owned()]),
            },
            now,
            expires_at,
            Uuid::now_v7(),
        )
        .map_err(|_| ApiError::Internal)?;
    let url = state
        .config
        .flow_internal_endpoint
        .join("v1/service-overview")
        .map_err(|_| ApiError::Internal)?;
    let response = state
        .flow_client
        .get(url)
        .header("x-flow-principal", signed.encoded)
        .header("x-flow-timestamp", signed.timestamp)
        .header("x-flow-signature", signed.signature)
        .send()
        .await
        .map_err(|error| {
            warn!(error = %error, "Flow metrics request failed");
            ApiError::RealtimeProviderUnavailable
        })?;
    if !response.status().is_success() {
        warn!(status = %response.status(), "Flow metrics request was rejected");
        return Err(ApiError::RealtimeProviderUnavailable);
    }
    let overview = response
        .json::<FlowServiceOverview>()
        .await
        .map_err(|error| {
            warn!(error = %error, "Flow metrics response was invalid");
            ApiError::RealtimeProviderUnavailable
        })?;
    let sample = sample_from_overview(&overview).map_err(|error| {
        warn!(error = %error, "Flow metrics response contained an invalid counter");
        ApiError::RealtimeProviderUnavailable
    })?;
    state
        .store
        .record_realtime_metric_sample(target.service_instance_id, Utc::now(), &sample)
        .await
        .map_err(ApiError::from_store)?;
    Ok(overview)
}

pub fn spawn_realtime_metrics_collector(state: Arc<AppState>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(METRIC_COLLECTION_INTERVAL);
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            collect_ready_realtime_metrics(Arc::clone(&state)).await;
        }
    })
}

async fn collect_ready_realtime_metrics(state: Arc<AppState>) {
    let targets = match state.store.list_ready_flow_metric_targets().await {
        Ok(targets) => targets,
        Err(error) => {
            warn!(error = %error, "failed to list Flow services for metrics collection");
            return;
        }
    };

    for targets in targets.chunks(METRIC_COLLECTION_CONCURRENCY) {
        let mut tasks = JoinSet::new();
        for target in targets.iter().cloned() {
            let state = Arc::clone(&state);
            tasks.spawn(async move {
                let service_instance_id = target.service_instance_id;
                let result =
                    fetch_and_record_realtime_metrics(&state, &target, PrincipalId(Uuid::nil()))
                        .await;
                (service_instance_id, result)
            });
        }
        while let Some(result) = tasks.join_next().await {
            match result {
                Ok((_, Ok(_))) => {}
                Ok((service_instance_id, Err(error))) => {
                    warn!(
                        %service_instance_id,
                        %error,
                        "Flow metrics collection failed"
                    );
                }
                Err(error) => warn!(error = %error, "Flow metrics collection task failed"),
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum MetricValueError {
    #[error("{0} exceeded the PostgreSQL bigint range")]
    Overflow(&'static str),
    #[error("{0} must not be negative")]
    Negative(&'static str),
    #[error("room_limit must be between 1 and {MAX_FLOW_ROOMS} when present")]
    InvalidRoomLimit,
}

fn sample_from_overview(
    overview: &FlowServiceOverview,
) -> Result<NewRealtimeMetricSample, MetricValueError> {
    let room_limit = overview
        .room_limit
        .map(|value| {
            if value == 0 || value > u64::from(MAX_FLOW_ROOMS) {
                return Err(MetricValueError::InvalidRoomLimit);
            }
            to_i64(value, "room_limit")
        })
        .transpose()?;
    Ok(NewRealtimeMetricSample {
        measured_at: overview.measured_at,
        active_rooms: to_i64(overview.active_rooms, "active_rooms")?,
        concurrent_connections: to_i64(overview.concurrent_connections, "concurrent_connections")?,
        sfu_participants: to_i64(overview.sfu_participants, "sfu_participants")?,
        p2p_connections: to_i64(overview.p2p_connections, "p2p_connections")?,
        ingress_bytes: nonnegative(overview.ingress_bytes, "ingress_bytes")?,
        egress_bytes: nonnegative(overview.egress_bytes, "egress_bytes")?,
        transferred_bytes: nonnegative(overview.transferred_bytes, "transferred_bytes")?,
        turn_allocations: overview
            .turn_allocations
            .map(|value| to_i64(value, "turn_allocations"))
            .transpose()?,
        room_limit,
    })
}

fn to_i64(value: u64, field: &'static str) -> Result<i64, MetricValueError> {
    i64::try_from(value).map_err(|_| MetricValueError::Overflow(field))
}

fn nonnegative(value: i64, field: &'static str) -> Result<i64, MetricValueError> {
    if value < 0 {
        return Err(MetricValueError::Negative(field));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use serde_json::json;

    use super::{FlowServiceOverview, sample_from_overview};

    fn overview() -> FlowServiceOverview {
        FlowServiceOverview {
            measured_at: Utc
                .timestamp_opt(1_785_600_000, 0)
                .single()
                .unwrap_or_else(Utc::now),
            active_rooms: 3,
            concurrent_connections: 8,
            sfu_participants: 6,
            p2p_connections: 2,
            ingress_bytes: 100,
            egress_bytes: 200,
            transferred_bytes: 300,
            turn_allocations: Some(1),
            endpoints: json!({"api": ["https://flow.example.test"]}),
            room_limit: Some(100),
        }
    }

    #[test]
    fn current_overview_keeps_room_limit_and_converts_to_storage() {
        let overview = overview();
        let rendered = serde_json::to_value(&overview).ok();
        assert_eq!(
            rendered.as_ref().map(|value| &value["room_limit"]),
            Some(&json!(100))
        );

        let sample = sample_from_overview(&overview);
        assert_eq!(sample.ok().and_then(|sample| sample.room_limit), Some(100));
    }

    #[test]
    fn invalid_provider_counters_are_rejected() {
        let mut zero_limit = overview();
        zero_limit.room_limit = Some(0);
        assert!(sample_from_overview(&zero_limit).is_err());

        let mut excessive_limit = overview();
        excessive_limit.room_limit = Some(1_000_001);
        assert!(sample_from_overview(&excessive_limit).is_err());

        let mut negative_bytes = overview();
        negative_bytes.ingress_bytes = -1;
        assert!(sample_from_overview(&negative_bytes).is_err());
    }
}
