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
use secrecy::{ExposeSecret, SecretString};
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
        state.clone(),
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

    store
        .create_session(
            oidc_user.user.id,
            &[7_u8; 32],
            Utc::now() + Duration::hours(1),
            Some("203.0.113.42"),
            "oidc",
        )
        .await?;
    let accounts = store.list_owner_accounts().await?;
    let account = accounts
        .iter()
        .find(|account| account.user.id == oidc_user.user.id)
        .ok_or("OIDC account is missing from owner account list")?;
    assert!(!account.has_local_password);
    assert_eq!(account.external_identities.len(), 1);
    assert_eq!(account.memberships.len(), 1);
    assert_eq!(account.login_count, 1);
    assert_eq!(
        account
            .last_login
            .as_ref()
            .and_then(|login| login.source_ip.as_deref()),
        Some("203.0.113.42")
    );
    let login_events = store.list_user_login_events(oidc_user.user.id, 100).await?;
    assert_eq!(login_events.len(), 1);
    assert_eq!(login_events[0].authentication_method, "oidc");

    let browser_token = "browser-session-for-cli-approval";
    let browser_token_hash = heterocloud_auth::token_hash(browser_token);
    store
        .create_session(
            owner.user.id,
            &browser_token_hash,
            Utc::now() + Duration::hours(1),
            Some("203.0.113.43"),
            "local",
        )
        .await?;
    let browser_csrf = heterocloud_auth::csrf_token(
        browser_token,
        &SecretString::from("test-csrf-key-at-least-32-bytes"),
    )?;
    let no_csrf = app(state.clone(), None)
        .oneshot(
            Request::post("/api/v1/organizations")
                .header(header::ORIGIN, PUBLIC_ORIGIN)
                .header(header::COOKIE, format!("hc_session={browser_token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"slug":"second-org","name":"Second Org"}"#))?,
        )
        .await?;
    assert_eq!(no_csrf.status(), StatusCode::FORBIDDEN);
    let created = app(state.clone(), None)
        .oneshot(
            Request::post("/api/v1/organizations")
                .header(header::ORIGIN, PUBLIC_ORIGIN)
                .header(header::COOKIE, format!("hc_session={browser_token}"))
                .header("x-heterocloud-csrf", browser_csrf.expose_secret())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"slug":"second-org","name":"Second Org"}"#))?,
        )
        .await?;
    assert_eq!(created.status(), StatusCode::CREATED);
    let created: Value =
        serde_json::from_slice(&to_bytes(created.into_body(), 1024 * 1024).await?)?;
    let refreshed = store
        .session_user(owner.user.id)
        .await?
        .ok_or("missing owner")?;
    assert_eq!(refreshed.memberships.len(), 2);
    assert!(refreshed.memberships.iter().any(|item| {
        item.organization_id.to_string() == created["id"].as_str().unwrap_or_default()
            && item.role == "owner"
    }));

    // An existing OIDC user can join another organization without creating a
    // second identity. The invitation remains single-use.
    let org = membership.organization_id;
    let invitation = "existing-user-invitation";
    store
        .create_invitation(
            org,
            owner.user.id,
            &heterocloud_auth::token_hash(invitation),
            Utc::now() + Duration::hours(1),
        )
        .await?;
    let member_token = "existing-user-browser-session";
    store
        .create_session(
            oidc_user.user.id,
            &heterocloud_auth::token_hash(member_token),
            Utc::now() + Duration::hours(1),
            None,
            "oidc",
        )
        .await?;
    let member_csrf = heterocloud_auth::csrf_token(
        member_token,
        &SecretString::from("test-csrf-key-at-least-32-bytes"),
    )?;
    let join = || {
        Request::post("/api/v1/organizations/join")
            .header(header::ORIGIN, PUBLIC_ORIGIN)
            .header(header::COOKIE, format!("hc_session={member_token}"))
            .header("x-heterocloud-csrf", member_csrf.expose_secret())
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"invitation_code":invitation}).to_string(),
            ))
    };
    assert_eq!(
        app(state.clone(), None).oneshot(join()?).await?.status(),
        StatusCode::OK
    );
    assert_ne!(
        app(state.clone(), None).oneshot(join()?).await?.status(),
        StatusCode::OK
    );
    let joined = store
        .session_user(oidc_user.user.id)
        .await?
        .ok_or("missing member")?;
    let joined_membership = joined
        .memberships
        .iter()
        .find(|m| m.organization_id == org)
        .ok_or("member did not join")?;
    assert_eq!(joined.memberships.len(), 2);
    let quota_path = format!("/api/v1/organizations/{org}/quotas");
    let response = app(state.clone(), None)
        .oneshot(
            Request::get(&quota_path)
                .header(header::COOKIE, format!("hc_session={member_token}"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let visible: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await?)?;
    assert!(visible["organization"].is_null());
    assert_eq!(visible["member"]["user_id"], json!(oidc_user.user.id));
    assert_eq!(visible["members"], json!([]));
    let quota = store.effective_resource_quota(org).await?;
    let member_path = format!(
        "/api/v1/organizations/{org}/members/{}/quota",
        oidc_user.user.id
    );
    let request = Request::put(&member_path)
        .header(header::ORIGIN, PUBLIC_ORIGIN)
        .header(header::COOKIE, format!("hc_session={member_token}"))
        .header("x-heterocloud-csrf", member_csrf.expose_secret())
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&quota)?))?;
    assert_eq!(
        app(state.clone(), None).oneshot(request).await?.status(),
        StatusCode::FORBIDDEN
    );
    let role_path = format!(
        "/api/v1/organizations/{org}/members/{}/role",
        oidc_user.user.id
    );
    let promoted = app(state.clone(), None)
        .oneshot(
            Request::put(&role_path)
                .header(header::ORIGIN, PUBLIC_ORIGIN)
                .header(header::COOKIE, format!("hc_session={browser_token}"))
                .header("x-heterocloud-csrf", browser_csrf.expose_secret())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"role":"admin"}"#))?,
        )
        .await?;
    assert_eq!(promoted.status(), StatusCode::NO_CONTENT);
    let forbidden_role_change = app(state.clone(), None)
        .oneshot(
            Request::put(format!(
                "/api/v1/organizations/{org}/members/{}/role",
                owner.user.id
            ))
            .header(header::ORIGIN, PUBLIC_ORIGIN)
            .header(header::COOKIE, format!("hc_session={member_token}"))
            .header("x-heterocloud-csrf", member_csrf.expose_secret())
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"role":"admin"}"#))?,
        )
        .await?;
    assert_eq!(forbidden_role_change.status(), StatusCode::FORBIDDEN);
    let response = app(state.clone(), None)
        .oneshot(
            Request::get(&quota_path)
                .header(header::COOKIE, format!("hc_session={member_token}"))
                .body(Body::empty())?,
        )
        .await?;
    let visible: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await?)?;
    let visible_members = visible["members"].as_array().ok_or("members")?;
    assert!(
        visible_members
            .iter()
            .any(|item| item["user_id"] == json!(owner.user.id))
    );
    assert!(
        visible_members
            .iter()
            .any(|item| item["user_id"] == json!(oidc_user.user.id))
    );
    let mut personal = quota.clone();
    personal.flow.max_services = 1;
    let legacy_account = store
        .create_service_account(org, "legacy-controller")
        .await?;
    let blocked = app(state.clone(), None)
        .oneshot(
            Request::put(&member_path)
                .header(header::ORIGIN, PUBLIC_ORIGIN)
                .header(header::COOKIE, format!("hc_session={member_token}"))
                .header("x-heterocloud-csrf", member_csrf.expose_secret())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&personal)?))?,
        )
        .await?;
    assert_ne!(blocked.status(), StatusCode::OK);
    let unassigned = app(state.clone(), None)
        .oneshot(
            Request::get(format!("/api/v1/organizations/{org}/quota-unassigned"))
                .header(header::COOKIE, format!("hc_session={member_token}"))
                .body(Body::empty())?,
        )
        .await?;
    let resources: Value =
        serde_json::from_slice(&to_bytes(unassigned.into_body(), 1024 * 1024).await?)?;
    assert_eq!(resources["items"][0]["id"], json!(legacy_account.id));
    let assign_path = format!(
        "/api/v1/organizations/{org}/quota-unassigned/service-account/{}",
        legacy_account.id
    );
    let assigned = app(state.clone(), None)
        .oneshot(
            Request::put(&assign_path)
                .header(header::ORIGIN, PUBLIC_ORIGIN)
                .header(header::COOKIE, format!("hc_session={member_token}"))
                .header("x-heterocloud-csrf", member_csrf.expose_secret())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"user_id":oidc_user.user.id}).to_string()))?,
        )
        .await?;
    assert_eq!(assigned.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        store.principal_quota_user(org, legacy_account.id).await?,
        Some(oidc_user.user.id)
    );
    let saved = app(state.clone(), None)
        .oneshot(
            Request::put(&member_path)
                .header(header::ORIGIN, PUBLIC_ORIGIN)
                .header(header::COOKIE, format!("hc_session={member_token}"))
                .header("x-heterocloud-csrf", member_csrf.expose_secret())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&personal)?))?,
        )
        .await?;
    assert_eq!(saved.status(), StatusCode::OK);
    let mut invalid_shared = personal.clone();
    invalid_shared.registry.storage_gib = quota.registry.storage_gib - 1;
    assert!(matches!(
        store
            .set_member_resource_quota(org, oidc_user.user.id, Some(&invalid_shared))
            .await,
        Err(StoreError::RequestRejected(_))
    ));
    // Allocation checks and inserts share a transaction lock: two concurrent
    // creations cannot both consume the last remaining slot.
    let project = store
        .create_project(org, "member-quota", "Member quota")
        .await?;
    let spec = json!({"region":"test","max_participants":1,"max_rooms":1,
        "rate_limit":{"requests_per_second":1,"burst":1},"metadata":{}});
    let first = store.create_service_instance(
        org,
        project.id,
        joined_membership.principal_id,
        "flow",
        "member-flow-one",
        spec.clone(),
    );
    let second = store.create_service_instance(
        org,
        project.id,
        joined_membership.principal_id,
        "flow",
        "member-flow-two",
        spec.clone(),
    );
    let (first, second) = tokio::join!(first, second);
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    let service_account_attempt = store
        .create_service_instance(
            org,
            project.id,
            legacy_account.id,
            "flow",
            "account-flow",
            spec.clone(),
        )
        .await;
    assert!(matches!(
        service_account_attempt,
        Err(StoreError::RequestRejected(_))
    ));
    let pending_before: i64 = sqlx::query_scalar("SELECT count(*) FROM outbox_events")
        .fetch_one(store.pool())
        .await?;
    let _ = store.effective_member_quota(org, oidc_user.user.id).await?;
    let pending_after: i64 = sqlx::query_scalar("SELECT count(*) FROM outbox_events")
        .fetch_one(store.pool())
        .await?;
    assert_eq!(
        pending_before, pending_after,
        "reading a quota must not enqueue reconciliations"
    );
    personal.flash.max_cpu_millis_per_vm = 500;
    personal.flash.max_total_cpu_millis = 500;
    store
        .set_member_resource_quota(org, oidc_user.user.id, Some(&personal))
        .await?;
    let flash_spec = json!({"region":"test", "image":"example/workspace:v1", "replicas":1,
        "cpu_millis":100, "memory_mib":256, "ephemeral_storage_gib":1,
        "ports":[], "exposure":{"type":"internal", "traffic_mode":"forwarded"},
        "env":{}, "command":[], "args":[], "metadata":{}});
    let mut too_large = flash_spec.clone();
    too_large["cpu_millis"] = json!(600);
    assert!(matches!(
        store
            .create_service_instance(
                org,
                project.id,
                joined_membership.principal_id,
                "flash",
                "too-large",
                too_large
            )
            .await,
        Err(StoreError::RequestRejected(_))
    ));
    let flash = store
        .create_service_instance(
            org,
            project.id,
            joined_membership.principal_id,
            "flash",
            "allowed-flash",
            flash_spec,
        )
        .await?;
    personal.flash.max_cpu_millis_per_vm = 50;
    personal.flash.max_total_cpu_millis = 50;
    store
        .set_member_resource_quota(org, oidc_user.user.id, Some(&personal))
        .await?;
    assert!(
        store
            .set_flash_service_stopped(org, flash.id, joined_membership.principal_id, true)
            .await?
            .spec["stopped"]
            == json!(true)
    );
    assert!(matches!(
        store
            .set_flash_service_stopped(org, flash.id, joined_membership.principal_id, false)
            .await,
        Err(StoreError::RequestRejected(_))
    ));
    assert_eq!(
        store
            .organization_member_quotas(org)
            .await?
            .iter()
            .find(|item| item.user_id == oidc_user.user.id)
            .ok_or("member quota")?
            .usage["flow_services"],
        json!(1)
    );
    // A different member has an independent allocation, still subject to the
    // organization ceiling.
    store
        .create_service_instance(
            org,
            project.id,
            membership.principal_id,
            "flow",
            "owner-flow",
            spec,
        )
        .await?;
    let other_org = created["id"].as_str().ok_or("created organization id")?;
    let forbidden = app(state.clone(), None)
        .oneshot(
            Request::get(format!("/api/v1/organizations/{other_org}/quotas"))
                .header(header::COOKIE, format!("hc_session={member_token}"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
    let device_response = app(state.clone(), None)
        .oneshot(
            Request::post("/api/v1/auth/cli/device")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&json!({
                    "organization_id": membership.organization_id,
                }))?))?,
        )
        .await?;
    assert_eq!(device_response.status(), StatusCode::OK);
    let device: Value =
        serde_json::from_slice(&to_bytes(device_response.into_body(), 1024 * 1024).await?)?;
    assert_eq!(
        device["verification_uri"],
        format!("{PUBLIC_ORIGIN}/cli/authorize")
    );
    assert!(
        device["verification_uri_complete"]
            .as_str()
            .is_some_and(|value| value.starts_with(PUBLIC_ORIGIN))
    );
    let user_code = device["user_code"].as_str().ok_or("missing user code")?;
    let device_code = device["device_code"]
        .as_str()
        .ok_or("missing device code")?;

    let inspect_response = app(state.clone(), None)
        .oneshot(
            Request::get(format!("/api/v1/auth/cli/device/{user_code}"))
                .header(header::COOKIE, format!("hc_session={browser_token}"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(inspect_response.status(), StatusCode::OK);

    let approve_response = app(state.clone(), None)
        .oneshot(
            Request::post("/api/v1/auth/cli/device/approve")
                .header(header::ORIGIN, PUBLIC_ORIGIN)
                .header(header::COOKIE, format!("hc_session={browser_token}"))
                .header("x-heterocloud-csrf", browser_csrf.expose_secret())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&json!({
                    "user_code": user_code,
                }))?))?,
        )
        .await?;
    assert_eq!(approve_response.status(), StatusCode::NO_CONTENT);

    let token_response = app(state.clone(), None)
        .oneshot(
            Request::post("/api/v1/auth/cli/token")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&json!({
                    "device_code": device_code,
                }))?))?,
        )
        .await?;
    assert_eq!(token_response.status(), StatusCode::OK);
    let issued: Value =
        serde_json::from_slice(&to_bytes(token_response.into_body(), 1024 * 1024).await?)?;
    let access_token = issued["access_token"]
        .as_str()
        .ok_or("missing access token")?;
    assert!(access_token.starts_with("hcu_"));
    assert_eq!(
        issued["organization"]["organization_id"],
        membership.organization_id.to_string()
    );

    let status_response = app(state.clone(), None)
        .oneshot(
            Request::get("/api/v1/auth/cli/session")
                .header(header::AUTHORIZATION, format!("Bearer {access_token}"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(status_response.status(), StatusCode::OK);

    let service_response = app(state.clone(), None)
        .oneshot(
            Request::get(format!(
                "/api/v1/organizations/{}/flash/services",
                membership.organization_id
            ))
            .header(header::AUTHORIZATION, format!("Bearer {access_token}"))
            .body(Body::empty())?,
        )
        .await?;
    assert_eq!(service_response.status(), StatusCode::OK);

    let logout_response = app(state.clone(), None)
        .oneshot(
            Request::post("/api/v1/auth/cli/logout")
                .header(header::AUTHORIZATION, format!("Bearer {access_token}"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(logout_response.status(), StatusCode::NO_CONTENT);
    let revoked_response = app(state, None)
        .oneshot(
            Request::get("/api/v1/auth/cli/session")
                .header(header::AUTHORIZATION, format!("Bearer {access_token}"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(revoked_response.status(), StatusCode::UNAUTHORIZED);

    Ok(())
}

async fn test_state() -> Result<Option<(Store, Arc<AppState>)>, Box<dyn Error>> {
    let Ok(database_url) = env::var(TEST_DATABASE_ENV) else {
        return Ok(None);
    };
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        rustls::crypto::ring::default_provider()
            .install_default()
            .map_err(|_| "failed to install the rustls ring crypto provider")?;
    }
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
        workload_identity: None,
        store: store.clone(),
        config: RuntimeConfig {
            public_origin,
            secret_manager_origin: None,
            allowed_origins: vec![PUBLIC_ORIGIN.to_owned()],
            trusted_proxy_networks: Vec::new(),
            secure_cookie: false,
            session_ttl: StdDuration::from_secs(3600),
            cli_token_ttl: StdDuration::from_secs(30 * 24 * 60 * 60),
            csrf_key: SecretString::from("test-csrf-key-at-least-32-bytes"),
            flow_access_signer: FlowAccessSigner::new(
                "heterocloud",
                "heterocloud-flow-data",
                SecretString::from("test-flow-access-secret-at-least-32-bytes"),
            )?,
            flow_public_endpoints: vec![Url::parse("http://flow.example.test")?],
            flow_internal_endpoint: Url::parse("http://flow.example.test")?,
            oidc: None,
            owner_origin: None,
            owner_email: None,
            owner_console_mode: false,
            owner_allowed_networks: Vec::new(),
        },
        flow_client: reqwest::Client::new(),
        flash_provider: None,
        vpc_provider: None,
        syouyu_provider: None,
        registry: None,
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
