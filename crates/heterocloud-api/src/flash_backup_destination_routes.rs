use super::*;
use crate::flash_provider::FlashProviderError;
use heterocloud_store::{FlashBackupDestination, NewFlashBackupDestination};
use reqwest::Method;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Credentials {
    access_key: String,
    secret_key: String,
}

fn context(org: Uuid, id: Uuid, principal: PrincipalId) -> FlashProviderContext {
    FlashProviderContext {
        principal_id: principal,
        user_id: None,
        organization_id: OrganizationId(org),
        project_id: ProjectId(Uuid::nil()),
        service_instance_id: ServiceInstanceId(id),
        generation: 1,
    }
}

fn credential_valid(value: &str) -> bool {
    !value.is_empty() && value.len() <= 4096 && !value.chars().any(char::is_control)
}

fn protected_ip(ip: std::net::IpAddr) -> bool {
    [
        "0.0.0.0/8",
        "10.0.0.0/8",
        "100.64.0.0/10",
        "127.0.0.0/8",
        "169.254.0.0/16",
        "172.16.0.0/12",
        "192.168.0.0/16",
        "224.0.0.0/4",
        "240.0.0.0/4",
        "::/128",
        "::1/128",
        "::ffff:0:0/96",
        "fc00::/7",
        "fe80::/10",
        "ff00::/8",
    ]
    .iter()
    .any(|range| {
        range
            .parse::<ipnet::IpNet>()
            .is_ok_and(|net| net.contains(&ip))
    })
}

fn validate(input: &NewFlashBackupDestination) -> Result<(), ApiError> {
    validate_name(&input.name)?;
    let origin = Url::parse(&input.endpoint)
        .map_err(|_| ApiError::BadRequest("Invalid S3 endpoint.".into()))?;
    let bucket = &input.bucket;
    let valid_bucket = (3..=63).contains(&bucket.len())
        && bucket
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && bucket
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
        && bucket
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'.'))
        && !bucket.contains("..")
        && !bucket.contains(".-")
        && !bucket.contains("-.")
        && bucket.parse::<std::net::Ipv4Addr>().is_err();
    let valid_prefix = input.prefix.len() <= 512
        && !input.prefix.starts_with('/')
        && input
            .prefix
            .split('/')
            .all(|part| part != "." && part != "..")
        && input
            .prefix
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/'));
    let host = origin
        .host_str()
        .unwrap_or_default()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    let forbidden_address = match origin.host() {
        Some(url::Host::Ipv4(ip)) => input.virtual_hosted || protected_ip(ip.into()),
        Some(url::Host::Ipv6(ip)) => input.virtual_hosted || protected_ip(ip.into()),
        _ => false,
    };
    if input.request_id.is_nil()
        || origin.scheme() != "https"
        || host.is_empty()
        || forbidden_address
        || !origin.username().is_empty()
        || origin.password().is_some()
        || !matches!(origin.path(), "" | "/")
        || origin.query().is_some()
        || origin.fragment().is_some()
        || host == "localhost"
        || [
            ".localhost",
            ".local",
            ".internal",
            ".svc",
            ".cluster.local",
        ]
        .iter()
        .any(|suffix| host.ends_with(suffix))
        || !valid_bucket
        || !valid_prefix
        || input.region.is_empty()
        || input.region.len() > 128
        || !input
            .region
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        || !credential_valid(&input.access_key)
        || !credential_valid(&input.secret_key)
    {
        return Err(ApiError::BadRequest(
            "Invalid S3 destination. Use a public HTTPS endpoint, bucket, region and credentials."
                .into(),
        ));
    }
    Ok(())
}

fn verified_ready(value: &Value, org: Uuid, id: Uuid) -> Result<bool, ApiError> {
    if value["id"].as_str() != Some(id.to_string().as_str())
        || value["organization_id"].as_str() != Some(org.to_string().as_str())
    {
        return Err(ApiError::FlashProviderUnavailable);
    }
    value["ready"]
        .as_bool()
        .ok_or(ApiError::FlashProviderUnavailable)
}

