use heterocloud_domain::OrganizationId;
use heterocloud_store::{BootstrapAdmin, Store, StoreError};
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn domain_reservations_are_unique_scoped_and_do_not_enqueue_workloads()
-> Result<(), Box<dyn std::error::Error>> {
    let Ok(dsn) = std::env::var("HETEROCLOUD_STORE_TEST_DATABASE_URL") else {
        return Ok(());
    };
    let store = Store::connect(&dsn, 8).await?;
    let database: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(store.pool())
        .await?;
    if !database.starts_with("heterocloud_test_") {
        return Err("requires a disposable heterocloud_test_ database".into());
    }
    sqlx::raw_sql("DROP SCHEMA public CASCADE; CREATE SCHEMA public")
        .execute(store.pool())
        .await?;
    store.migrate().await?;
    let owner = store
        .bootstrap_admin(BootstrapAdmin {
            email: "domains@example.test",
            display_name: "Domains",
            password_hash: "test-only",
            organization_slug: "domains-test",
            organization_name: "Domains",
        })
        .await?;
    let member = owner.memberships.first().ok_or("membership")?;
    let org = member.organization_id;
    let principal = member.principal_id;
    let project = store
        .create_project(org, "domain-test", "Domain test")
        .await?;
    let spec = json!({"region":"test","image":"example/app:v1","replicas":1,"cpu_millis":100,"memory_mib":128,"ephemeral_storage_gib":1,"env":{},"command":[],"args":[],"metadata":{},"ports":[{"name":"http","protocol":"tcp","container_port":8080}],"exposure":{"type":"public","traffic_mode":"forwarded","endpoint_mode":"web"}});
    let a = store
        .create_service_instance(
            org,
            project.id,
            principal,
            "flash",
            "domain-a",
            spec.clone(),
        )
        .await?;
    let b = store
        .create_service_instance(
            org,
            project.id,
            principal,
            "flash",
            "domain-b",
            spec.clone(),
        )
        .await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM outbox_events")
        .fetch_one(store.pool())
        .await?;
    let binding = store
        .reserve_flash_domain(org, a.id, principal, "app.example.org")
        .await?;
    assert_eq!(
        binding.id,
        store
            .reserve_flash_domain(org, a.id, principal, "app.example.org")
            .await?
            .id
    );
    assert!(matches!(
        store
            .reserve_flash_domain(org, b.id, principal, "app.example.org")
            .await,
        Err(StoreError::AlreadyExists)
    ));
    assert!(
        store
            .flash_domain_bindings(OrganizationId(Uuid::now_v7()), a.id)
            .await?
            .is_empty()
    );
    assert!(matches!(
        store
            .request_flash_domain_delete(org, b.id, binding.id)
            .await,
        Err(StoreError::NotFound)
    ));
    let stored = store.service_instance(a.id).await?.ok_or("service")?;
    assert_eq!(stored.spec, a.spec);
    assert_eq!(stored.generation, a.generation);
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM outbox_events")
        .fetch_one(store.pool())
        .await?;
    assert_eq!(
        count, after,
        "a domain binding must never enqueue a Pod update"
    );
    store.release_flash_domain(binding.id).await?;
    assert_eq!(
        store.flash_domain_bindings(org, a.id).await?.len(),
        1,
        "active reservation cannot be released"
    );
    store
        .request_flash_domain_delete(org, a.id, binding.id)
        .await?;
    store.mark_flash_domain_synced(binding.id).await?;
    assert!(store.flash_domain_bindings(org, a.id).await?[0].reconcile_pending);
    assert!(matches!(
        store
            .reserve_flash_domain(org, b.id, principal, "app.example.org")
            .await,
        Err(StoreError::AlreadyExists)
    ));
    store.release_flash_domain(binding.id).await?;
    assert!(
        store
            .reserve_flash_domain(org, b.id, principal, "app.example.org")
            .await
            .is_ok()
    );
    let mut private = spec;
    private["exposure"]["type"] = json!("internal");
    private["exposure"]
        .as_object_mut()
        .ok_or("exposure")?
        .remove("endpoint_mode");
    let internal = store
        .create_service_instance(org, project.id, principal, "flash", "private", private)
        .await?;
    assert!(matches!(
        store
            .reserve_flash_domain(org, internal.id, principal, "private.example.org")
            .await,
        Err(StoreError::RequestRejected(_))
    ));
    Ok(())
}
