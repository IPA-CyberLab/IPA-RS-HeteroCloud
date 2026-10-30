use std::{env, error::Error, fs, path::PathBuf};

use heterocloud_store::Store;
use uuid::Uuid;

#[tokio::test]
async fn existing_owner_is_backfilled_and_ambiguous_owner_is_not_guessed()
-> Result<(), Box<dyn Error>> {
    let Ok(url) = env::var("HETEROCLOUD_TEST_DATABASE_URL") else {
        return Ok(());
    };
    let store = Store::connect(&url, 4).await?;
    let db: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(store.pool())
        .await?;
    if !db.starts_with("heterocloud_test_") {
        return Err("a disposable heterocloud_test_ database is required".into());
    }
    sqlx::raw_sql("DROP SCHEMA public CASCADE; CREATE SCHEMA public;")
        .execute(store.pool())
        .await?;
    let migrations = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../migrations");
    let mut paths = fs::read_dir(&migrations)?
        .map(|entry| entry.map(|item| item.path()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    for path in paths.iter().filter(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".sql") && name < "0025_")
    }) {
        // These are checked-in migration files, never user-provided SQL.
        sqlx::raw_sql(sqlx::AssertSqlSafe(fs::read_to_string(path)?))
            .execute(store.pool())
            .await?;
    }
    sqlx::raw_sql(r#"
      INSERT INTO users(id,email,display_name,password_hash,status) VALUES ('00000000-0000-0000-0000-000000000001','test@example.test','Test','hash','active');
      INSERT INTO organizations(id,slug,name) VALUES ('00000000-0000-0000-0000-000000000002','test-org','Test');
      INSERT INTO principals(id,organization_id,kind,name,user_id) VALUES ('00000000-0000-0000-0000-000000000003','00000000-0000-0000-0000-000000000002','user','test','00000000-0000-0000-0000-000000000001');
      INSERT INTO organization_memberships(organization_id,user_id,principal_id,role) VALUES ('00000000-0000-0000-0000-000000000002','00000000-0000-0000-0000-000000000001','00000000-0000-0000-0000-000000000003','owner');
      INSERT INTO projects(id,organization_id,slug,name) VALUES ('00000000-0000-0000-0000-000000000004','00000000-0000-0000-0000-000000000002','test-project','Test');
      INSERT INTO service_instances(id,organization_id,project_id,provider,name,state,spec) VALUES ('00000000-0000-0000-0000-000000000005','00000000-0000-0000-0000-000000000002','00000000-0000-0000-0000-000000000004','flash','known','ready','{}'), ('00000000-0000-0000-0000-000000000006','00000000-0000-0000-0000-000000000002','00000000-0000-0000-0000-000000000004','flash','unknown','ready','{}');
      INSERT INTO outbox_events(id,topic,aggregate_id,payload) VALUES ('00000000-0000-0000-0000-000000000007','service-instance.reconcile','00000000-0000-0000-0000-000000000005','{"principal_id":"00000000-0000-0000-0000-000000000003","generation":1}');
    "#).execute(store.pool()).await?;
    sqlx::raw_sql(include_str!(
        "../../../migrations/0025_organization_member_limits.sql"
    ))
    .execute(store.pool())
    .await?;
    let rows: Vec<(String, Option<Uuid>)> =
        sqlx::query_as("SELECT name, quota_user_id FROM service_instances ORDER BY name")
            .fetch_all(store.pool())
            .await?;
    assert_eq!(rows[0].1, Some(Uuid::from_u128(1)));
    assert_eq!(rows[1].1, None);
    let pending: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_events WHERE topic='service-instance.flash-quota'",
    )
    .fetch_one(store.pool())
    .await?;
    assert_eq!(pending, 2);
    Ok(())
}