async fn apply(
    state: &AppState,
    org: Uuid,
    principal: PrincipalId,
    destination: FlashBackupDestination,
    access: &str,
    secret: &str,
) -> Result<FlashBackupDestination, ApiError> {
    if !credential_valid(access) || !credential_valid(secret) {
        return Err(ApiError::BadRequest("Invalid S3 credentials.".into()));
    }
    // Serialize credential writes for this destination across API replicas.
    // Otherwise two concurrent rotations can leave the durable secret and
    // the provider with different keys depending on network completion order.
    let mut tx = state
        .store
        .pool()
        .begin()
        .await
        .map_err(|_| ApiError::Internal)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::uuid::text, 7))")
        .bind(destination.id)
        .execute(&mut *tx)
        .await
        .map_err(|_| ApiError::Internal)?;
    // Store the only durable copy in the secret manager, not PostgreSQL.
    let credential = serde_json::to_string(&json!({"access_key":access,"secret_key":secret}))
        .map_err(|_| ApiError::Internal)?;
    flash_secret_manager(state)
        .await?
        .put(destination.id, "backup-s3", &credential)
        .await
        .map_err(map_secret_error)?;
    let provider = state
        .flash_provider
        .as_deref()
        .ok_or(ApiError::FlashProviderUnavailable)?;
    let body = json!({"endpoint":destination.endpoint,"bucket":destination.bucket,"region":destination.region,
        "prefix":destination.prefix,"virtual_hosted":destination.virtual_hosted,"access_key":access,"secret_key":secret});
    let remote = provider
        .backup_destination_request(
            context(org, destination.id, principal),
            Method::PUT,
            Some(&body),
        )
        .await
        .map_err(|error| match error {
            FlashProviderError::ProviderStatus(400) => {
                ApiError::BadRequest("Invalid S3 destination.".into())
            }
            FlashProviderError::ProviderStatus(409) => ApiError::Conflict,
            _ => ApiError::FlashProviderUnavailable,
        })?;
    let ready = verified_ready(&remote, org, destination.id)?;
    let updated = sqlx::query_as::<_, FlashBackupDestination>("UPDATE flash_backup_destinations SET state=$3,updated_at=now() WHERE organization_id=$1 AND id=$2 RETURNING *")
        .bind(org).bind(destination.id).bind(if ready { "ready" } else { "pending" })
        .fetch_optional(&mut *tx).await.map_err(|_| ApiError::Internal)?.ok_or(ApiError::NotFound)?;
    tx.commit().await.map_err(|_| ApiError::Internal)?;
    Ok(updated)
}

pub(super) async fn list(
    State(state): State<Arc<AppState>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Json<Value>, ApiError> {
    let actor = authenticated_actor(&state, &headers, &jar).await?;
    authorize_actor(
        &state,
        &actor,
        OrganizationId(org),
        "flash:ListBackups",
        &format!("hc:org:{org}:flash/backup/*"),
    )
    .await?;
    let items = state
        .store
        .flash_backup_destinations(OrganizationId(org))
        .await
        .map_err(ApiError::from_store)?;
    Ok(Json(json!({"items":items})))
}

pub(super) async fn create(
    State(state): State<Arc<AppState>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(input): Json<NewFlashBackupDestination>,
) -> Result<impl IntoResponse, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    let authorization = authorize_actor(
        &state,
        &actor,
        OrganizationId(org),
        "flash:ManageBackupDestinations",
        &format!("hc:org:{org}:flash/backup-destination/*"),
    )
    .await?;
    validate(&input)?;
    let destination = state
        .store
        .reserve_flash_backup_destination(OrganizationId(org), authorization.principal_id, &input)
        .await
        .map_err(ApiError::from_store)?;
    let destination = apply(
        &state,
        org,
        authorization.principal_id,
        destination,
        &input.access_key,
        &input.secret_key,
    )
    .await?;
    Ok((StatusCode::ACCEPTED, Json(destination)))
}

pub(super) async fn rotate_credentials(
    State(state): State<Arc<AppState>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(credentials): Json<Credentials>,
) -> Result<Json<FlashBackupDestination>, ApiError> {
    let actor = authenticated_actor_mutation(&state, &headers, &jar).await?;
    let authorization = authorize_actor(
        &state,
        &actor,
        OrganizationId(org),
        "flash:ManageBackupDestinations",
        &format!("hc:org:{org}:flash/backup-destination/{id}"),
    )
    .await?;
    let destination = state
        .store
        .flash_backup_destination(OrganizationId(org), id)
        .await
        .map_err(ApiError::from_store)?;
    Ok(Json(
        apply(
            &state,
            org,
            authorization.principal_id,
            destination,
            &credentials.access_key,
            &credentials.secret_key,
        )
        .await?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn destination() -> NewFlashBackupDestination {
        NewFlashBackupDestination {
            name: "offsite".into(),
            request_id: Uuid::now_v7(),
            endpoint: "https://s3.example.com".into(),
            bucket: "backup-bucket".into(),
            region: "ap-northeast-1".into(),
            prefix: "backups".into(),
            virtual_hosted: false,
            access_key: "fake-access".into(),
            secret_key: "fake-secret".into(),
        }
    }

    #[test]
    fn rejects_private_targets_and_unsafe_paths_before_reserving() {
        assert!(validate(&destination()).is_ok());
        for endpoint in [
            "http://s3.example.com",
            "https://127.0.0.1",
            "https://169.254.169.254",
            "https://service.namespace.svc.cluster.local",
            "https://user:password@s3.example.com",
            "https://s3.example.com/path",
        ] {
            let mut input = destination();
            input.endpoint = endpoint.into();
            assert!(validate(&input).is_err(), "accepted {endpoint}");
        }
        for prefix in ["/other", "a/../other", "a/./other", "with spaces"] {
            let mut input = destination();
            input.prefix = prefix.into();
            assert!(validate(&input).is_err(), "accepted {prefix}");
        }
    }

    #[test]
    fn refuses_provider_response_for_another_tenant() {
        let org = Uuid::now_v7();
        let id = Uuid::now_v7();
        assert!(
            verified_ready(
                &json!({"id":id,"organization_id":org,"ready":true}),
                org,
                id
            )
            .is_ok_and(|v| v)
        );
        assert!(
            verified_ready(
                &json!({"id":id,"organization_id":Uuid::now_v7(),"ready":true}),
                org,
                id
            )
            .is_err()
        );
    }
}
