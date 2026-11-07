use super::*;
use russh::keys::{HashAlg, PublicKey, ssh_key::public::KeyData};

const MAX_PUBLIC_KEY_BYTES: usize = 16 * 1024;
const MAX_GITHUB_RESPONSE_BYTES: usize = 128 * 1024;
static GITHUB_IMPORT_LIMITER: Semaphore = Semaphore::const_new(8);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NewKey {
    name: String,
    public_key: String,
    #[serde(default)]
    github_username: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GithubPreview {
    username: String,
}

#[derive(Serialize)]
struct PreviewKey {
    public_key: String,
    fingerprint: String,
}

fn valid_github_username(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 39
        && !value.starts_with('-')
        && !value.ends_with('-')
        && !value.contains("--")
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

fn parse_key(value: &str) -> Result<PreviewKey, ApiError> {
    let value = value.trim();
    if value.len() > MAX_PUBLIC_KEY_BYTES || value.contains(['\n', '\r', '\0']) {
        return Err(ApiError::BadRequest("Invalid SSH public key".into()));
    }
    let key = PublicKey::from_openssh(value)
        .map_err(|_| ApiError::BadRequest("Invalid SSH public key".into()))?;
    match key.key_data() {
        KeyData::Ed25519(_) | KeyData::Ecdsa(_) => {}
        KeyData::Rsa(rsa) if rsa.key_size() >= 2048 => {}
        _ => return Err(ApiError::BadRequest("Unsupported SSH public key".into())),
    }
    Ok(PreviewKey {
        public_key: key.to_openssh().map_err(|_| ApiError::Internal)?,
        fingerprint: key.fingerprint(HashAlg::Sha256).to_string(),
    })
}

async fn human(
    state: &AppState,
    actor: &AuthenticatedActor,
    org: OrganizationId,
) -> Result<(UserId, AuthorizationContext), ApiError> {
    let user = match actor {
        AuthenticatedActor::User(session) => session.user.user.id,
        AuthenticatedActor::CliToken(token) if token.organization_id == org => token.user.user.id,
        // A temporary role or workload credential must not mint a durable SSH key.
        _ => return Err(ApiError::Forbidden),
    };
    let context = state
        .store
        .authorization_context(user, org)
        .await
        .map_err(ApiError::from_store)?
        .ok_or(ApiError::Forbidden)?;
    Ok((user, context))
}

async fn key_audit(
    state: &AppState,
    org: OrganizationId,
    user: UserId,
    principal: PrincipalId,
    action: &str,
    key_id: Uuid,
) -> Result<(), ApiError> {
    state
        .store
        .append_audit(&AuditEvent {
            organization_id: Some(org),
            principal_id: Some(principal),
            user_id: Some(user),
            request_id: &Uuid::now_v7().to_string(),
            source_ip: None,
            action,
            resource: &format!("urn:heterocloud:flash:{org}:ssh-key:{key_id}"),
            decision: "allow",
            reason: "self_service",
            metadata: json!({"key_id":key_id}),
        })
        .await
        .map_err(ApiError::from_store)
}

pub(super) async fn list_keys(
    State(state): State<Arc<AppState>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Response, ApiError> {
    let actor = authenticated_actor(&state, &headers, &jar).await?;
    human(&state, &actor, OrganizationId(org)).await?;
    let items = state
        .store
        .flash_ssh_keys(OrganizationId(org))
        .await
        .map_err(ApiError::from_store)?;
    Ok(no_store_response(Json(json!({"items":items}))))
}

pub(super) async fn gateway_info(
    State(state): State<Arc<AppState>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Response, ApiError> {
    let actor = authenticated_actor(&state, &headers, &jar).await?;
    human(&state, &actor, OrganizationId(org)).await?;
    let fingerprint = state
        .ssh_gateway
        .as_ref()
        .map(|gateway| gateway.fingerprint.as_str());
    let tcp = state
        .ssh_gateway
        .as_ref()
        .and_then(|gateway| gateway.advertised_tcp.as_ref())
        .map(|(host, port)| json!({"host":host,"port":port}));
    Ok(no_store_response(Json(
        json!({"enabled":fingerprint.is_some(),"host_key_fingerprint":fingerprint,"tcp":tcp}),
    )))
}

pub(super) async fn create_key(
    State(state): State<Arc<AppState>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(input): Json<NewKey>,
) -> Result<Response, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    let org = OrganizationId(org);
    let (user, context) = human(&state, &actor, org).await?;
    if input.name.trim() != input.name || input.name.is_empty() || input.name.chars().count() > 120
    {
        return Err(ApiError::BadRequest(
            "SSH key name must contain 1–120 characters".into(),
        ));
    }
    if input
        .github_username
        .as_deref()
        .is_some_and(|s| !valid_github_username(s))
    {
        return Err(ApiError::BadRequest("Invalid GitHub username".into()));
    }
    let key = parse_key(&input.public_key)?;
    if let Some(username) = input.github_username.as_deref() {
        // Do not allow an arbitrary pasted key to be presented as imported
        // from another GitHub account. Revalidate at the time of registration.
        let github_keys = fetch_github_keys(username).await?;
        if !github_keys
            .iter()
            .any(|candidate| candidate.fingerprint == key.fingerprint)
        {
            return Err(ApiError::BadRequest(
                "GitHub の公開鍵が変更されています。もう一度検索してください".into(),
            ));
        }
    }
    let item = state
        .store
        .create_flash_ssh_key(
            org,
            user,
            &input.name,
            &key.public_key,
            &key.fingerprint,
            input.github_username.as_deref(),
        )
        .await
        .map_err(ApiError::from_store)?;
    key_audit(
        &state,
        org,
        user,
        context.principal_id,
        "flash:RegisterSshKey",
        item.id,
    )
    .await?;
    Ok(no_store_response((StatusCode::CREATED, Json(item))))
}

pub(super) async fn delete_key(
    State(state): State<Arc<AppState>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<StatusCode, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    let org = OrganizationId(org);
    let (user, context) = human(&state, &actor, org).await?;
    state
        .store
        .delete_flash_ssh_key(org, user, id)
        .await
        .map_err(ApiError::from_store)?;
    key_audit(
        &state,
        org,
        user,
        context.principal_id,
        "flash:DeleteSshKey",
        id,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn github_preview(
    State(state): State<Arc<AppState>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(input): Json<GithubPreview>,
) -> Result<Response, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    human(&state, &actor, OrganizationId(org)).await?;
    let items = fetch_github_keys(&input.username).await?;
    Ok(no_store_response(Json(
        json!({"username":input.username,"items":items}),
    )))
}

async fn fetch_github_keys(username: &str) -> Result<Vec<PreviewKey>, ApiError> {
    if !valid_github_username(username) {
        return Err(ApiError::BadRequest("Invalid GitHub username".into()));
    }
    let _permit = GITHUB_IMPORT_LIMITER
        .try_acquire()
        .map_err(|_| ApiError::TooManyRequests)?;
    let client = reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(10))
        .user_agent("HeteroCloud-SSH-key-import")
        .build()
        .map_err(|_| ApiError::Internal)?;
    let url = format!("https://api.github.com/users/{}/keys?per_page=50", username);
    let mut response = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .map_err(|_| ApiError::BadRequest("GitHub の公開鍵を取得できませんでした".into()))?;
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|n| n > MAX_GITHUB_RESPONSE_BYTES as u64)
    {
        return Err(ApiError::BadRequest(
            "GitHub の公開鍵を取得できませんでした".into(),
        ));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| ApiError::BadRequest("GitHub の公開鍵を取得できませんでした".into()))?
    {
        if body.len().saturating_add(chunk.len()) > MAX_GITHUB_RESPONSE_BYTES {
            return Err(ApiError::BadRequest("GitHub の応答が大きすぎます".into()));
        }
        body.extend_from_slice(&chunk);
    }
    let values: Vec<Value> = serde_json::from_slice(&body)
        .map_err(|_| ApiError::BadRequest("GitHub の応答が不正です".into()))?;
    if values.len() > 50 {
        return Err(ApiError::BadRequest("GitHub の公開鍵が多すぎます".into()));
    }
    let items: Vec<PreviewKey> = values
        .iter()
        .filter_map(|v| v.get("key")?.as_str())
        .filter_map(|v| parse_key(v).ok())
        .collect();
    Ok(items)
}

pub(super) async fn service_keys(
    State(state): State<Arc<AppState>>,
    Path((org, service)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Response, ApiError> {
    let actor = authenticated_actor(&state, &headers, &jar).await?;
    authorize_actor(
        &state,
        &actor,
        OrganizationId(org),
        "flash:GetInstance",
        &flash_service_resource(org, service),
    )
    .await?;
    flash_service(&state, org, service).await?;
    let ids = state
        .store
        .flash_service_ssh_key_ids(OrganizationId(org), ServiceInstanceId(service))
        .await
        .map_err(ApiError::from_store)?;
    Ok(no_store_response(Json(json!({"key_ids":ids}))))
}

pub(super) async fn attach_service_key(
    State(state): State<Arc<AppState>>,
    Path((org, service, key)): Path<(Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Response, ApiError> {
    change_service_key(state, org, service, key, true, headers, jar).await
}

pub(super) async fn detach_service_key(
    State(state): State<Arc<AppState>>,
    Path((org, service, key)): Path<(Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Response, ApiError> {
    change_service_key(state, org, service, key, false, headers, jar).await
}

async fn change_service_key(
    state: Arc<AppState>,
    org: Uuid,
    service: Uuid,
    key: Uuid,
    enabled: bool,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Response, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    let authorization = authorize_actor(
        &state,
        &actor,
        OrganizationId(org),
        "flash:UpdateInstance",
        &flash_service_resource(org, service),
    )
    .await?;
    let ids = state
        .store
        .set_flash_service_ssh_key(
            OrganizationId(org),
            ServiceInstanceId(service),
            key,
            enabled,
        )
        .await
        .map_err(ApiError::from_store)?;
    let user_id = match &actor {
        AuthenticatedActor::User(session) => Some(session.user.user.id),
        AuthenticatedActor::CliToken(token) => Some(token.user.user.id),
        _ => None,
    };
    state
        .store
        .append_audit(&AuditEvent {
            organization_id: Some(OrganizationId(org)),
            principal_id: Some(authorization.principal_id),
            user_id,
            request_id: &Uuid::now_v7().to_string(),
            source_ip: None,
            action: "flash:SetSshKeys",
            resource: &flash_service_resource(org, service),
            decision: "allow",
            reason: "authorized_service_update",
            metadata: json!({"key_id":key,"enabled":enabled}),
        })
        .await
        .map_err(ApiError::from_store)?;
    Ok(no_store_response(Json(json!({"key_ids":ids}))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_username_cannot_escape_fixed_api_origin() {
        for value in [
            "", "-name", "name-", "a--b", "../admin", "a?x=y", "a/b", "a@host", "a.b", "あ",
        ] {
            assert!(!valid_github_username(value));
        }
        for value in ["a", "mizuame-d", "A123"] {
            assert!(valid_github_username(value));
        }
    }

    #[test]
    fn public_key_parser_rejects_options_multiline_and_bad_data() {
        assert!(parse_key("ssh-ed25519 broken").is_err());
        assert!(parse_key("command=\"echo hi\" ssh-ed25519 AAAA").is_err());
        assert!(parse_key("ssh-ed25519 AAAA\nssh-ed25519 AAAA").is_err());
    }

    #[test]
    fn public_key_fingerprint_is_independent_of_the_comment() -> Result<(), ApiError> {
        let key =
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILM+rvN+ot98qgEN796jTiQfZfG1KaT0PtFDJ/XFSqti";
        let first = parse_key(&format!("{key} first"))?;
        let second = parse_key(&format!("{key} second"))?;
        assert_eq!(first.fingerprint, second.fingerprint);
        assert!(first.fingerprint.starts_with("SHA256:"));
        Ok(())
    }
}
