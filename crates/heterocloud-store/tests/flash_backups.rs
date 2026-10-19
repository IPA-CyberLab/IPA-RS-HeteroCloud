use heterocloud_domain::{OrganizationId, ProjectId, ServiceInstanceId};
use heterocloud_store::{BootstrapAdmin, Store, StoreError};
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn backups_retain_deleted_source_data_and_enforce_scope_idempotency_quota_and_deletion()
-> Result<(), Box<dyn std::error::Error>> {
    let Ok(dsn) = std::env::var("HETEROCLOUD_STORE_TEST_DATABASE_URL") else {
        return Ok(());
    };
    let store = Store::connect(&dsn, 8).await?;
    let database: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(store.pool())
        .await?;
    if !database.starts_with("heterocloud_test_") {
        return Err("disposable heterocloud_test_ database required".into());
    }
    sqlx::raw_sql("DROP SCHEMA public CASCADE; CREATE SCHEMA public")
        .execute(store.pool())
        .await?;
    store.migrate().await?;
    let owner = store
        .bootstrap_admin(BootstrapAdmin {
            email: "backups@example.test",
            display_name: "Backups",
            password_hash: "test-only",
            organization_slug: "backups-test",
            organization_name: "Backups",
        })
        .await?;
    let member = owner.memberships.first().ok_or("membership")?;
    let org = member.organization_id;
    let principal = member.principal_id;
    let project = store.create_project(org, "backups", "Backups").await?;
    let service=store.create_service_instance(org,project.id,principal,"flash","source",json!({
        "region":"test","image":"example/app:v1","cpu_millis":100,"memory_mib":128,"replicas":1,
        "ephemeral_storage_gib":1,"env":{"PRIVATE":"must-not-copy"},"secret_env":{"TOKEN":"do-not-copy"},
        "exposure":{"type":"internal","traffic_mode":"forwarded"},"ports":[],"command":[],"args":[],"metadata":{}})).await?;
    let request = Uuid::now_v7();
    let backup = store
        .request_flash_backup(
            org,
            service.id,
            principal,
            request,
            "first",
            1024_i64.pow(3),
            "uid-first",
            Some("example/app@sha256:pinned"),
            service.generation,
        )
        .await?;
    assert_eq!(backup.source_spec["image"], "example/app@sha256:pinned");
    assert_eq!(backup.source_spec["env"], json!({}));
    assert_eq!(backup.source_spec["secret_env"], json!({}));
    assert!(
        !serde_json::to_value(&backup)?
            .to_string()
            .contains("uid-first")
    );
    assert_eq!(
        store
            .request_flash_backup(
                org,
                service.id,
                principal,
                request,
                "first",
                1024_i64.pow(3),
                "uid-first",
                None,
                service.generation
            )
            .await?
            .id,
        backup.id
    );
    assert!(matches!(
        store
            .request_flash_backup(
                org,
                service.id,
                principal,
                request,
                "changed",
                1,
                "uid-first",
                None,
                service.generation
            )
            .await,
        Err(StoreError::Conflict)
    ));
    assert!(matches!(
        store
            .request_flash_backup(
                org,
                service.id,
                principal,
                Uuid::now_v7(),
                "stale",
                1,
                "uid-first",
                None,
                service.generation + 1
            )
            .await,
        Err(StoreError::Conflict)
    ));
    let other = OrganizationId(Uuid::now_v7());
    assert!(store.list_flash_backups(other, None).await?.is_empty());
    assert!(matches!(
        store.flash_backup(other, backup.id).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store.request_flash_backup_delete(other, backup.id).await,
        Err(StoreError::NotFound)
    ));
    for i in 1..10 {
        store
            .request_flash_backup(
                org,
                service.id,
                principal,
                Uuid::now_v7(),
                &format!("backup-{i}"),
                1,
                "uid-first",
                None,
                service.generation,
            )
            .await?;
    }
    assert!(matches!(
        store
            .request_flash_backup(
                org,
                service.id,
                principal,
                Uuid::now_v7(),
                "too-many",
                1,
                "uid-first",
                None,
                service.generation
            )
            .await,
        Err(StoreError::RequestRejected(_))
    ));
    store
        .update_flash_backup_status(backup.id, "ready", json!({}))
        .await?;
    let restore_request = Uuid::now_v7();
    let target = store
        .reserve_flash_restore(
            org,
            principal,
            restore_request,
            backup.id,
            project.id,
            "restored",
        )
        .await?;
    assert_ne!(target, service.id);
    assert_eq!(
        target,
        store
            .reserve_flash_restore(
                org,
                principal,
                restore_request,
                backup.id,
                project.id,
                "restored"
            )
            .await?
    );
    assert!(matches!(
        store
            .reserve_flash_restore(
                org,
                principal,
                restore_request,
                backup.id,
                ProjectId(Uuid::now_v7()),
                "restored"
            )
            .await,
        Err(StoreError::Conflict)
    ));
    // A backup is independent of the source row, source project and its PVC.
    sqlx::query("DELETE FROM service_instances WHERE id=$1")
        .bind(service.id.0)
        .execute(store.pool())
        .await?;
    assert_eq!(
        store.flash_backup(org, backup.id).await?.source_name,
        "source"
    );
    assert!(
        store
            .flash_backup_request(org, principal, request, service.id, "first")
            .await?
            .is_some()
    );
    store.request_flash_backup_delete(org, backup.id).await?;
    assert!(matches!(
        store
            .reserve_flash_restore(
                org,
                principal,
                Uuid::now_v7(),
                backup.id,
                project.id,
                "blocked"
            )
            .await,
        Err(StoreError::Conflict)
    ));
    store
        .update_flash_backup_status(backup.id, "ready", json!({}))
        .await?;
    assert_eq!(
        store.flash_backup(org, backup.id).await?.phase,
        "deleting",
        "stale observations cannot cancel deletion"
    );
    store.release_flash_backup(backup.id).await?;
    store
        .update_flash_backup_status(backup.id, "pending", json!({}))
        .await?;
    assert!(
        matches!(
            store.flash_backup(org, backup.id).await,
            Err(StoreError::NotFound)
        ),
        "tombstones cannot be resurrected"
    );
    assert!(
        store
            .list_flash_backups(org, Some(ServiceInstanceId(Uuid::now_v7())))
            .await?
            .is_empty()
    );
    Ok(())
}
