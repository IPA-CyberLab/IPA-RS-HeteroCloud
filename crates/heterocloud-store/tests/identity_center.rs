use std::{env, error::Error};

use heterocloud_auth::token_hash;
use heterocloud_domain::{PolicyDocument, PrincipalId, UserId};
use heterocloud_iam::semantics_digest;
use heterocloud_store::{BootstrapAdmin, Store, StoreError};
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn assignments_provision_managed_roles_and_revocation_never_revives_sessions()
-> Result<(), Box<dyn Error>> {
    let Ok(url) = env::var("HETEROCLOUD_TEST_DATABASE_URL") else {
        return Ok(());
    };
    let store = Store::connect(&url, 8).await?;
    let name: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(store.pool())
        .await?;
    if !name.starts_with("heterocloud_test_") {
        return Err("a disposable test database is required".into());
    }
    sqlx::raw_sql("DROP SCHEMA public CASCADE; CREATE SCHEMA public;")
        .execute(store.pool())
        .await?;
    store.migrate().await?;
    let owner = store
        .bootstrap_admin(BootstrapAdmin {
            email: "identity-center@example.test",
            display_name: "Owner",
            password_hash: "test-only",
            organization_slug: "identity-center",
            organization_name: "Identity Center",
        })
        .await?;
    let management = owner.memberships[0].organization_id;
    let cloud = store
        .cloud_organization_for_account(management)
        .await?
        .ok_or("cloud organization")?;
    let account = store
        .create_cloud_member_account(
            management,
            cloud.root_id,
            owner.user.id,
            "identity-target",
            "Target",
        )
        .await?;
    let independent = store
        .bootstrap_admin(BootstrapAdmin {
            email: "identity-other@example.test",
            display_name: "Other",
            password_hash: "test-only",
            organization_slug: "identity-other",
            organization_name: "Other",
        })
        .await?;
    let user = UserId(Uuid::now_v7());
    let principal = PrincipalId(Uuid::now_v7());
    sqlx::query("INSERT INTO users(id,email,display_name,password_hash,status) VALUES($1,'member@example.test','Member','test-only','active')").bind(user.0).execute(store.pool()).await?;
    sqlx::query("INSERT INTO principals(id,organization_id,kind,name,user_id,quota_user_id,enabled) VALUES($1,$2,'user','Member',$3,$3,true)").bind(principal.0).bind(management.0).bind(user.0).execute(store.pool()).await?;
    sqlx::query("INSERT INTO organization_memberships(organization_id,user_id,principal_id,role) VALUES($1,$2,$3,'member')").bind(management.0).bind(user.0).bind(principal.0).execute(store.pool()).await?;
    assert!(!store.identity_center_enabled().await?);
    store.enable_identity_center().await?;
    let policy = PolicyDocument::from_aws_json(
        json!({"Statement":{"Effect":"Allow","Action":"project:List","Resource":"*"}}),
    )?;
    let set = store
        .create_identity_center_permission_set(management, "Reader", &policy, 3600)
        .await?;
    let group = store
        .create_identity_center_group(management, "Developers")
        .await?;
    store
        .change_identity_center_group_member(management, group.id, user, true)
        .await?;
    assert!(matches!(
        store
            .create_identity_center_assignment(
                management,
                set.id,
                independent.memberships[0].organization_id,
                "group",
                group.id,
                &semantics_digest()
            )
            .await,
        Err(StoreError::NotFound)
    ));
    let assignment = store
        .create_identity_center_assignment(
            management,
            set.id,
            account.id,
            "group",
            group.id,
            &semantics_digest(),
        )
        .await?;
    assert!(
        store
            .is_identity_center_role(account.id, PrincipalId(assignment.role_principal_id))
            .await?
    );
    assert!(
        store
            .authorization_context(user, account.id)
            .await?
            .is_some(),
        "assigned users must be attributable to their individual quota"
    );
    assert_eq!(
        store
            .identity_center_assignments(management, Some(user))
            .await?
            .len(),
        1
    );
    assert!(matches!(
        store
            .delete_identity_center_group(management, group.id)
            .await,
        Err(StoreError::RequestRejected(_))
    ));
    let bindings = store.list_iam_bindings(account.id).await?;
    let binding = bindings
        .iter()
        .find(|b| b["principal_id"] == assignment.role_principal_id.to_string())
        .ok_or("managed binding")?;
    let binding_id = Uuid::parse_str(binding["id"].as_str().ok_or("binding id")?)?;
    let policy_id = heterocloud_domain::PolicyId(Uuid::parse_str(
        binding["policy_id"].as_str().ok_or("policy id")?,
    )?);
    assert!(
        store
            .create_binding(
                account.id,
                PrincipalId(assignment.role_principal_id),
                policy_id
            )
            .await
            .is_err()
    );
    assert!(store.delete_binding(account.id, binding_id).await.is_err());
    assert!(
        store
            .set_permissions_boundary(
                account.id,
                PrincipalId(assignment.role_principal_id),
                Some(policy_id.0)
            )
            .await
            .is_err()
    );
    assert!(
        store
            .update_iam_role_trust(
                account.id,
                PrincipalId(assignment.role_principal_id),
                &json!({})
            )
            .await
            .is_err()
    );
    assert!(
        store
            .delete_iam_role(account.id, PrincipalId(assignment.role_principal_id))
            .await
            .is_err()
    );
    let first = "identity-center-session-1";
    let first_session = store
        .create_identity_center_session(management, assignment.id, user, &token_hash(first))
        .await?;
    assert_eq!(first_session.account_id, account.id.0);
    assert_eq!(
        store
            .authenticate_iam_role_session(&token_hash(first))
            .await?
            .ok_or("role session")?
            .identity_center_user_id,
        Some(user.0)
    );
    assert!(
        store
            .create_identity_center_session(
                management,
                assignment.id,
                owner.user.id,
                &token_hash("not-assigned")
            )
            .await
            .is_err()
    );
    store
        .change_identity_center_group_member(management, group.id, user, false)
        .await?;
    assert!(
        store
            .authenticate_iam_role_session(&token_hash(first))
            .await?
            .is_none()
    );
    store
        .change_identity_center_group_member(management, group.id, user, true)
        .await?;
    assert!(
        store
            .authenticate_iam_role_session(&token_hash(first))
            .await?
            .is_none(),
        "readding a member must not revive an old session"
    );
    let second = "identity-center-session-2";
    store
        .create_identity_center_session(management, assignment.id, user, &token_hash(second))
        .await?;
    store
        .revoke_identity_center_assignment(management, assignment.id)
        .await?;
    assert!(
        store
            .authenticate_iam_role_session(&token_hash(second))
            .await?
            .is_none()
    );
    let reassigned = store
        .create_identity_center_assignment(
            management,
            set.id,
            account.id,
            "group",
            group.id,
            &semantics_digest(),
        )
        .await?;
    assert_eq!(reassigned.id, assignment.id);
    assert!(
        store
            .authenticate_iam_role_session(&token_hash(second))
            .await?
            .is_none(),
        "reassignment must not revive an old session"
    );
    let third = "identity-center-session-3";
    store
        .create_identity_center_session(management, assignment.id, user, &token_hash(third))
        .await?;
    let target_principal: Uuid = sqlx::query_scalar(
        "SELECT principal_id FROM organization_memberships WHERE organization_id=$1 AND user_id=$2",
    )
    .bind(account.id.0)
    .bind(user.0)
    .fetch_one(store.pool())
    .await?;
    sqlx::query("DELETE FROM organization_memberships WHERE organization_id=$1 AND user_id=$2")
        .bind(account.id.0)
        .bind(user.0)
        .execute(store.pool())
        .await?;
    sqlx::query("INSERT INTO organization_memberships(organization_id,user_id,principal_id,role) VALUES($1,$2,$3,'member')")
        .bind(account.id.0).bind(user.0).bind(target_principal).execute(store.pool()).await?;
    assert!(
        store
            .authenticate_iam_role_session(&token_hash(third))
            .await?
            .is_none(),
        "rejoining the account with the same principal must not revive an old session"
    );
    let fourth = "identity-center-session-4";
    store
        .create_identity_center_session(management, assignment.id, user, &token_hash(fourth))
        .await?;
    sqlx::query("DELETE FROM organization_memberships WHERE organization_id=$1 AND user_id=$2")
        .bind(management.0)
        .bind(user.0)
        .execute(store.pool())
        .await?;
    sqlx::query("INSERT INTO organization_memberships(organization_id,user_id,principal_id,role) VALUES($1,$2,$3,'member')")
        .bind(management.0).bind(user.0).bind(principal.0).execute(store.pool()).await?;
    assert!(
        store
            .authenticate_iam_role_session(&token_hash(fourth))
            .await?
            .is_none(),
        "rejoining the management account must not revive an old session"
    );
    let replacement = PolicyDocument::from_aws_json(
        json!({"Statement":{"Effect":"Deny","Action":"project:List","Resource":"*"}}),
    )?;
    store
        .update_identity_center_permission_set(
            management,
            set.id,
            &replacement,
            3600,
            &semantics_digest(),
        )
        .await?;
    let context = store
        .authorization_context_for_principal(PrincipalId(assignment.role_principal_id), account.id)
        .await?
        .ok_or("managed context")?;
    assert_eq!(context.policies, vec![replacement]);
    store
        .revoke_identity_center_assignment(management, assignment.id)
        .await?;
    store
        .delete_identity_center_group(management, group.id)
        .await?;
    store
        .delete_identity_center_permission_set(management, set.id)
        .await?;
    // Deleting a test organization must cascade through managed-role mappings.
    sqlx::query("DELETE FROM organizations WHERE id=$1")
        .bind(account.id.0)
        .execute(store.pool())
        .await?;
    Ok(())
}
