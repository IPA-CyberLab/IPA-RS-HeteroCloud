use std::{env, error::Error};

use chrono::Utc;
use heterocloud_store::{
    BootstrapAdmin, NewFlashRateCard, NewFlashWeeklyBudget, Store, StoreError,
};

#[tokio::test]
async fn rates_are_future_only_and_budgets_are_organization_scoped() -> Result<(), Box<dyn Error>> {
    let Ok(url) = env::var("HETEROCLOUD_STORE_TEST_DATABASE_URL") else {
        return Ok(());
    };
    let store = Store::connect(&url, 8).await?;
    let database: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(store.pool())
        .await?;
    if !database.starts_with("heterocloud_test_") {
        return Err("a disposable test database is required".into());
    }
    sqlx::raw_sql("DROP SCHEMA public CASCADE; CREATE SCHEMA public;")
        .execute(store.pool())
        .await?;
    store.migrate().await?;
    let owner = store
        .bootstrap_admin(BootstrapAdmin {
            email: "budget@example.test",
            display_name: "Budget owner",
            password_hash: "test-only",
            organization_slug: "budget",
            organization_name: "Budget",
        })
        .await?;
    let first = owner
        .memberships
        .first()
        .ok_or("membership")?
        .organization_id;
    let second = store
        .create_organization(owner.user.id, "budget-two", "Budget Two")
        .await?
        .id;
    let input = NewFlashWeeklyBudget {
        currency: "USD".into(),
        amount_microunits: 10_000_000,
        warning_percentage: 80,
    };
    store.set_flash_weekly_budget(first, &input).await?;
    assert_eq!(
        store
            .flash_weekly_budget(first)
            .await?
            .ok_or("missing budget")?
            .amount_microunits,
        10_000_000
    );
    assert!(store.flash_weekly_budget(second).await?.is_none());
    assert!(matches!(
        store
            .set_flash_weekly_budget(
                first,
                &NewFlashWeeklyBudget {
                    amount_microunits: 0,
                    ..input.clone()
                }
            )
            .await,
        Err(StoreError::RequestRejected(_))
    ));
    assert_eq!(
        store
            .flash_weekly_budget(first)
            .await?
            .ok_or("missing budget")?
            .amount_microunits,
        10_000_000
    );

    let now = Utc::now().timestamp();
    let this_week = (now - 345_600).div_euclid(604_800) * 604_800 + 345_600;
    let next_week = this_week + 604_800;
    let rate = NewFlashRateCard {
        currency: "USD".into(),
        cpu_microunits_per_vcpu_hour: 1_000_000,
        memory_microunits_per_gib_hour: 500_000,
        gpu_microunits_per_gpu_hour: 2_000_000,
    };
    assert!(store.flash_rate_for_week(this_week).await?.is_none());
    assert!(matches!(
        store
            .schedule_flash_rate_card(&rate, owner.user.id, this_week)
            .await,
        Err(StoreError::Conflict)
    ));
    let saved = store
        .schedule_flash_rate_card(&rate, owner.user.id, next_week)
        .await?;
    assert_eq!(saved.effective_week_start, next_week);
    assert!(store.flash_rate_for_week(this_week).await?.is_none());
    assert_eq!(
        store
            .flash_rate_for_week(next_week)
            .await?
            .ok_or("missing rate")?
            .cpu_microunits_per_vcpu_hour,
        1_000_000
    );
    store.delete_flash_weekly_budget(first).await?;
    assert!(store.flash_weekly_budget(first).await?.is_none());
    Ok(())
}
