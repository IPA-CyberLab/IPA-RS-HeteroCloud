use std::{env, error::Error};

use heterocloud_domain::{OrganizationId, ProjectId, ServiceInstanceId, ServiceState};
use heterocloud_store::{BootstrapAdmin, Store};
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

    let non_flow = store
        .create_service_instance(
            organization_id,
            ProjectId(project.id.0),
            membership.principal_id,
            "other",
            "other-service",
            json!({}),
        )
        .await?;
    assert!(
        !store
            .mark_service_instance_ready(
                non_flow.id,
                non_flow.generation,
                Uuid::from_u128(43),
                json!("accepted"),
            )
            .await?
    );
    Ok(())
}
