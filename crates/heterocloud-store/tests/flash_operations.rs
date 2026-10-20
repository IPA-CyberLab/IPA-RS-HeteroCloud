use chrono::Utc;
use heterocloud_domain::{OrganizationId, ServiceInstanceId};
use heterocloud_store::{BackupPolicyInput, BootstrapAdmin, Store, StoreError, rollback_spec};
use serde_json::{Value, json};
use std::{env, error::Error};
use uuid::Uuid;

#[tokio::test]
async fn revisions_observations_leases_and_retention_are_durable_and_generation_fenced()
-> Result<(), Box<dyn Error>> {
    let Ok(url) = env::var("HETEROCLOUD_STORE_TEST_DATABASE_URL") else {
        return Ok(());
    };
    let store = Store::connect(&url, 8).await?;
    let db: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(store.pool())
        .await?;
    if !db.starts_with("heterocloud_test_") {
        return Err("a disposable test database is required".into());
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
            email: "operations@example.test",
            display_name: "Operations",
            password_hash: "test-only",
            organization_slug: "operations",
            organization_name: "Operations",
        })
        .await?;
    let member = owner.memberships.first().ok_or("membership")?;
    let org = OrganizationId(member.organization_id.0);
    let project = store.create_project(org, "services", "Services").await?;
    let original=store.create_service_instance(org,project.id,member.principal_id,"flash","workspace",json!({
        "region":"test","image":"example/app:good","replicas":1,"cpu_millis":100,"memory_mib":128,"ephemeral_storage_gib":5,
        "ports":[],"exposure":{"type":"internal","traffic_mode":"forwarded"},"env":{"VALUE":"first"},"command":["sh"],"args":["-c","sleep infinity"],"metadata":{}
    })).await?;
    store
        .mark_service_instance_ready(
            original.id,
            "flash",
            1,
            Uuid::now_v7(),
            json!({"phase":"ready","observed_generation":1,"ready_replicas":0,"desired_replicas":0}),
        )
        .await?;
    assert!(
        store
            .flash_revision(org, original.id, 1)
            .await?
            .ready_at
            .is_none(),
        "zero replicas is not proof of application health"
    );
    store.record_flash_observation(&original,&json!({"phase":"provisioning","observed_generation":1,"ready_replicas":0,"desired_replicas":1})).await?;
    let activated = store.flash_revision(org, original.id, 1).await?;
    assert!(activated.activation_started_at.is_some());
    assert!(activated.ready_at.is_none());
    store.record_flash_observation(&original,&json!({"phase":"ready","observed_generation":1,"ready_replicas":1,"desired_replicas":1})).await?;
    let healthy = store.flash_revision(org, original.id, 1).await?;
    assert!(healthy.ready_at.is_some());
    store.record_flash_observation(&original,&json!({"phase":"ready","observed_generation":1,"ready_replicas":0,"desired_replicas":0})).await?;
    assert_eq!(
        store.flash_revision(org, original.id, 1).await?.ready_at,
        healthy.ready_at,
        "an already verified revision remains healthy after scaling to zero"
    );
    let mut cold_spec = original.spec.clone();
    cold_spec["autoscaling"] = json!({"min_replicas":0,"max_replicas":1,"target_cpu_utilization_percent":80,"idle_timeout_seconds":300});
    cold_spec["ports"] = json!([{"name":"http","protocol":"tcp","container_port":8080}]);
    cold_spec["exposure"] =
        json!({"type":"public","traffic_mode":"forwarded","endpoint_mode":"web"});
    let cold = store
        .create_service_instance(
            org,
            project.id,
            member.principal_id,
            "flash",
            "idle-cold",
            cold_spec,
        )
        .await?;
    store.record_flash_observation(&cold,&json!({"phase":"ready","observed_generation":1,"ready_replicas":0,"desired_replicas":0})).await?;
    let idle = store.flash_revision(org, cold.id, 1).await?;
    assert!(idle.ready_at.is_none());
    assert!(
        idle.activation_started_at.is_none(),
        "cold-start deadline must not run while idle"
    );
    let mut changed = original.spec.clone();
    changed["image"] = json!("example/app:bad");
    changed["env"] = json!({"VALUE":"current"});
    changed["ephemeral_storage_gib"] = json!(8);
    let update = store
        .update_service_instance_checked(
            org,
            original.id,
            "flash",
            member.principal_id,
            None,
            "workspace",
            changed.clone(),
            Some(1),
        )
        .await?;
    let revisions = store.flash_revisions(org, original.id).await?;
    assert_eq!(revisions.len(), 2);
    assert_eq!(revisions[1].phase, "ready");
    assert!(revisions[1].ready_at.is_some());
    assert_eq!(revisions[0].principal_id, Some(member.principal_id.0));
    assert!(
        !serde_json::to_string(&revisions)?.contains("first"),
        "history listing must not return old environment"
    );
    let stale = store
        .update_service_instance_checked(
            org,
            original.id,
            "flash",
            member.principal_id,
            None,
            "stale",
            original.spec.clone(),
            Some(1),
        )
        .await;
    assert!(matches!(stale, Err(StoreError::Conflict)));
    let stale_observation = json!({"phase":"error","observed_generation":1,"diagnostics":[{"code":"disk_full","severity":"error","message":"Disk full."}]});
    store
        .record_flash_observation(&original, &stale_observation)
        .await?;
    assert!(store.flash_events(org, Some(original.id)).await?.is_empty());
    let status = json!({"phase":"error","observed_generation":2,"diagnostics":[{"code":"oom_killed","severity":"error","message":"Memory limit exceeded."}]});
    store.record_flash_observation(&update, &status).await?;
    store.record_flash_observation(&update, &status).await?;
    assert_eq!(store.flash_events(org, Some(original.id)).await?.len(), 1);
    let recovered = json!({"phase":"ready","observed_generation":2,"diagnostics":[]});
    store.record_flash_observation(&update, &recovered).await?;
    assert!(
        store.flash_events(org, Some(original.id)).await?[0]
            .resolved_at
            .is_some()
    );
    let target = store.flash_revision(org, original.id, 1).await?;
    let reverted = store
        .update_service_instance_checked(
            org,
            original.id,
            "flash",
            member.principal_id,
            None,
            "workspace",
            rollback_spec(&target.spec, &changed),
            Some(2),
        )
        .await?;
    assert_eq!(reverted.generation, 3);
    assert_eq!(reverted.spec["image"], "example/app:good");
    assert_eq!(reverted.spec["env"], changed["env"]);
    assert_eq!(reverted.spec["ephemeral_storage_gib"], 8);
    let (a, b) = tokio::join!(
        store.update_service_instance_checked(
            org,
            original.id,
            "flash",
            member.principal_id,
            None,
            "first-wins",
            reverted.spec.clone(),
            Some(3)
        ),
        store.update_service_instance_checked(
            org,
            original.id,
            "flash",
            member.principal_id,
            None,
            "second-wins",
            reverted.spec.clone(),
            Some(3)
        )
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    let current = store
        .service_instance(original.id)
        .await?
        .ok_or("service")?;
    assert_eq!(current.generation, 4);
    let input = BackupPolicyInput {
        enabled: true,
        interval_seconds: 3600,
        retention_count: 1,
        verify_restore: true,
    };
    let policy = store
        .put_flash_backup_policy(org, original.id, member.principal_id, &input)
        .await?;
    let (a, b) = tokio::join!(store.claim_backup_policies(), store.claim_backup_policies());
    let claimed = a?.into_iter().chain(b?).collect::<Vec<_>>();
    assert_eq!(
        claimed.len(),
        1,
        "two API replicas must not claim the same schedule"
    );
    store
        .finish_backup_policy(&claimed[0], None, Some("backup_source_unavailable"))
        .await?;
    assert_eq!(
        store
            .flash_backup_policy(org, original.id)
            .await?
            .ok_or("policy")?
            .last_error
            .as_deref(),
        Some("backup_source_unavailable")
    );
    assert!(
        store
            .flash_backup_policy(OrganizationId(Uuid::now_v7()), original.id)
            .await?
            .is_none()
    );
    let mut ids = Vec::new();
    for index in 0..4 {
        let backup = store
            .request_flash_backup(
                org,
                original.id,
                member.principal_id,
                Uuid::now_v7(),
                &format!("backup-{index}"),
                1024 * 1024,
                "test-pvc",
                None,
                current.generation,
            )
            .await?;
        if index > 0 {
            store.attach_scheduled_backup(&policy, backup.id).await?;
        }
        let verified = index < 3;
        store
            .update_flash_backup_status(
                backup.id,
                "ready",
                json!({"restore_verification":{"phase":if verified {"passed"} else {"restoring"}}}),
            )
            .await?;
        ids.push(backup.id);
    }
    store.expire_scheduled_backups(&policy).await?;
    assert!(
        !store.flash_backup(org, ids[0]).await?.delete_requested,
        "manual backups are never pruned by a policy"
    );
    assert!(store.flash_backup(org, ids[1]).await?.delete_requested);
    assert!(
        !store.flash_backup(org, ids[2]).await?.delete_requested,
        "keep last verified backup until new verification succeeds"
    );
    assert!(!store.flash_backup(org, ids[3]).await?.delete_requested);
    store
        .update_flash_backup_status(
            ids[3],
            "ready",
            json!({"restore_verification":{"phase":"passed"}}),
        )
        .await?;
    store.expire_scheduled_backups(&policy).await?;
    assert!(store.flash_backup(org, ids[2]).await?.delete_requested);
    assert!(!store.flash_backup(org, ids[3]).await?.delete_requested);
    store.run_flash_backup_policy_now(org, original.id).await?;
    let claim = store.claim_backup_policies().await?;
    assert_eq!(claim.len(), 1);
    assert!(claim[0].next_run_at <= Utc::now());
    assert!(
        store
            .run_flash_backup_policy_now(org, original.id)
            .await
            .is_err(),
        "cannot interrupt an active scheduling lease"
    );
    let changed = store
        .put_flash_backup_policy(
            org,
            original.id,
            member.principal_id,
            &BackupPolicyInput {
                enabled: false,
                ..input.clone()
            },
        )
        .await?;
    store
        .finish_backup_policy(&claim[0], None, Some("stale_worker_error"))
        .await?;
    let after = store
        .flash_backup_policy(org, original.id)
        .await?
        .ok_or("policy")?;
    assert!(!after.enabled);
    assert_eq!(
        after.last_error, changed.last_error,
        "a stale worker cannot overwrite edited policy results"
    );
    let other = store
        .flash_events(
            OrganizationId(Uuid::now_v7()),
            Some(ServiceInstanceId(original.id.0)),
        )
        .await?;
    assert!(other.is_empty());
    let _: Value = serde_json::to_value(policy)?;
    Ok(())
}
