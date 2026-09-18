use std::{env, error::Error};

use heterocloud_domain::{OrganizationId, ProjectId};
use heterocloud_store::{BootstrapAdmin, GpuCatalogDevice, GpuVisibility, Store, StoreError};
use serde_json::{Value, json};

const TEST_DATABASE_ENV: &str = "HETEROCLOUD_STORE_TEST_DATABASE_URL";
const GPU_TYPE: &str = "nvidia-geforce-gtx-1080-ti";

#[tokio::test]
async fn gpu_visibility_dynamic_availability_and_revocation_are_enforced()
-> Result<(), Box<dyn Error>> {
    let Some(store) = test_store().await? else {
        return Ok(());
    };
    let alice = store
        .bootstrap_admin(BootstrapAdmin {
            email: "gpu-alice@example.test",
            display_name: "GPU Alice",
            password_hash: "test-password-hash",
            organization_slug: "gpu-alice",
            organization_name: "GPU Alice",
        })
        .await?;
    let bob = store
        .bootstrap_admin(BootstrapAdmin {
            email: "gpu-bob@example.test",
            display_name: "GPU Bob",
            password_hash: "test-password-hash",
            organization_slug: "gpu-bob",
            organization_name: "GPU Bob",
        })
        .await?;
    let alice_membership = alice
        .memberships
        .first()
        .ok_or("missing Alice membership")?;
    let bob_membership = bob.memberships.first().ok_or("missing Bob membership")?;
    let alice_organization = OrganizationId(alice_membership.organization_id.0);
    let bob_organization = OrganizationId(bob_membership.organization_id.0);
    let alice_project = store
        .create_project(alice_organization, "gpu-jobs", "GPU Jobs")
        .await?;
    let bob_project = store
        .create_project(bob_organization, "gpu-jobs", "GPU Jobs")
        .await?;

    store
        .sync_gpu_catalog(&[
            gpu("uc-k8sp5/GPU-a", GpuVisibility::Open, vec![]),
            gpu(
                "uc-k8sp5/GPU-b",
                GpuVisibility::Private,
                vec![alice.user.id.0],
            ),
            GpuCatalogDevice {
                management_id: "worker/GPU-t4".into(),
                gpu_type: "nvidia-tesla-t4".into(),
                display_name: "NVIDIA Tesla T4".into(),
                available: true,
                visibility: GpuVisibility::Private,
                assigned_user_ids: vec![alice.user.id.0],
            },
        ])
        .await?;

    let alice_types = store.list_accessible_gpu_types(Some(alice.user.id)).await?;
    let gtx = alice_types
        .iter()
        .find(|item| item.gpu_type == GPU_TYPE)
        .ok_or("Alice cannot see GTX inventory")?;
    assert_eq!(gtx.access, GpuVisibility::Open);
    assert_eq!((gtx.total, gtx.available), (2, 2));
    assert!(
        alice_types
            .iter()
            .any(|item| item.gpu_type == "nvidia-tesla-t4")
    );
    let bob_types = store.list_accessible_gpu_types(Some(bob.user.id)).await?;
    assert_eq!(bob_types.len(), 1);
    assert_eq!(bob_types[0].gpu_type, GPU_TYPE);
    assert_eq!((bob_types[0].total, bob_types[0].available), (1, 1));

    let service_account = store
        .create_service_account(alice_organization, "gpu-runner")
        .await?;
    assert_eq!(store.principal_user_id(service_account.id).await?, None);
    assert!(matches!(
        store
            .create_service_instance(
                alice_organization,
                ProjectId(alice_project.id.0),
                service_account.id,
                "flash",
                "service-account-private-gpu",
                flash_spec("nvidia-tesla-t4"),
            )
            .await,
        Err(StoreError::RequestRejected(message)) if message.contains("not accessible")
    ));
    let service_account_service = store
        .create_service_instance(
            alice_organization,
            ProjectId(alice_project.id.0),
            service_account.id,
            "flash",
            "service-account-open-gpu",
            flash_spec(GPU_TYPE),
        )
        .await?;
    let service_account_requester: (Option<uuid::Uuid>, Option<uuid::Uuid>) = sqlx::query_as(
        "SELECT requested_by_principal_id, requested_by_user_id
         FROM flash_gpu_service_requests WHERE service_instance_id = $1",
    )
    .bind(service_account_service.id.0)
    .fetch_one(store.pool())
    .await?;
    assert_eq!(service_account_requester.0, Some(service_account.id.0));
    assert_eq!(service_account_requester.1, None);

    let alice_service = store
        .create_service_instance(
            alice_organization,
            ProjectId(alice_project.id.0),
            alice_membership.principal_id,
            "flash",
            "alice-gpu-job",
            flash_spec("nvidia-tesla-t4"),
        )
        .await?;
    assert_eq!(alice_service.spec["gpu_type"], "nvidia-tesla-t4");
    let requester: (Option<uuid::Uuid>, Option<uuid::Uuid>) = sqlx::query_as(
        "SELECT requested_by_principal_id, requested_by_user_id
         FROM flash_gpu_service_requests WHERE service_instance_id = $1",
    )
    .bind(alice_service.id.0)
    .fetch_one(store.pool())
    .await?;
    assert_eq!(requester.0, Some(alice_membership.principal_id.0));
    assert_eq!(requester.1, Some(alice.user.id.0));
    assert_ne!(requester.0, requester.1);
    assert_eq!(
        store
            .list_accessible_gpu_types(Some(alice.user.id))
            .await?
            .iter()
            .find(|item| item.gpu_type == "nvidia-tesla-t4")
            .ok_or("missing T4 after service creation")?
            .available,
        1
    );
    store
        .sync_gpu_catalog(&[
            gpu("uc-k8sp5/GPU-a", GpuVisibility::Open, vec![]),
            gpu(
                "uc-k8sp5/GPU-b",
                GpuVisibility::Private,
                vec![alice.user.id.0],
            ),
            GpuCatalogDevice {
                management_id: "worker/GPU-t4".into(),
                gpu_type: "nvidia-tesla-t4".into(),
                display_name: "NVIDIA Tesla T4".into(),
                available: false,
                visibility: GpuVisibility::Private,
                assigned_user_ids: vec![alice.user.id.0],
            },
        ])
        .await?;
    assert_eq!(
        store
            .list_accessible_gpu_types(Some(alice.user.id))
            .await?
            .iter()
            .find(|item| item.gpu_type == "nvidia-tesla-t4")
            .ok_or("missing T4 after reservation")?
            .available,
        0
    );

    store
        .sync_gpu_catalog(&[
            gpu("uc-k8sp5/GPU-a", GpuVisibility::Open, vec![]),
            gpu(
                "uc-k8sp5/GPU-b",
                GpuVisibility::Private,
                vec![bob.user.id.0],
            ),
            GpuCatalogDevice {
                management_id: "worker/GPU-t4".into(),
                gpu_type: "nvidia-tesla-t4".into(),
                display_name: "NVIDIA Tesla T4".into(),
                available: true,
                visibility: GpuVisibility::Private,
                assigned_user_ids: vec![],
            },
        ])
        .await?;
    assert!(
        store
            .list_accessible_gpu_types(Some(alice.user.id))
            .await?
            .iter()
            .all(|item| item.gpu_type != "nvidia-tesla-t4")
    );
    assert!(matches!(
        store
            .update_service_instance(
                alice_organization,
                alice_service.id,
                "flash",
                alice_membership.principal_id,
                "alice-gpu-job",
                flash_spec("nvidia-tesla-t4"),
            )
            .await,
        Err(StoreError::RequestRejected(message)) if message.contains("not accessible")
    ));
    let reconcile_events: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_events
         WHERE aggregate_id = $1 AND topic = 'service-instance.reconcile'",
    )
    .bind(alice_service.id.0)
    .fetch_one(store.pool())
    .await?;
    assert_eq!(reconcile_events, 2);

    let deleting = store
        .begin_delete_service_instance(
            alice_organization,
            alice_service.id,
            "flash",
            alice_membership.principal_id,
        )
        .await?;
    assert!(
        store
            .complete_delete_service_instance(deleting.id, "flash", deleting.generation)
            .await?
    );
    store
        .sync_gpu_catalog(&[
            gpu("uc-k8sp5/GPU-a", GpuVisibility::Open, vec![]),
            gpu("uc-k8sp5/GPU-b", GpuVisibility::Private, vec![]),
        ])
        .await?;
    let first = store.create_service_instance(
        alice_organization,
        alice_project.id,
        alice_membership.principal_id,
        "flash",
        "capacity-race-a",
        flash_spec(GPU_TYPE),
    );
    let second = store.create_service_instance(
        bob_organization,
        bob_project.id,
        bob_membership.principal_id,
        "flash",
        "capacity-race-b",
        flash_spec(GPU_TYPE),
    );
    let results = tokio::join!(first, second);
    assert!(results.0.is_ok());
    assert!(results.1.is_ok());
    Ok(())
}

