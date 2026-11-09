use std::{env, error::Error};

use heterocloud_store::Store;

static NEXT: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

#[tokio::test]
async fn staged_release_survives_additive_upgrade_but_rejects_unknown_versions()
-> Result<(), Box<dyn Error>> {
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
    let applied: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM _sqlx_migrations WHERE version=36)")
            .fetch_one(store.pool())
            .await?;
    assert!(
        !applied,
        "the compatibility release must not activate migration 36"
    );
    NEXT.run(store.pool()).await?;
    store.migrate().await?;
    let applied: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM _sqlx_migrations WHERE version=36)")
            .fetch_one(store.pool())
            .await?;
    assert!(
        applied,
        "a later release must be able to activate migration 36"
    );

    sqlx::query("INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) VALUES (37, 'unknown future migration', true, decode('00','hex'), 0)")
        .execute(store.pool()).await?;
    assert!(
        store.migrate().await.is_err(),
        "unrecognized schema versions must fail closed"
    );
    sqlx::query("DELETE FROM _sqlx_migrations WHERE version=37")
        .execute(store.pool())
        .await?;
    sqlx::query("UPDATE _sqlx_migrations SET checksum=decode('00','hex') WHERE version=35")
        .execute(store.pool())
        .await?;
    assert!(
        store.migrate().await.is_err(),
        "modified historical migrations must fail closed"
    );
    Ok(())
}
