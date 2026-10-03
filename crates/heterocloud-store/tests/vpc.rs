use heterocloud_domain::{OrganizationId, ServiceInstanceId};
use heterocloud_store::{BootstrapAdmin, Store, StoreError};
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn vpc_isolation_lifecycle_and_concurrent_attachment()
-> Result<(), Box<dyn std::error::Error>> {
    let Ok(dsn) = std::env::var("HETEROCLOUD_STORE_TEST_DATABASE_URL") else {
        return Ok(());
    };
    let store = Store::connect(&dsn, 6).await?;
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
            email: "vpc-owner@example.test",
            display_name: "VPC owner",
            password_hash: "test-password-hash",
            organization_slug: "vpc-lifecycle-test",
            organization_name: "VPC lifecycle test",
        })
        .await?;
    let member = owner.memberships.first().ok_or("missing membership")?;
    let org = member.organization_id;
    let principal = member.principal_id;
    let project = store.create_project(org, "vpc-one", "VPC one").await?;
    let second = store.create_project(org, "vpc-two", "VPC two").await?;
    let spec = json!({"region":"test","security_groups":["parent","child"]});
    let v = store
        .create_service_instance(org, project.id, principal, "vpc", "private", spec.clone())
        .await?;
    assert_eq!(v.spec["nat"]["enabled"], false);
    assert_eq!(v.spec["rules"], json!([]));
    let flash = |id: Uuid, name: &str| json!({"region":"test","image":"docker.io/library/nginx:alpine","replicas":1,"cpu_millis":100,"memory_mib":64,"ephemeral_storage_gib":1,"env":{},"command":[],"args":[],"metadata":{},"ports":[{"name":"http","protocol":"tcp","container_port":8080}],"exposure":{"type":"internal","traffic_mode":"forwarded"},"network":{"vpc_id":id,"security_groups":["child"],"private_name":name}});
    assert!(
        store
            .create_service_instance(
                org,
                second.id,
                principal,
                "flash",
                "cross-project",
                flash(v.id.0, "child")
            )
            .await
            .is_err()
    );
    assert!(
        store
            .create_service_instance(
                OrganizationId(Uuid::now_v7()),
                project.id,
                principal,
                "flash",
                "cross-org",
                flash(v.id.0, "child")
            )
            .await
            .is_err()
    );
    let child = store
        .create_service_instance(
            org,
            project.id,
            principal,
            "flash",
            "child",
            flash(v.id.0, "child"),
        )
        .await?;
    assert_eq!(
        child.spec["ports"][0]["service_port"], 8080,
        "private ports must not consume public allocations"
    );
    assert!(
        store
            .create_service_instance(
                org,
                project.id,
                principal,
                "flash",
                "duplicate",
                flash(v.id.0, "child")
            )
            .await
            .is_err()
    );
    let mut missing_group = flash(v.id.0, "other");
    missing_group["network"]["security_groups"] = json!(["missing"]);
    assert!(
        store
            .create_service_instance(
                org,
                project.id,
                principal,
                "flash",
                "missing-group",
                missing_group
            )
            .await
            .is_err()
    );
    assert!(
        store
            .begin_delete_service_instance(org, v.id, "vpc", principal)
            .await
            .is_err()
    );
    assert!(
        store
            .update_service_instance(
                org,
                v.id,
                "vpc",
                principal,
                "private",
                json!({"region":"test","security_groups":["parent"]})
            )
            .await
            .is_err()
    );
    let other = store
        .create_service_instance(org, project.id, principal, "vpc", "other", spec.clone())
        .await?;
    let foreign = store
        .create_service_instance(
            org,
            project.id,
            principal,
            "flash",
            "foreign",
            flash(other.id.0, "child"),
        )
        .await?;
    let rule = |target: Uuid| json!({"source":{"type":"security_group","name":"parent"},"destination":{"type":"service","service_id":target},"protocol":"tcp","port":8080});
    let mut rules = spec.clone();
    rules["rules"] = json!([rule(foreign.id.0)]);
    assert!(
        store
            .update_service_instance(org, v.id, "vpc", principal, "private", rules.clone())
            .await
            .is_err()
    );
    rules["rules"] = json!([rule(child.id.0)]);
    store
        .update_service_instance(org, v.id, "vpc", principal, "private", rules)
        .await?;
    // Deleting Flash is still an attachment until the provider has removed it.
    store
        .begin_delete_service_instance(org, child.id, "flash", principal)
        .await?;
    assert!(
        store
            .begin_delete_service_instance(org, v.id, "vpc", principal)
            .await
            .is_err()
    );
    // Attach/delete race: the tenant transaction lock must make exactly one win.
    let racing = store
        .create_service_instance(org, project.id, principal, "vpc", "race", spec)
        .await?;
    let (attach, delete) = tokio::join!(
        store.create_service_instance(
            org,
            project.id,
            principal,
            "flash",
            "racing-child",
            flash(racing.id.0, "race")
        ),
        store.begin_delete_service_instance(org, racing.id, "vpc", principal)
    );
    assert_ne!(attach.is_ok(), delete.is_ok());
    let current = store
        .service_instance(ServiceInstanceId(racing.id.0))
        .await?
        .ok_or("race VPC missing")?;
    if attach.is_ok() {
        assert_ne!(
            serde_json::to_value(current.state)?,
            Value::String("deleting".into())
        );
    } else {
        assert!(matches!(attach, Err(StoreError::Conflict)));
    }
    Ok(())
}
