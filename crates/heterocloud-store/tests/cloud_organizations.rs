use std::{env, error::Error, fs, path::PathBuf};

use heterocloud_domain::{OrganizationId, PolicyDocument, UserId};
use heterocloud_iam::{
    AuthorizationRequest, Decision, authorize_with_administrator, enforce_service_control_policies,
};
use heterocloud_store::Store;
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn migration_and_organization_controls_preserve_isolation() -> Result<(), Box<dyn Error>> {
    let Ok(url) = env::var("HETEROCLOUD_TEST_DATABASE_URL") else {
        return Ok(());
    };
    let store = Store::connect(&url, 4).await?;
    let database: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(store.pool())
        .await?;
    if !database.starts_with("heterocloud_test_") {
        return Err("a disposable heterocloud_test_ database is required".into());
    }
    sqlx::raw_sql("DROP SCHEMA public CASCADE; CREATE SCHEMA public;")
        .execute(store.pool())
        .await?;
    let migrations = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../migrations");
    let mut paths = fs::read_dir(migrations)?
        .map(|entry| entry.map(|item| item.path()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    for path in paths.iter().filter(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".sql") && name < "0026_")
    }) {
        sqlx::raw_sql(sqlx::AssertSqlSafe(fs::read_to_string(path)?))
            .execute(store.pool())
            .await?;
    }
    let user = Uuid::now_v7();
    let old_account = Uuid::now_v7();
    let principal = Uuid::now_v7();
    let project = Uuid::now_v7();
    let service = Uuid::now_v7();
    sqlx::query("INSERT INTO users(id,email,display_name,password_hash,status) VALUES ($1,'aws-model@example.test','Model Owner','hash','active')").bind(user).execute(store.pool()).await?;
    sqlx::query("INSERT INTO organizations(id,slug,name) VALUES ($1,'old-account','Old account')")
        .bind(old_account)
        .execute(store.pool())
        .await?;
    sqlx::query("INSERT INTO principals(id,organization_id,kind,name,user_id) VALUES ($1,$2,'user','Model Owner',$3)").bind(principal).bind(old_account).bind(user).execute(store.pool()).await?;
    sqlx::query("INSERT INTO organization_memberships(organization_id,user_id,principal_id,role) VALUES ($1,$2,$3,'owner')").bind(old_account).bind(user).bind(principal).execute(store.pool()).await?;
    sqlx::query("INSERT INTO projects(id,organization_id,slug,name) VALUES ($1,$2,'existing-project','Existing')").bind(project).bind(old_account).execute(store.pool()).await?;
    sqlx::query("INSERT INTO service_instances(id,organization_id,project_id,provider,name,state,spec) VALUES ($1,$2,$3,'flash','existing','ready','{}')").bind(service).bind(old_account).bind(project).execute(store.pool()).await?;
    let existing_session_hash = [8_u8; 32];
    sqlx::query("INSERT INTO sessions(id,user_id,token_hash,expires_at) VALUES ($1,$2,$3,now() + interval '1 hour')")
        .bind(Uuid::now_v7()).bind(user).bind(existing_session_hash.as_slice()).execute(store.pool()).await?;
    sqlx::raw_sql(include_str!(
        "../../../migrations/0026_cloud_organizations.sql"
    ))
    .execute(store.pool())
    .await?;

    let existing_service_account = Uuid::now_v7();
    let existing_key_hash = [7_u8; 32];
    sqlx::query("INSERT INTO principals (id, organization_id, kind, name) VALUES ($1,$2,'service_account','existing-service-account')")
        .bind(existing_service_account).bind(old_account).execute(store.pool()).await?;
    sqlx::query("INSERT INTO api_keys (id, organization_id, principal_id, name, prefix, secret_hash) VALUES ($1,$2,$3,'existing','0123456789abcdef',$4)")
        .bind(Uuid::now_v7()).bind(old_account).bind(existing_service_account).bind(existing_key_hash.as_slice()).execute(store.pool()).await?;
    sqlx::raw_sql(include_str!("../../../migrations/0027_iam_roles.sql"))
        .execute(store.pool())
        .await?;
    assert!(!store.roles_enabled().await?);
    sqlx::raw_sql(include_str!(
        "../../../migrations/0028_browser_role_sessions.sql"
    ))
    .execute(store.pool())
    .await?;
    assert!(!store.browser_roles_enabled().await?);
    // The current store queries require the latest schema. Preserve the
    // pre-organization fixtures above, then complete every later migration
    // before checking that existing sessions and keys still work.
    for path in paths.iter().filter(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".sql") && name >= "0029_")
    }) {
        sqlx::raw_sql(sqlx::AssertSqlSafe(fs::read_to_string(path)?))
            .execute(store.pool())
            .await?;
    }
    assert!(
        store
            .session_user_by_token_hash(&existing_session_hash)
            .await?
            .is_some()
    );
    assert!(
        store
            .browser_active_role(&existing_session_hash)
            .await?
            .is_none()
    );
    assert!(
        store
            .enabled_task_role(
                OrganizationId(old_account),
                heterocloud_domain::PrincipalId(existing_service_account)
            )
            .await?
    );
    assert!(
        store
            .authenticate_api_key("0123456789abcdef", &existing_key_hash)
            .await?
            .is_some()
    );

    sqlx::query("UPDATE cloud_security_activation SET scp_enabled = true WHERE singleton = true")
        .execute(store.pool())
        .await?;
    let management = OrganizationId(old_account);
    let org = store
        .cloud_organization_for_account(management)
        .await?
        .ok_or("missing management organization")?;
    assert_eq!(org.management_account_id, old_account);
    assert_eq!(org.current_account_number.len(), 12);
    let preserved: Uuid =
        sqlx::query_scalar("SELECT organization_id FROM service_instances WHERE id=$1")
            .bind(service)
            .fetch_one(store.pool())
            .await?;
    assert_eq!(preserved, old_account);
    let ou = store
        .create_cloud_organizational_unit(management, org.root_id, "Workloads")
        .await?;
    let mut parent = ou.id;
    for level in 2..=5 {
        parent = store
            .create_cloud_organizational_unit(management, parent, &format!("Level{level}"))
            .await?
            .id;
    }
    assert!(
        store
            .create_cloud_organizational_unit(management, parent, "TooDeep")
            .await
            .is_err()
    );
    let account = store
        .create_cloud_member_account(management, ou.id, UserId(user), "new-member", "Member")
        .await?;
    let member_org = store
        .cloud_organization_for_account(account.id)
        .await?
        .ok_or("missing member organization")?;
    assert_eq!(member_org.id, org.id);
    assert_ne!(
        member_org.current_account_number,
        org.current_account_number
    );
    assert_eq!(
        store
            .cloud_organization_tree(management)
            .await?
            .accounts
            .len(),
        2
    );
    assert!(store.cloud_organization_tree(account.id).await.is_err());
    assert!(
        store
            .move_cloud_member_account(management, management, ou.id)
            .await
            .is_err()
    );
    assert!(
        store
            .create_cloud_organizational_unit(account.id, ou.id, "Forbidden")
            .await
            .is_err()
    );

    let request = AuthorizationRequest {
        principal_organization_id: account.id,
        evaluated_at: chrono::Utc::now(),
        resource_organization_id: account.id,
        action: "flash:CreateService",
        resource: "hc:org:test:flash/*",
    };
    let identity = authorize_with_administrator(&request, &[], true)?;
    let levels = store.service_control_levels(account.id).await?;
    assert_eq!(levels.len(), 3);
    assert_eq!(
        enforce_service_control_policies(&request, identity.clone(), &levels)?.decision,
        Decision::Allow
    );
    let deny = PolicyDocument::from_aws_json(
        json!({"Version":"2012-10-17","Statement":{"Effect":"Deny","Action":"flash:Create*","Resource":"*"}}),
    )?;
    let deny = store
        .create_service_control_policy(management, "DenyFlash", &deny)
        .await?;
    for index in 0..9 {
        let policy = store
            .create_service_control_policy(
                management,
                &format!("Extra{index}"),
                &PolicyDocument::from_aws_json(
                    json!({"Statement":{"Effect":"Allow","Action":"*","Resource":"*"}}),
                )?,
            )
            .await?;
        store
            .change_service_control_attachment(management, "unit", org.root_id, policy.id, true)
            .await?;
    }
    assert!(
        store
            .change_service_control_attachment(management, "unit", org.root_id, deny.id, true)
            .await
            .is_err()
    );
    store
        .change_service_control_attachment(management, "unit", ou.id, deny.id, true)
        .await?;
    assert_eq!(
        enforce_service_control_policies(
            &request,
            identity.clone(),
            &store.service_control_levels(account.id).await?
        )?
        .reason,
        "scp_explicit_deny"
    );
    // A management account is not subject to SCPs; moving a member removes the OU cap.
    assert!(store.service_control_levels(management).await?.is_empty());
    store
        .move_cloud_member_account(management, account.id, org.root_id)
        .await?;
    assert_eq!(
        enforce_service_control_policies(
            &request,
            identity,
            &store.service_control_levels(account.id).await?
        )?
        .decision,
        Decision::Allow
    );
    let (policies, _) = store.list_service_control_policies(management).await?;
    let full = policies
        .iter()
        .find(|policy| policy.is_managed)
        .ok_or("missing default policy")?;
    assert!(
        store
            .change_service_control_attachment(management, "account", account.id.0, full.id, false)
            .await
            .is_err()
    );
    Ok(())
}