fn gpu(
    management_id: &str,
    visibility: GpuVisibility,
    assigned_user_ids: Vec<uuid::Uuid>,
) -> GpuCatalogDevice {
    GpuCatalogDevice {
        management_id: management_id.into(),
        gpu_type: GPU_TYPE.into(),
        display_name: "NVIDIA GeForce GTX 1080 Ti".into(),
        available: true,
        visibility,
        assigned_user_ids,
    }
}

fn flash_spec(gpu_type: &str) -> Value {
    json!({
        "region": "heteronet-global",
        "image": "ghcr.io/example/gpu-job:v1",
        "replicas": 1,
        "cpu_millis": 500,
        "memory_mib": 512,
        "gpu_type": gpu_type,
        "ephemeral_storage_gib": 10,
        "ports": [{
            "name": "http",
            "protocol": "tcp",
            "container_port": 8080
        }],
        "exposure": {"type": "public", "traffic_mode": "forwarded"},
        "env": {},
        "command": [],
        "args": [],
        "metadata": {}
    })
}

async fn test_store() -> Result<Option<Store>, Box<dyn Error>> {
    let Ok(database_url) = env::var(TEST_DATABASE_ENV) else {
        return Ok(None);
    };
    let store = Store::connect(&database_url, 8).await?;
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
    Ok(Some(store))
}
