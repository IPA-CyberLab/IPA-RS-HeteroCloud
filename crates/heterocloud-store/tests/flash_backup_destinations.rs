use heterocloud_domain::OrganizationId;
use heterocloud_store::{
    BackupPolicyInput, BootstrapAdmin, NewFlashBackupDestination, Store, StoreError,
};
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn destinations_are_tenant_scoped_idempotent_and_retention_does_not_cross_targets()
-> Result<(), Box<dyn std::error::Error>> {
    let Ok(dsn) = std::env::var("HETEROCLOUD_STORE_TEST_DATABASE_URL") else {
        return Ok(());
    };
    let store = Store::connect(&dsn, 8).await?;
    let database: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(store.pool())
        .await?;
    if !database.starts_with("heterocloud_test_") {
        return Err("a disposable test database is required".into());
    }
    sqlx::raw_sql("DROP SCHEMA public CASCADE; CREATE SCHEMA public")
        .execute(store.pool())
        .await?;
    store.migrate().await?;
    let owner = store
        .bootstrap_admin(BootstrapAdmin {
            email: "backup-destination@example.test",
            display_name: "Test",
            password_hash: "test-only",
            organization_slug: "backup-destination",
            organization_name: "Backup Destination",
        })
        .await?;
    let member = owner.memberships.first().ok_or("missing owner")?;
    let org = member.organization_id;
    let principal = member.principal_id;
    let project = store
        .create_project(org, "test-project", "Test Project")
        .await?;
    let service=store.create_service_instance(org, project.id, principal, "flash", "source", json!({
        "region":"test","image":"example/app:v1","cpu_millis":100,"memory_mib":128,"replicas":1,
        "ephemeral_storage_gib":1,"exposure":{"type":"internal","traffic_mode":"forwarded"},"ports":[],"env":{},"command":[],"args":[],"metadata":{}})).await?;
    let input = NewFlashBackupDestination {
        name: "external".into(),
        request_id: Uuid::now_v7(),
        endpoint: "https://s3.example.com".into(),
        bucket: "backup-bucket".into(),
        region: "region-1".into(),
        prefix: "test".into(),
        virtual_hosted: false,
        access_key: "fake-access".into(),
        secret_key: "fake-secret".into(),
    };
    let dest = store
        .reserve_flash_backup_destination(org, principal, &input)
        .await?;
    assert_eq!(dest.state, "pending");
    assert_eq!(
        store
            .reserve_flash_backup_destination(org, principal, &input)
            .await?
            .id,
        dest.id
    );
    let mut changed = input.clone();
    changed.bucket = "other-bucket".into();
    assert!(matches!(
        store
            .reserve_flash_backup_destination(org, principal, &changed)
            .await,
        Err(StoreError::Conflict)
    ));
    let other = OrganizationId(Uuid::now_v7());
    assert!(matches!(
        store.flash_backup_destination(other, dest.id).await,
        Err(StoreError::NotFound)
    ));
    assert!(store.flash_backup_destinations(other).await?.is_empty());
    assert!(!serde_json::to_string(&dest)?.contains("fake-secret"));
    assert!(matches!(
        store
            .request_flash_backup_to(
                org,
                service.id,
                principal,
                Uuid::now_v7(),
                "pending",
                1,
                "pvc",
                None,
                service.generation,
                Some(dest.id)
            )
            .await,
        Err(StoreError::RequestRejected(_))
    ));
    store
        .update_flash_backup_destination_state(org, dest.id, true)
        .await?;
    let baseline = store
        .request_flash_backup(
            org,
            service.id,
            principal,
            Uuid::now_v7(),
            "syouyu",
            1,
            "pvc",
            None,
            service.generation,
        )
        .await?;
    let request = Uuid::now_v7();
    let backup = store
        .request_flash_backup_to(
            org,
            service.id,
            principal,
            request,
            "s3",
            1,
            "pvc",
            None,
            service.generation,
            Some(dest.id),
        )
        .await?;
    assert_eq!(backup.destination_id, Some(dest.id));
    assert!(matches!(
        store
            .request_flash_backup_to(
                org,
                service.id,
                principal,
                request,
                "s3",
                1,
                "pvc",
                None,
                service.generation,
                None
            )
            .await,
        Err(StoreError::Conflict)
    ));
    let policy = store
        .put_flash_backup_policy(
            org,
            service.id,
            principal,
            &BackupPolicyInput {
                enabled: true,
                interval_seconds: 3600,
                retention_count: 1,
                verify_restore: false,
                destination_id: Some(dest.id),
            },
        )
        .await?;
    for id in [baseline.id, backup.id] {
        store.attach_scheduled_backup(&policy, id).await?;
        store
            .update_flash_backup_status(id, "ready", json!({}))
            .await?;
    }
    sqlx::query("UPDATE flash_backups SET created_at=now()-interval '1 day' WHERE id=$1")
        .bind(baseline.id)
        .execute(store.pool())
        .await?;
    store.expire_scheduled_backups(&policy).await?;
    assert!(
        !store.flash_backup(org, baseline.id).await?.delete_requested,
        "Syouyu backup was pruned after switching to S3"
    );
    assert!(!store.flash_backup(org, backup.id).await?.delete_requested);
    Ok(())
}
