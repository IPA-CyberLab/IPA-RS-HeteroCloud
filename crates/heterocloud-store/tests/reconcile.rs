use std::{env, error::Error};

use chrono::{Duration, TimeZone, Utc};
use heterocloud_domain::{OrganizationId, ProjectId, ServiceInstanceId, ServiceState};
use heterocloud_store::{BootstrapAdmin, NewRealtimeMetricSample, Store, StoreError};
use serde_json::json;
use uuid::Uuid;

const TEST_DATABASE_ENV: &str = "HETEROCLOUD_STORE_TEST_DATABASE_URL";

#[tokio::test]
async fn reconcile_ready_update_is_generation_and_provider_guarded() -> Result<(), Box<dyn Error>> {
    let Ok(database_url) = env::var(TEST_DATABASE_ENV) else {
        return Ok(());
    };
    let store = Store::connect(&database_url, 4).await?;
    let database_name: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(store.pool())
        .await?;
    if !database_name.starts_with("heterocloud_test_") {
        return Err(format!(
            "{TEST_DATABASE_ENV} must name a disposable database starting with heterocloud_test_"
        )
        .into());
    }
    sqlx::query("DROP SCHEMA public CASCADE")
        .execute(store.pool())
        .await?;
    sqlx::query("CREATE SCHEMA public")
        .execute(store.pool())
        .await?;
    store.migrate().await?;

    let owner = store
        .bootstrap_admin(BootstrapAdmin {
            email: "reconcile-owner@example.test",
            display_name: "Reconcile Test Owner",
            password_hash: "test-password-hash",
            organization_slug: "reconcile-ready-test",
            organization_name: "Reconcile Ready Test",
        })
        .await?;
    let membership = owner
        .memberships
        .first()
        .ok_or("bootstrap owner has no membership")?;
    let organization_id = OrganizationId(membership.organization_id.0);
    let project = store
        .create_project(organization_id, "flow-project", "Flow Project")
        .await?;
    let instance = store
        .create_service_instance(
            organization_id,
            ProjectId(project.id.0),
            membership.principal_id,
            "flow",
            "production-flow",
            json!({"region": "global"}),
        )
        .await?;
    let operation_id = Uuid::from_u128(42);

    assert!(
        !store
            .mark_service_instance_ready(
                instance.id,
                "flow",
                instance.generation + 1,
                operation_id,
                json!("accepted"),
            )
            .await?
    );
    assert_eq!(
        store
            .service_instance(ServiceInstanceId(instance.id.0))
            .await?
            .ok_or("Flow instance disappeared")?
            .state,
        ServiceState::Provisioning
    );
    assert!(
        store
            .mark_service_instance_ready(
                instance.id,
                "flow",
                instance.generation,
                operation_id,
                json!({"phase": "accepted"}),
            )
            .await?
    );
    let ready = store
        .service_instance(instance.id)
        .await?
        .ok_or("ready Flow instance disappeared")?;
    assert_eq!(ready.state, ServiceState::Ready);
    assert_eq!(ready.status["operation_id"], json!(operation_id));
    assert_eq!(ready.status["status"], json!({"phase": "accepted"}));

    let sampled_at = Utc
        .with_ymd_and_hms(2026, 8, 1, 0, 0, 1)
        .single()
        .ok_or("invalid metric test timestamp")?;
    let sample = NewRealtimeMetricSample {
        measured_at: sampled_at,
        active_rooms: 1,
        concurrent_connections: 2,
        sfu_participants: 2,
        p2p_connections: 0,
        ingress_bytes: 100,
        egress_bytes: 200,
        transferred_bytes: 300,
        turn_allocations: Some(1),
        room_limit: Some(100),
    };
    store
        .record_realtime_metric_sample(instance.id, sampled_at, &sample)
        .await?;
    store
        .record_realtime_metric_sample(
            instance.id,
            sampled_at + Duration::seconds(10),
            &NewRealtimeMetricSample {
                measured_at: sampled_at + Duration::seconds(5),
                active_rooms: 2,
                ..sample.clone()
            },
        )
        .await?;
    store
        .record_realtime_metric_sample(
            instance.id,
            sampled_at + Duration::seconds(11),
            &NewRealtimeMetricSample {
                measured_at: sampled_at + Duration::seconds(3),
                active_rooms: 99,
                ..sample.clone()
            },
        )
        .await?;
    store
        .record_realtime_metric_sample(
            instance.id,
            sampled_at + Duration::seconds(15),
            &NewRealtimeMetricSample {
                measured_at: sampled_at + Duration::seconds(15),
                active_rooms: 3,
                ..sample
            },
        )
        .await?;
    let history = store
        .realtime_metric_history(instance.id, sampled_at - Duration::seconds(1), 15)
        .await?;
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].active_rooms, 2);
    assert_eq!(history[0].room_limit, Some(100));
    assert_eq!(history[1].active_rooms, 3);
    assert!(history[0].sampled_at < history[1].sampled_at);

    let targets = store.list_ready_flow_metric_targets().await?;
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].service_instance_id, instance.id);

    let flash_request = json!({
        "region": "heteronet-global",
        "image": "ghcr.io/example/udp-server:v1",
        "replicas": 1,
        "cpu_millis": 500,
        "memory_mib": 512,
        "ports": [{
            "name": "game-udp",
            "protocol": "udp",
            "container_port": 7777
        }],
        "exposure": {"type": "public", "traffic_mode": "forwarded"},
        "env": {},
        "command": [],
        "args": [],
        "metadata": {}
    });
    let flash = store
        .create_service_instance(
            organization_id,
            ProjectId(project.id.0),
            membership.principal_id,
            "flash",
            "flash-service",
            flash_request.clone(),
        )
        .await?;
    let assigned_port = flash.spec["ports"][0]["service_port"]
        .as_u64()
        .ok_or("Flash service port was not assigned")?;
    assert!((30_000..=32_767).contains(&assigned_port));
    assert!(
        !store
            .mark_service_instance_ready(
                flash.id,
                "flow",
                flash.generation,
                Uuid::from_u128(43),
                json!("accepted"),
            )
            .await?
    );
    assert!(
        store
            .mark_service_instance_ready(
                flash.id,
                "flash",
                flash.generation,
                Uuid::from_u128(44),
                json!({"phase": "ready"}),
            )
            .await?
    );
    let targets = store.list_ready_flow_metric_targets().await?;
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].service_instance_id, instance.id);
    assert!(matches!(
        store
            .update_service_instance(
                organization_id,
                flash.id,
                "flow",
                membership.principal_id,
                "wrong-provider",
                json!({}),
            )
            .await,
        Err(StoreError::NotFound)
    ));
    let mut updated_flash_request = flash_request.clone();
    updated_flash_request["replicas"] = json!(2);
    let flash = store
        .update_service_instance(
            organization_id,
            flash.id,
            "flash",
            membership.principal_id,
            "flash-service-updated",
            updated_flash_request,
        )
        .await?;
    assert_eq!(flash.provider, "flash");
    assert_eq!(flash.spec["replicas"], json!(2));
    assert_eq!(flash.spec["ports"][0]["service_port"], json!(assigned_port));

    let second_flash = store
        .create_service_instance(
            organization_id,
            ProjectId(project.id.0),
            membership.principal_id,
            "flash",
            "second-flash-service",
            flash_request.clone(),
        )
        .await?;
    assert_ne!(
        second_flash.spec["ports"][0]["service_port"],
        flash.spec["ports"][0]["service_port"]
    );

    let mut over_quota = flash_request;
    over_quota["replicas"] = json!(6);
    over_quota["cpu_millis"] = json!(4_000);
    assert!(matches!(
        store
            .create_service_instance(
                organization_id,
                ProjectId(project.id.0),
                membership.principal_id,
                "flash",
                "over-quota",
                over_quota,
            )
            .await,
        Err(StoreError::RequestRejected(message)) if message.contains("CPU limit")
    ));

    let mut over_memory_quota = json!({
        "region": "heteronet-global",
        "image": "ghcr.io/example/memory-server:v1",
        "replicas": 5,
        "cpu_millis": 500,
        "memory_mib": 8_128,
        "ports": [{
            "name": "memory-udp",
            "protocol": "udp",
            "container_port": 7778
        }],
        "exposure": {"type": "public", "traffic_mode": "forwarded"},
        "env": {},
        "command": [],
        "args": [],
        "metadata": {}
    });
    over_memory_quota["ports"][0]["service_port"] = json!(1);
    assert!(matches!(
        store
            .create_service_instance(
                organization_id,
                ProjectId(project.id.0),
                membership.principal_id,
                "flash",
                "over-memory-quota",
                over_memory_quota,
            )
            .await,
        Err(StoreError::RequestRejected(message)) if message.contains("memory limit")
    ));
    assert!(matches!(
        store
            .begin_delete_service_instance(
                organization_id,
                flash.id,
                "flow",
                membership.principal_id,
            )
            .await,
        Err(StoreError::NotFound)
    ));
    let deleting = store
        .begin_delete_service_instance(organization_id, flash.id, "flash", membership.principal_id)
        .await?;
    assert!(
        !store
            .complete_delete_service_instance(deleting.id, "flow", deleting.generation)
            .await?
    );
    assert!(
        store
            .complete_delete_service_instance(deleting.id, "flash", deleting.generation)
            .await?
    );
    Ok(())
}
