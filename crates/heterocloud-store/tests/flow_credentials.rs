use std::{env, error::Error};

use chrono::{Duration, Utc};
use heterocloud_domain::{OrganizationId, PrincipalId, ProjectId};
use heterocloud_store::{
    BootstrapAdmin, DeveloperCredentialMint, DeveloperCredentialMintOutcome, NewFlowAccessContext,
    NewFlowDeveloperCredential, Store,
};
use serde_json::json;
use uuid::Uuid;

const TEST_DATABASE_ENV: &str = "HETEROCLOUD_STORE_TEST_DATABASE_URL";

#[tokio::test]
async fn developer_credentials_are_scoped_hashed_rotatable_and_revocable()
-> Result<(), Box<dyn Error>> {
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
            email: "flow-credential-owner@example.test",
            display_name: "Flow Credential Owner",
            password_hash: "test-password-hash",
            organization_slug: "flow-credential-test",
            organization_name: "Flow Credential Test",
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
            json!({
                "region": "global",
                "traffic_mode": "forwarded",
                "max_participants": 100,
                "max_rooms": 100,
                "rate_limit": {"requests_per_second": 20, "burst": 40},
                "turn_enabled": true,
                "metadata": {}
            }),
        )
        .await?;
    assert!(
        store
            .mark_service_instance_ready(
                instance.id,
                instance.generation,
                Uuid::from_u128(10),
                json!({"phase": "ready"}),
            )
            .await?
    );

    let permissions = vec![
        "flow.room.join".to_owned(),
        "flow.signal.connect".to_owned(),
    ];
    let first_hash = [7_u8; 32];
    let first_created_at = Utc::now();
    let first = store
        .create_flow_developer_credential(NewFlowDeveloperCredential {
            organization_id,
            service_instance_id: instance.id,
            created_by: membership.principal_id,
            name: "application backend",
            prefix: "hcf_0123456789abcdef",
            secret_hash: &first_hash,
            permissions: &permissions,
            expires_at: first_created_at + Duration::days(30),
            created_at: first_created_at,
        })
        .await?;
    assert_eq!(first.permissions, permissions);
    let stored_hash: Vec<u8> =
        sqlx::query_scalar("SELECT secret_hash FROM flow_developer_credentials WHERE id = $1")
            .bind(first.id)
            .fetch_one(store.pool())
            .await?;
    assert_eq!(stored_hash, first_hash);
    let secret_columns: i64 = sqlx::query_scalar(
        "SELECT count(*)
         FROM information_schema.columns
         WHERE table_schema = 'public'
           AND table_name = 'flow_developer_credentials'
           AND column_name IN ('credential', 'secret', 'token')",
    )
    .fetch_one(store.pool())
    .await?;
    assert_eq!(secret_columns, 0);

    let issued_at = Utc::now();
    let first_context_id = Uuid::now_v7();
    let requested_permissions = vec!["flow.room.join".to_owned()];
    let outcome = store
        .mint_flow_access_context_with_developer_credential(&DeveloperCredentialMint {
            prefix: &first.prefix,
            secret_hash: &first_hash,
            context_id: first_context_id,
            principal_id: PrincipalId(Uuid::from_u128(100)),
            permissions: &requested_permissions,
            issued_at,
            expires_at: issued_at + Duration::minutes(5),
        })
        .await?;
    assert!(matches!(outcome, DeveloperCredentialMintOutcome::Issued(_)));
    let mint_audit: serde_json::Value = sqlx::query_scalar(
        "SELECT metadata FROM audit_events
         WHERE action = 'realtime:MintAccessCredential' AND request_id = $1",
    )
    .bind(first_context_id.to_string())
    .fetch_one(store.pool())
    .await?;
    assert_eq!(
        mint_audit["authentication"]["credential_id"],
        json!(first.id)
    );
    assert_eq!(mint_audit["context_id"], json!(first_context_id));
    assert_eq!(mint_audit["service_instance_id"], json!(instance.id));
    assert_eq!(
        mint_audit["issued_principal_id"],
        json!(Uuid::from_u128(100))
    );
    let rendered_mint_audit = serde_json::to_string(&mint_audit)?;
    assert!(!rendered_mint_audit.contains("hcf_"));
    assert!(!rendered_mint_audit.contains("secret"));
    let denied = store
        .mint_flow_access_context_with_developer_credential(&DeveloperCredentialMint {
            prefix: &first.prefix,
            secret_hash: &first_hash,
            context_id: Uuid::now_v7(),
            principal_id: PrincipalId(Uuid::from_u128(101)),
            permissions: &["flow.turn.issue".to_owned()],
            issued_at,
            expires_at: issued_at + Duration::minutes(5),
        })
        .await?;
    assert!(matches!(
        denied,
        DeveloperCredentialMintOutcome::PermissionDenied
    ));

    let browser_context_id = Uuid::now_v7();
    store
        .record_flow_access_context(&NewFlowAccessContext {
            context_id: browser_context_id,
            organization_id,
            project_id: project.id,
            service_instance_id: instance.id,
            credential_id: None,
            principal_id: membership.principal_id,
            permissions: &requested_permissions,
            issued_at,
            expires_at: issued_at + Duration::minutes(5),
        })
        .await?;
    let all_contexts = store
        .list_flow_access_contexts(organization_id, instance.id, 100)
        .await?;
    assert_eq!(all_contexts.len(), 2);
    assert!(
        all_contexts
            .iter()
            .any(|item| { item.context_id == browser_context_id && item.credential_id.is_none() })
    );

    assert!(
        store
            .revoke_flow_access_context(
                organization_id,
                instance.id,
                browser_context_id,
                membership.principal_id,
            )
            .await?
    );
    assert!(
        !store
            .revoke_flow_access_context(
                organization_id,
                instance.id,
                browser_context_id,
                membership.principal_id,
            )
            .await?
    );

    let second_hash = [8_u8; 32];
    let second_created_at = Utc::now();
    let second = store
        .create_flow_developer_credential(NewFlowDeveloperCredential {
            organization_id,
            service_instance_id: instance.id,
            created_by: membership.principal_id,
            name: "second backend",
            prefix: "hcf_fedcba9876543210",
            secret_hash: &second_hash,
            permissions: &permissions,
            expires_at: second_created_at + Duration::days(30),
            created_at: second_created_at,
        })
        .await?;
    let second_context_id = Uuid::now_v7();
    let second_outcome = store
        .mint_flow_access_context_with_developer_credential(&DeveloperCredentialMint {
            prefix: &second.prefix,
            secret_hash: &second_hash,
            context_id: second_context_id,
            principal_id: PrincipalId(Uuid::from_u128(102)),
            permissions: &requested_permissions,
            issued_at,
            expires_at: issued_at + Duration::minutes(5),
        })
        .await?;
    assert!(matches!(
        second_outcome,
        DeveloperCredentialMintOutcome::Issued(_)
    ));
    assert_eq!(
        store
            .revoke_flow_access_context_with_developer_credential(
                &first.prefix,
                &first_hash,
                second_context_id,
            )
            .await?,
        Some(false)
    );
    assert_eq!(
        store
            .revoke_flow_access_context_with_developer_credential(
                &first.prefix,
                &first_hash,
                first_context_id,
            )
            .await?,
        Some(true)
    );
    assert_eq!(
        store
            .revoke_flow_access_context_with_developer_credential(
                &first.prefix,
                &first_hash,
                first_context_id,
            )
            .await?,
        Some(false)
    );
    let revoke_audit: serde_json::Value = sqlx::query_scalar(
        "SELECT metadata FROM audit_events
         WHERE action = 'realtime:RevokeAccessContext'
           AND metadata ->> 'context_id' = $1
           AND metadata ->> 'revoked_now' = 'true'",
    )
    .bind(first_context_id.to_string())
    .fetch_one(store.pool())
    .await?;
    assert_eq!(
        revoke_audit["authentication"]["credential_id"],
        json!(first.id)
    );
    assert_eq!(
        revoke_audit["context_principal_id"],
        json!(Uuid::from_u128(100))
    );
    assert!(!serde_json::to_string(&revoke_audit)?.contains("hcf_"));
    let first_issued = store
        .list_flow_access_contexts_for_developer_credential(&first.prefix, &first_hash, 100)
        .await?
        .ok_or("first credential should authenticate")?;
    assert_eq!(first_issued.len(), 1);
    assert_eq!(first_issued[0].context_id, first_context_id);

    let revocation_events: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_events WHERE topic = 'principal-context.revoke'",
    )
    .fetch_one(store.pool())
    .await?;
    assert_eq!(revocation_events, 2);
    let payload: serde_json::Value = sqlx::query_scalar(
        "SELECT payload FROM outbox_events
         WHERE topic = 'principal-context.revoke' AND aggregate_id = $1",
    )
    .bind(first_context_id)
    .fetch_one(store.pool())
    .await?;
    assert_eq!(payload["context_id"], json!(first_context_id));
    assert_eq!(payload["service_instance_id"], json!(instance.id));
    assert_eq!(payload["provider"], json!("flow"));

    let rotated_hash = [9_u8; 32];
    let rotation = store
        .rotate_flow_developer_credential(
            organization_id,
            instance.id,
            second.id,
            "hcf_0011223344556677",
            &rotated_hash,
            membership.principal_id,
        )
        .await?;
    assert_eq!(rotation.contexts_revoked, 1);
    let rotated = rotation.credential;
    assert_eq!(rotated.id, second.id);
    assert!(
        store
            .list_flow_access_contexts_for_developer_credential(&second.prefix, &second_hash, 100,)
            .await?
            .is_none()
    );
    assert!(
        store
            .list_flow_access_contexts_for_developer_credential(
                &rotated.prefix,
                &rotated_hash,
                100,
            )
            .await?
            .is_some()
    );

    let delete_cascade_context_id = Uuid::now_v7();
    let delete_cascade = store
        .mint_flow_access_context_with_developer_credential(&DeveloperCredentialMint {
            prefix: &first.prefix,
            secret_hash: &first_hash,
            context_id: delete_cascade_context_id,
            principal_id: PrincipalId(Uuid::from_u128(103)),
            permissions: &requested_permissions,
            issued_at,
            expires_at: issued_at + Duration::minutes(5),
        })
        .await?;
    assert!(matches!(
        delete_cascade,
        DeveloperCredentialMintOutcome::Issued(_)
    ));
    let revocation = store
        .revoke_flow_developer_credential(
            organization_id,
            instance.id,
            first.id,
            membership.principal_id,
        )
        .await?;
    assert!(revocation.credential_revoked);
    assert_eq!(revocation.contexts_revoked, 1);
    let repeated = store
        .revoke_flow_developer_credential(
            organization_id,
            instance.id,
            first.id,
            membership.principal_id,
        )
        .await?;
    assert!(!repeated.credential_revoked);
    assert_eq!(repeated.contexts_revoked, 0);
    assert_eq!(
        revocation_event_count(&store, delete_cascade_context_id).await?,
        1
    );
    let after_revoke = store
        .mint_flow_access_context_with_developer_credential(&DeveloperCredentialMint {
            prefix: &first.prefix,
            secret_hash: &first_hash,
            context_id: Uuid::now_v7(),
            principal_id: PrincipalId(Uuid::from_u128(104)),
            permissions: &requested_permissions,
            issued_at,
            expires_at: issued_at + Duration::minutes(5),
        })
        .await?;
    assert!(matches!(
        after_revoke,
        DeveloperCredentialMintOutcome::InvalidCredential
    ));
    Ok(())
}

async fn revocation_event_count(store: &Store, context_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT count(*)
         FROM outbox_events
         WHERE topic = 'principal-context.revoke' AND aggregate_id = $1",
    )
    .bind(context_id)
    .fetch_one(store.pool())
    .await
}
