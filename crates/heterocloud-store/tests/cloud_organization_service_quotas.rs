use std::{env, error::Error, time::Duration};

use heterocloud_domain::{OrganizationId, ResourceQuotaLimits};
use heterocloud_store::{BootstrapAdmin, Store, StoreError};
use serde_json::json;

#[tokio::test]
async fn organization_allocation_is_shared_across_accounts_and_serialized()
-> Result<(), Box<dyn Error>> {
    let Ok(url) = env::var("HETEROCLOUD_TEST_DATABASE_URL") else {
        return Ok(());
    };
    let store = Store::connect(&url, 8).await?;
    let database: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(store.pool())
        .await?;
    if !database.starts_with("heterocloud_test_") {
        return Err("a disposable heterocloud_test_ database is required".into());
    }
    sqlx::raw_sql("DROP SCHEMA public CASCADE; CREATE SCHEMA public;")
        .execute(store.pool())
        .await?;
    store.migrate().await?;
    let owner = store
        .bootstrap_admin(BootstrapAdmin {
            email: "cloud-quota@example.test",
            display_name: "Owner",
            password_hash: "test-only",
            organization_slug: "quota-root",
            organization_name: "Quota root",
        })
        .await?;
    let management = owner.memberships[0].organization_id;
    let cloud = store
        .cloud_organization_for_account(management)
        .await?
        .ok_or("cloud org")?;
    let first = store
        .create_cloud_member_account(
            management,
            cloud.root_id,
            owner.user.id,
            "quota-first",
            "First",
        )
        .await?;
    let second = store
        .create_cloud_member_account(
            management,
            cloud.root_id,
            owner.user.id,
            "quota-second",
            "Second",
        )
        .await?;
    let membership = store.session_user(owner.user.id).await?.ok_or("owner")?;
    let first_principal = membership
        .memberships
        .iter()
        .find(|m| m.organization_id == first.id)
        .ok_or("first principal")?
        .principal_id;
    let second_principal = membership
        .memberships
        .iter()
        .find(|m| m.organization_id == second.id)
        .ok_or("second principal")?
        .principal_id;
    let first_project = store.create_project(first.id, "main", "Main").await?;
    let second_project = store.create_project(second.id, "main", "Main").await?;
    let listed = store.list_resource_quota_tenants().await?;
    for account in [management, first.id, second.id] {
        let row = listed
            .iter()
            .find(|row| row.organization.id == account)
            .ok_or("quota account")?;
        assert_eq!(row.cloud_organization_id, cloud.id);
        assert_eq!(row.management_account_id, management.0);
    }
    let mut limits = ResourceQuotaLimits::default();
    limits.flow.max_services = 1;
    limits.flow.max_rooms_per_service = 50;
    limits.flow.max_total_rooms = 50;
    limits.flash.max_replicas_per_service = 1;
    limits.flash.max_memory_mib_per_vm = 128;
    limits.flash.max_total_memory_mib = 128;
    limits.syouyu.max_buckets = 1;
    limits.registry.max_credentials = 1;
    store
        .set_organization_resource_quota(management, &limits)
        .await?;
    assert!(store.cloud_flash_weekly_quota(first.id).await?.is_none());
    assert_eq!(store.enable_organization_flash_weekly_quotas().await?, 0);
    let (accounts, weekly_limits) = store
        .cloud_flash_weekly_quota(first.id)
        .await?
        .ok_or("weekly policy")?;
    assert_eq!(accounts.len(), 3);
    assert!(accounts.contains(&management));
    assert!(accounts.contains(&first.id));
    assert!(accounts.contains(&second.id));
    assert_eq!(
        weekly_limits.flash.max_weekly_cpu_millicore_seconds,
        limits.flash.max_weekly_cpu_millicore_seconds
    );

    assert!(!store.organization_registry_credentials_enabled().await?);
    let first_reservation = store
        .reserve_registry_credential(first.id, first_principal, "before-first")
        .await?;
    let second_reservation = store
        .reserve_registry_credential(second.id, second_principal, "before-second")
        .await?;
    store
        .cancel_registry_credential_reservation(first.id, first_reservation.id)
        .await?;
    store
        .cancel_registry_credential_reservation(second.id, second_reservation.id)
        .await?;
    // Activation cannot race a reservation that started under the old gate.
    let mut in_flight = store.pool().begin().await?;
    sqlx::query(
        "SELECT pg_advisory_xact_lock_shared(hashtextextended('heterocloud-quota-defaults', 0))",
    )
    .execute(&mut *in_flight)
    .await?;
    let activation_store = store.clone();
    let mut activation = tokio::spawn(async move {
        activation_store
            .enable_organization_registry_credentials()
            .await
    });
    assert!(
        tokio::time::timeout(Duration::from_millis(150), &mut activation)
            .await
            .is_err()
    );
    in_flight.commit().await?;
    tokio::time::timeout(Duration::from_secs(5), activation).await???;
    store.enable_organization_registry_credentials().await?;
    assert!(store.organization_registry_credentials_enabled().await?);
    let (one, two) = tokio::join!(
        store.reserve_registry_credential(first.id, first_principal, "concurrent-first"),
        store.reserve_registry_credential(second.id, second_principal, "concurrent-second"),
    );
    let (winner_account, winner_reservation, loser_account, loser_principal) = match (one, two) {
        (Ok(record), Err(StoreError::RequestRejected(message))) => {
            assert!(message.contains("cloud organization registry credential limit"));
            (first.id, record, second.id, second_principal)
        }
        (Err(StoreError::RequestRejected(message)), Ok(record)) => {
            assert!(message.contains("cloud organization registry credential limit"));
            (second.id, record, first.id, first_principal)
        }
        other => return Err(format!("unexpected registry reservations: {other:?}").into()),
    };
    store
        .cancel_registry_credential_reservation(winner_account, winner_reservation.id)
        .await?;
    let replacement = store
        .reserve_registry_credential(loser_account, loser_principal, "after-release")
        .await?;
    store
        .cancel_registry_credential_reservation(loser_account, replacement.id)
        .await?;
    let enabled: bool = sqlx::query_scalar(
        "SELECT organization_service_quotas_enabled FROM cloud_security_activation WHERE singleton",
    )
    .fetch_one(store.pool())
    .await?;
    assert!(
        !enabled,
        "mixed-version fleets must not activate cross-account limits"
    );
    sqlx::query("UPDATE cloud_security_activation SET organization_service_quotas_enabled = true WHERE singleton")
        .execute(store.pool()).await?;

    // Flow has no global Flash port lock: only the new organization lock
    // prevents both accounts from simultaneously spending the same slot.
    let flow =
        json!({"region":"heteronet-global", "max_participants":10, "max_rooms":20, "metadata":{}});
    let (one, two) = tokio::join!(
        store.create_service_instance(
            first.id,
            first_project.id,
            first_principal,
            "flow",
            "first-flow",
            flow.clone()
        ),
        store.create_service_instance(
            second.id,
            second_project.id,
            second_principal,
            "flow",
            "second-flow",
            flow.clone()
        ),
    );
    assert_ne!(
        one.is_ok(),
        two.is_ok(),
        "only one concurrent reservation can succeed"
    );
    let (winner, account, principal) = match (one, two) {
        (Ok(instance), Err(StoreError::RequestRejected(message))) => {
            assert!(message.contains("cloud organization service quota"));
            (instance, first.id, first_principal)
        }
        (Err(StoreError::RequestRejected(message)), Ok(instance)) => {
            assert!(message.contains("cloud organization service quota"));
            (instance, second.id, second_principal)
        }
        other => return Err(format!("unexpected concurrent results: {other:?}").into()),
    };

    // Lowering a ceiling must not prevent reducing an existing allocation.
    limits.flow.max_rooms_per_service = 10;
    limits.flow.max_total_rooms = 10;
    store
        .set_organization_resource_quota(management, &limits)
        .await?;
    let mut increased = winner.spec.clone();
    increased["max_rooms"] = json!(21);
    assert!(
        matches!(store.update_service_instance(account, winner.id, "flow", principal, "increased", increased).await,
        Err(StoreError::RequestRejected(message)) if message.contains("cloud organization service quota"))
    );
    let mut reduced = winner.spec.clone();
    reduced["max_rooms"] = json!(15);
    assert!(
        store
            .update_service_instance(account, winner.id, "flow", principal, "reduced", reduced)
            .await
            .is_ok()
    );
    // A deletion request does not release reserved capacity until the
    // provider confirms deletion and the service record is actually removed.
    let (other_account, other_project, other_principal) = if account == first.id {
        (second.id, second_project.id, second_principal)
    } else {
        (first.id, first_project.id, first_principal)
    };
    let deleting = store
        .begin_delete_service_instance(account, winner.id, "flow", principal)
        .await?;
    let mut replacement = flow.clone();
    replacement["max_rooms"] = json!(10);
    assert!(
        matches!(store.create_service_instance(other_account, other_project, other_principal, "flow", "replacement", replacement.clone()).await,
        Err(StoreError::RequestRejected(message)) if message.contains("cloud organization service quota"))
    );
    assert!(
        store
            .complete_delete_service_instance(winner.id, "flow", deleting.generation)
            .await?
    );
    assert!(
        store
            .create_service_instance(
                other_account,
                other_project,
                other_principal,
                "flow",
                "replacement",
                replacement
            )
            .await
            .is_ok()
    );

    let flash = json!({"region":"test", "image":"example/workspace:v1", "replicas":1,
        "cpu_millis":100, "memory_mib":128, "ephemeral_storage_gib":5,
        "ports":[], "exposure":{"type":"internal", "traffic_mode":"forwarded"},
        "env":{}, "command":[], "args":[], "metadata":{}});
    let first_flash = store
        .create_service_instance(
            first.id,
            first_project.id,
            first_principal,
            "flash",
            "first-flash",
            flash.clone(),
        )
        .await?;
    let count_events = || async {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM outbox_events WHERE aggregate_id=$1 AND topic='service-instance.flash-quota'")
            .bind(first_flash.id.0).fetch_one(store.pool()).await
    };
    let before_account = count_events().await?;
    let third = store
        .create_cloud_member_account(
            management,
            cloud.root_id,
            owner.user.id,
            "quota-third",
            "Third",
        )
        .await?;
    assert!(
        count_events().await? > before_account,
        "adding an account must refresh existing Flash policies"
    );
    let (accounts, _) = store
        .cloud_flash_weekly_quota(first.id)
        .await?
        .ok_or("weekly policy")?;
    assert!(accounts.contains(&third.id));
    let before_limit = count_events().await?;
    limits.flash.max_weekly_gpu_seconds = 0;
    store
        .set_organization_resource_quota(management, &limits)
        .await?;
    assert!(
        count_events().await? > before_limit,
        "changing the management ceiling must refresh member-account policies"
    );
    let before_retry = count_events().await?;
    assert_eq!(store.enable_organization_flash_weekly_quotas().await?, 1);
    assert!(
        count_events().await? > before_retry,
        "activation can safely requeue after an interrupted rollout"
    );
    assert!(
        matches!(store.create_service_instance(second.id, second_project.id, second_principal, "flash", "second-flash", flash).await,
        Err(StoreError::RequestRejected(message)) if message.contains("cloud organization service quota"))
    );

    let bucket = json!({"region":"heteronet-global", "bucket_name":"cloud-quota-first",
        "quota_bytes":1048576, "quota_objects":100, "metadata":{}});
    store
        .create_service_instance(
            first.id,
            first_project.id,
            first_principal,
            "syouyu",
            "first-bucket",
            bucket.clone(),
        )
        .await?;
    let mut next_bucket = bucket;
    next_bucket["bucket_name"] = json!("cloud-quota-second");
    assert!(
        matches!(store.create_service_instance(second.id, second_project.id, second_principal, "syouyu", "second-bucket", next_bucket).await,
        Err(StoreError::RequestRejected(message)) if message.contains("cloud organization service quota"))
    );

    // A separate cloud organization has a separate quota and lock.
    let independent = store
        .bootstrap_admin(BootstrapAdmin {
            email: "independent-quota@example.test",
            display_name: "Independent",
            password_hash: "test-only",
            organization_slug: "independent-quota",
            organization_name: "Independent",
        })
        .await?;
    let own = &independent.memberships[0];
    let isolated_credential = store
        .reserve_registry_credential(own.organization_id, own.principal_id, "independent")
        .await?;
    store
        .cancel_registry_credential_reservation(own.organization_id, isolated_credential.id)
        .await?;
    let project = store
        .create_project(OrganizationId(own.organization_id.0), "main", "Main")
        .await?;
    assert!(
        store
            .create_service_instance(
                own.organization_id,
                project.id,
                own.principal_id,
                "flow",
                "independent",
                flow
            )
            .await
            .is_ok()
    );
    Ok(())
}
