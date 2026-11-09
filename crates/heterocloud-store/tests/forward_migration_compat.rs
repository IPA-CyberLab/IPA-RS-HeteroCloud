use std::{env, error::Error, time::Duration};

use heterocloud_store::Store;

static NEXT: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

// The preceding compatibility release embeds the same sources but stops at
// 35. Exercise it before and after the current release activates 36.
async fn compatibility_release(store: &Store) -> Result<(), Box<dyn Error>> {
    let mut connection = store.pool().acquire().await?;
    connection.close_on_drop();
    let result = NEXT.run_to(35, &mut *connection).await;
    let closed = connection.close().await;
    result?;
    closed?;
    Ok(())
}

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

    compatibility_release(&store).await?;
    let applied: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM _sqlx_migrations WHERE version=36)")
            .fetch_one(store.pool())
            .await?;
    assert!(
        !applied,
        "the compatibility release must not activate migration 36"
    );
    store.migrate().await?;
    compatibility_release(&store).await?;
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
    tokio::time::timeout(Duration::from_secs(5), store.migrate()).await??;
    let original_checksum: Vec<u8> =
        sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version=35")
            .fetch_one(store.pool())
            .await?;
    sqlx::query("UPDATE _sqlx_migrations SET checksum=decode('00','hex') WHERE version=35")
        .execute(store.pool())
        .await?;
    assert!(
        store.migrate().await.is_err(),
        "modified historical migrations must fail closed"
    );
    sqlx::query("UPDATE _sqlx_migrations SET checksum=$1 WHERE version=35")
        .bind(original_checksum)
        .execute(store.pool())
        .await?;
    tokio::time::timeout(Duration::from_secs(5), store.migrate()).await??;
    Ok(())
}
