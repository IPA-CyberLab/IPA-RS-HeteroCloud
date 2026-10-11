use std::{env, error::Error};

use heterocloud_domain::{OrganizationId, ProjectId};
use heterocloud_store::{BootstrapAdmin, Store, StoreError};
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn flash_replacement_updates_change_creation_settings_and_preserve_identity()
-> Result<(), Box<dyn Error>> {
    let Ok(url) = env::var("HETEROCLOUD_STORE_TEST_DATABASE_URL") else {
        return Ok(());
    };
    let store = Store::connect(&url, 4).await?;
    let db: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(store.pool())
        .await?;
    if !db.starts_with("heterocloud_test_") {
        return Err("a disposable heterocloud_test_ database is required".into());
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
            email: "flash-updates@example.test",
            display_name: "Flash updates",
            password_hash: "test-only",
            organization_slug: "flash-updates",
            organization_name: "Flash updates",
        })
        .await?;
    let membership = owner.memberships.first().ok_or("membership")?;
    let org = OrganizationId(membership.organization_id.0);
    let first = store.create_project(org, "first", "First").await?;
    let second = store.create_project(org, "second", "Second").await?;
    let original = store.create_service_instance(org, first.id, membership.principal_id, "flash", "workspace", json!({
        "region":"test", "image":"example/workspace:v1", "replicas":1, "cpu_millis":100,
        "memory_mib":128, "ephemeral_storage_gib":5, "rootfs_storage_gib":1, "persistent_storage_gib":2,
        "ports":[], "exposure":{"type":"internal", "traffic_mode":"forwarded"},
        "env":{"HOME":"/root"}, "command":["sh"], "args":["-c","sleep infinity"], "metadata":{}
    })).await?;
    let mut spec = original.spec.clone();
    spec["region"] = json!("updated-region");
    spec["image"] = json!("example/workspace:v2");
    spec["replicas"] = json!(2);
    spec["cpu_millis"] = json!(200);
    spec["memory_mib"] = json!(256);
    spec["ephemeral_storage_gib"] = json!(7);
    spec["rootfs_storage_gib"] = json!(2);
    spec["persistent_storage_gib"] = json!(3);
    spec["env"] = json!({"HOME":"/root", "UPDATED":"yes"});
    spec["command"] = json!(["/bin/sh"]);
    spec["args"] = json!(["-c", "exec sleep 7200"]);
    spec["metadata"] = json!({"updated":true});
    spec["ports"] = json!([{"name":"http", "protocol":"tcp", "container_port":8080}]);
    spec["exposure"] = json!({"type":"public", "traffic_mode":"forwarded", "endpoint_mode":"web"});
    spec["egress"] = json!({"mode":"disabled"});
    let updated = store
        .update_service_instance_in_project(
            org,
            original.id,
            "flash",
            membership.principal_id,
            Some(second.id),
            "updated-workspace",
            spec.clone(),
        )
        .await?;
    assert_eq!(updated.id, original.id);
    assert_eq!(updated.created_at, original.created_at);
    assert_eq!(updated.project_id, second.id);
    assert_eq!(updated.generation, original.generation + 1);
    for (key, value) in spec.as_object().ok_or("spec")? {
        if key != "ports" && key != "egress" && key != "exposure" {
            assert_eq!(&updated.spec[key], value, "{key}");
        }
    }
    assert_eq!(updated.spec["ports"][0]["container_port"], 8080);
    assert_eq!(updated.spec["exposure"]["endpoint_mode"], "web");
    let payload: Value = sqlx::query_scalar("SELECT payload FROM outbox_events WHERE aggregate_id=$1 AND (payload->>'generation')::bigint=$2").bind(updated.id.0).bind(updated.generation).fetch_one(store.pool()).await?;
    assert_eq!(payload["project_id"], json!(second.id));
    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM outbox_events")
        .fetch_one(store.pool())
        .await?;
    let mut shrink = updated.spec.clone();
    shrink["persistent_storage_gib"] = json!(2);
    assert!(
        matches!(store.update_service_instance(org, updated.id, "flash", membership.principal_id, "shrink", shrink).await, Err(StoreError::RequestRejected(message)) if message.contains("cannot be shrunk"))
    );
    assert!(matches!(
        store
            .update_service_instance_in_project(
                org,
                updated.id,
                "flash",
                membership.principal_id,
                Some(ProjectId(Uuid::new_v4())),
                "invalid-project",
                updated.spec.clone()
            )
            .await,
        Err(StoreError::NotFound)
    ));
    let unchanged = store.service_instance(updated.id).await?.ok_or("service")?;
    assert_eq!(unchanged.generation, updated.generation);
    assert_eq!(unchanged.spec, updated.spec);
    assert_eq!(unchanged.project_id, second.id);
    assert_eq!(
        before,
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM outbox_events")
            .fetch_one(store.pool())
            .await?
    );
    // The snapshot of the actual existing disk protects legacy automatic allocations too.
    store
        .mark_service_instance_ready(
            updated.id,
            "flash",
            updated.generation,
            Uuid::new_v4(),
            json!({"persistent_storage_bytes":3_u64*1024*1024*1024,"image_size_bytes":100_000_000}),
        )
        .await?;
    let mut insufficient = updated.spec.clone();
    insufficient["rootfs_storage_gib"] = json!(4);
    assert!(matches!(
        store
            .update_service_instance(
                org,
                updated.id,
                "flash",
                membership.principal_id,
                "insufficient",
                insufficient
            )
            .await,
        Err(StoreError::RequestRejected(_))
    ));
    // Removing an explicit split is a supported update rather than an immutable field.
    let mut automatic = updated.spec.clone();
    automatic
        .as_object_mut()
        .ok_or("spec")?
        .remove("rootfs_storage_gib");
    store
        .update_service_instance(
            org,
            updated.id,
            "flash",
            membership.principal_id,
            "automatic",
            automatic,
        )
        .await?;
    Ok(())
}
