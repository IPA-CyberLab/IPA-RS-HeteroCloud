use std::{env, error::Error, sync::Arc, time::Duration as StdDuration};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use chrono::{Duration, Utc};
use heterocloud_api::flow_access::FlowAccessSigner;
use heterocloud_api::{app, config::RuntimeConfig, routes::AppState};
use heterocloud_domain::OrganizationId;
use heterocloud_store::{BootstrapAdmin, OidcUser, RegisterWithInvitation, Store, StoreError};
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::sync::Semaphore;
use tower::ServiceExt;
use url::Url;

const TEST_DATABASE_ENV: &str = "HETEROCLOUD_TEST_DATABASE_URL";
const PUBLIC_ORIGIN: &str = "http://console.example.test";

#[tokio::test]
async fn registration_checks_invitation_before_hashing_and_consumes_it_once()
-> Result<(), Box<dyn Error>> {
    let Some((store, state)) = test_state().await? else {
        return Ok(());
    };

    let invalid_response = register_request(
        state.clone(),
        json!({
            "invitation_code": "invalid-invitation",
            "email": "invalid@example.test",
            "display_name": "Invalid Invitation",
            "password": "short"
        }),
    )
    .await?;
    assert_eq!(invalid_response.0, StatusCode::BAD_REQUEST);
    assert_eq!(
        invalid_response.1,
        json!({
            "error": {
                "code": "bad_request",
                "message": "The invitation is invalid or no longer available."
            }
        })
    );

    let owner = store
        .bootstrap_admin(BootstrapAdmin {
            email: "api-owner@example.test",
            display_name: "API Test Owner",
            password_hash: "test-owner-password-hash",
            organization_slug: "api-abuse-hardening-test",
            organization_name: "API Abuse Hardening Test",
        })
        .await?;
    let membership = owner
        .memberships
        .first()
        .ok_or("bootstrap owner has no membership")?;
    let invitation_hash = heterocloud_auth::token_hash("valid-invitation");
    store
        .create_invitation(
            OrganizationId(membership.organization_id.0),
            owner.user.id,
            &invitation_hash,
            Utc::now() + Duration::hours(1),
        )
        .await?;

    let weak_password_response = register_request(
        state,
        json!({
            "invitation_code": "valid-invitation",
            "email": "weak-password@example.test",
            "display_name": "Weak Password",
            "password": "short"
        }),
    )
    .await?;
    assert_eq!(weak_password_response.0, StatusCode::BAD_REQUEST);
    assert_eq!(
        weak_password_response.1["error"]["message"],
        "password does not satisfy the password policy"
    );
    assert!(store.invitation_available(&invitation_hash).await?);

    let first_store = store.clone();
    let second_store = store.clone();
    let first = first_store.register_with_invitation(RegisterWithInvitation {
        code_hash: &invitation_hash,
        email: "first@example.test",
        display_name: "First Registrant",
        password_hash: "test-first-password-hash",
    });
    let second = second_store.register_with_invitation(RegisterWithInvitation {
        code_hash: &invitation_hash,
        email: "second@example.test",
        display_name: "Second Registrant",
        password_hash: "test-second-password-hash",
    });
    let (first_result, second_result) = tokio::join!(first, second);
    let results = [first_result, second_result];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(StoreError::InvitationUnavailable)))
            .count(),
        1
    );
    assert!(!store.invitation_available(&invitation_hash).await?);
    let used_count: i32 =
        sqlx::query_scalar("SELECT used_count FROM invitations WHERE code_hash = $1")
            .bind(invitation_hash.as_slice())
            .fetch_one(store.pool())
            .await?;
    assert_eq!(used_count, 1);

    let oidc_user = store
        .find_or_create_oidc_user(OidcUser {
            issuer: "https://idp.example.test/realms/heterocloud",
            subject: "oidc-subject-1",
            email: "oidc-user@example.test",
            display_name: "OIDC User",
        })
        .await?;
    assert_eq!(oidc_user.memberships.len(), 1);
    assert_eq!(oidc_user.memberships[0].role, "owner");
    assert!(
        store
            .list_projects(oidc_user.memberships[0].organization_id)
            .await?
            .is_empty()
    );
    assert!(
        store
            .password_user_by_email("oidc-user@example.test")
            .await?
            .is_none()
    );
    let returning_oidc_user = store
        .find_or_create_oidc_user(OidcUser {
            issuer: "https://idp.example.test/realms/heterocloud",
            subject: "oidc-subject-1",
            email: "changed-claim@example.test",
            display_name: "Changed Claim",
        })
        .await?;
    assert_eq!(returning_oidc_user.user.id, oidc_user.user.id);
    assert_eq!(returning_oidc_user.user.email, "oidc-user@example.test");
    assert!(matches!(
        store
            .find_or_create_oidc_user(OidcUser {
                issuer: "https://idp.example.test/realms/other",
                subject: "different-subject",
                email: "oidc-user@example.test",
                display_name: "Must Not Link",
            })
            .await,
        Err(StoreError::AlreadyExists)
    ));
    assert!(matches!(
        store
            .find_or_create_oidc_user(OidcUser {
                issuer: "https://idp.example.test/realms/heterocloud",
                subject: "local-email-subject",
                email: "api-owner@example.test",
                display_name: "Must Not Link Local",
            })
            .await,
        Err(StoreError::AlreadyExists)
    ));
    let external_identity_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM user_external_identities")
            .fetch_one(store.pool())
            .await?;
    assert_eq!(external_identity_count, 1);

    Ok(())
}

async fn test_state() -> Result<Option<(Store, Arc<AppState>)>, Box<dyn Error>> {
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

    let public_origin = Url::parse(PUBLIC_ORIGIN)?;
    let state = Arc::new(AppState {
        store: store.clone(),
        config: RuntimeConfig {
            public_origin,
            allowed_origins: vec![PUBLIC_ORIGIN.to_owned()],
            secure_cookie: false,
            session_ttl: StdDuration::from_secs(3600),
            csrf_key: SecretString::from("test-csrf-key-at-least-32-bytes"),
            flow_access_signer: FlowAccessSigner::new(
                "heterocloud",
                "heterocloud-flow-data",
                SecretString::from("test-flow-access-secret-at-least-32-bytes"),
            )?,
            flow_public_endpoints: vec![Url::parse("http://flow.example.test")?],
            oidc: None,
        },
        registration_limiter: Arc::new(Semaphore::new(2)),
    });
    Ok(Some((store, state)))
}

async fn register_request(
    state: Arc<AppState>,
    payload: Value,
) -> Result<(StatusCode, Value), Box<dyn Error>> {
    let response = app(state, None)
        .oneshot(
            Request::post("/api/v1/auth/register")
                .header(header::ORIGIN, PUBLIC_ORIGIN)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&payload)?))?,
        )
        .await?;
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024 * 1024).await?;
    Ok((status, serde_json::from_slice(&body)?))
}
