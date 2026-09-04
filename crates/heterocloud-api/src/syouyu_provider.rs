use std::{collections::BTreeSet, sync::Arc, time::Duration};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use heterocloud_domain::{OrganizationId, PrincipalId, ProjectId, ServiceInstanceId};
use hmac::{Hmac, Mac};
use reqwest::{Client, RequestBuilder, Response, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::Sha256;
use url::Url;
use uuid::Uuid;

const PRINCIPAL_HEADER: &str = "x-syouyu-principal";
const PRINCIPAL_TIMESTAMP_HEADER: &str = "x-syouyu-timestamp";
const PRINCIPAL_SIGNATURE_HEADER: &str = "x-syouyu-signature";
const IDEMPOTENCY_KEY_HEADER: &str = "idempotency-key";
const PRINCIPAL_TTL: Duration = Duration::from_secs(60);
const MAX_RESPONSE_BYTES: usize = 64 * 1024;

pub struct SyouyuProviderProxy {
    endpoint: Url,
    signer: SyouyuPrincipalSigner,
    client: Client,
}

impl SyouyuProviderProxy {
    pub fn new(
        endpoint: Url,
        issuer: impl Into<String>,
        audience: impl Into<String>,
        secret: SecretString,
        client: Client,
    ) -> Result<Self, SyouyuProviderError> {
        if !matches!(endpoint.scheme(), "http" | "https")
            || !endpoint.has_host()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || !matches!(endpoint.path(), "" | "/")
        {
            return Err(SyouyuProviderError::InvalidConfiguration(
                "Syouyu endpoint must be an absolute HTTP(S) base URL",
            ));
        }
        let signer = SyouyuPrincipalSigner::new(issuer, audience, secret, PRINCIPAL_TTL)?;
        Ok(Self {
            endpoint,
            signer,
            client,
        })
    }

    pub async fn service_overview(
        &self,
        context: &SyouyuProviderContext,
    ) -> Result<SyouyuServiceOverview, SyouyuProviderError> {
        let response = self
            .authenticated(
                self.client.get(self.endpoint.join("v1/service-overview")?),
                context,
            )?
            .send()
            .await?;
        decode_response(response).await
    }

    pub async fn usage(
        &self,
        context: &SyouyuProviderContext,
    ) -> Result<SyouyuProviderUsage, SyouyuProviderError> {
        let response = self
            .authenticated(self.client.get(self.endpoint.join("v1/usage")?), context)?
            .send()
            .await?;
        decode_response(response).await
    }

    pub async fn list_credentials(
        &self,
        context: &SyouyuProviderContext,
    ) -> Result<Vec<SyouyuProviderCredential>, SyouyuProviderError> {
        let response = self
            .authenticated(
                self.client.get(self.endpoint.join("v1/credentials")?),
                context,
            )?
            .send()
            .await?;
        let response: CredentialListResponse = decode_response(response).await?;
        Ok(response.credentials)
    }

    pub async fn create_credential(
        &self,
        context: &SyouyuProviderContext,
        idempotency_key: Uuid,
        name: &str,
        permissions: SyouyuProviderPermissions,
    ) -> Result<IssuedSyouyuProviderCredential, SyouyuProviderError> {
        let response = self
            .authenticated(
                self.client
                    .post(self.endpoint.join("v1/credentials")?)
                    .header(IDEMPOTENCY_KEY_HEADER, idempotency_key.to_string())
                    .json(&CreateCredentialRequest { name, permissions }),
                context,
            )?
            .send()
            .await?;
        decode_response(response).await
    }

    pub async fn revoke_credential(
        &self,
        context: &SyouyuProviderContext,
        credential_id: Uuid,
        idempotency_key: Uuid,
    ) -> Result<(), SyouyuProviderError> {
        let response = self
            .authenticated(
                self.client
                    .delete(
                        self.endpoint
                            .join(&format!("v1/credentials/{credential_id}"))?,
                    )
                    .header(IDEMPOTENCY_KEY_HEADER, idempotency_key.to_string()),
                context,
            )?
            .send()
            .await?;
        let revoked: RevokedCredentialResponse = decode_response(response).await?;
        if revoked.credential_id != credential_id || revoked.status != "revoked" {
            return Err(SyouyuProviderError::InvalidResponse);
        }
        Ok(())
    }

    fn authenticated(
        &self,
        request: RequestBuilder,
        context: &SyouyuProviderContext,
    ) -> Result<RequestBuilder, SyouyuProviderError> {
        let signed = self.signer.sign(context)?;
        Ok(request
            .header(PRINCIPAL_HEADER, signed.encoded)
            .header(PRINCIPAL_TIMESTAMP_HEADER, signed.timestamp)
            .header(PRINCIPAL_SIGNATURE_HEADER, signed.signature))
    }
}

#[derive(Clone, Debug)]
pub struct SyouyuProviderContext {
    pub principal_id: PrincipalId,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub service_instance_id: ServiceInstanceId,
    pub permissions: BTreeSet<String>,
    pub credential_limits: SyouyuCredentialLimits,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SyouyuCredentialLimits {
    pub max_credentials_per_bucket: u32,
    pub max_total_credentials: u32,
}

#[derive(Clone)]
pub(crate) struct SyouyuPrincipalSigner {
    issuer: Arc<str>,
    audience: Arc<str>,
    secret: SecretString,
    ttl: Duration,
}

impl SyouyuPrincipalSigner {
    pub(crate) fn new(
        issuer: impl Into<String>,
        audience: impl Into<String>,
        secret: SecretString,
        ttl: Duration,
    ) -> Result<Self, SyouyuProviderError> {
        let issuer = issuer.into();
        let audience = audience.into();
        if issuer.is_empty() || audience.is_empty() {
            return Err(SyouyuProviderError::InvalidConfiguration(
                "Syouyu principal issuer and audience are required",
            ));
        }
        if secret.expose_secret().len() < 32 {
            return Err(SyouyuProviderError::InvalidConfiguration(
                "Syouyu principal HMAC secret must contain at least 32 bytes",
            ));
        }
        if ttl.is_zero() || ttl > Duration::from_secs(300) {
            return Err(SyouyuProviderError::InvalidConfiguration(
                "Syouyu principal TTL must be between one and 300 seconds",
            ));
        }
        Ok(Self {
            issuer: issuer.into(),
            audience: audience.into(),
            secret,
            ttl,
        })
    }

    fn sign(
        &self,
        context: &SyouyuProviderContext,
    ) -> Result<SignedSyouyuPrincipal, SyouyuProviderError> {
        let issued_at = u64::try_from(Utc::now().timestamp())
            .map_err(|_| SyouyuProviderError::InvalidSystemTime)?;
        self.sign_at(context, issued_at)
    }

    fn sign_at(
        &self,
        context: &SyouyuProviderContext,
        issued_at: u64,
    ) -> Result<SignedSyouyuPrincipal, SyouyuProviderError> {
        if context.principal_id.0.is_nil()
            || context.organization_id.0.is_nil()
            || context.project_id.0.is_nil()
            || context.service_instance_id.0.is_nil()
            || context.permissions.is_empty()
            || context.credential_limits.max_credentials_per_bucket == 0
            || context.credential_limits.max_total_credentials
                < context.credential_limits.max_credentials_per_bucket
        {
            return Err(SyouyuProviderError::InvalidContext);
        }
        let expires_at = issued_at
            .checked_add(self.ttl.as_secs())
            .ok_or(SyouyuProviderError::InvalidSystemTime)?;
        let context_id = Uuid::new_v4();
        let principal = PrincipalPayload {
            issuer: &self.issuer,
            audience: &self.audience,
            organization_id: context.organization_id.0,
            project_id: context.project_id.0,
            service_instance_id: context.service_instance_id.0,
            principal_id: context.principal_id.0,
            permissions: &context.permissions,
            issued_at,
            expires_at,
            context_id,
            credential_limits: context.credential_limits,
        };
        let encoded = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&principal)?);
        let timestamp = issued_at.to_string();
        let mut mac = Hmac::<Sha256>::new_from_slice(self.secret.expose_secret().as_bytes())
            .map_err(|_| {
                SyouyuProviderError::InvalidConfiguration("Syouyu HMAC secret is invalid")
            })?;
        mac.update(timestamp.as_bytes());
        mac.update(b".");
        mac.update(encoded.as_bytes());
        let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
        Ok(SignedSyouyuPrincipal {
            encoded,
            timestamp,
            signature,
        })
    }
}

struct SignedSyouyuPrincipal {
    encoded: String,
    timestamp: String,
    signature: String,
}

#[derive(Serialize)]
struct PrincipalPayload<'a> {
    issuer: &'a str,
    audience: &'a str,
    organization_id: Uuid,
    project_id: Uuid,
    service_instance_id: Uuid,
    principal_id: Uuid,
    permissions: &'a BTreeSet<String>,
    issued_at: u64,
    expires_at: u64,
    context_id: Uuid,
    credential_limits: SyouyuCredentialLimits,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SyouyuProviderPermissions {
    #[serde(default)]
    pub read: bool,
    #[serde(default)]
    pub write: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct SyouyuProviderCredential {
    pub id: Uuid,
    pub name: String,
    pub access_key_id: String,
    pub permissions: SyouyuProviderPermissions,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

pub struct IssuedSyouyuProviderCredential {
    pub credential: SyouyuProviderCredential,
    pub secret_access_key: SecretString,
    pub bucket_name: String,
    pub endpoint: String,
}

impl<'de> Deserialize<'de> for IssuedSyouyuProviderCredential {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct WireResponse {
            credential: SyouyuProviderCredential,
            secret_access_key: SecretString,
            bucket_name: String,
            endpoint: String,
        }

        let wire = WireResponse::deserialize(deserializer)?;
        Ok(Self {
            credential: wire.credential,
            secret_access_key: wire.secret_access_key,
            bucket_name: wire.bucket_name,
            endpoint: wire.endpoint,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct SyouyuServiceOverview {
    pub service_instance_id: Uuid,
    pub name: String,
    pub region: String,
    pub bucket_name: String,
    pub phase: String,
    pub endpoint: String,
    pub quota: SyouyuProviderQuota,
    pub usage: SyouyuProviderBucketUsage,
    pub active_credentials: u64,
    pub measured_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub struct SyouyuProviderQuota {
    pub bytes: u64,
    pub objects: u64,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct SyouyuProviderBucketUsage {
    pub bytes: u64,
    pub objects: u64,
    pub unfinished_upload_bytes: u64,
    pub unfinished_uploads: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct SyouyuProviderUsage {
    pub service_instance_id: Uuid,
    pub bytes_used: u64,
    pub objects_used: u64,
    pub unfinished_upload_bytes: u64,
    pub unfinished_uploads: u64,
    pub quota_bytes: u64,
    pub quota_objects: u64,
    pub measured_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct CreateCredentialRequest<'a> {
    name: &'a str,
    permissions: SyouyuProviderPermissions,
}

#[derive(Deserialize)]
struct CredentialListResponse {
    credentials: Vec<SyouyuProviderCredential>,
}

#[derive(Deserialize)]
struct RevokedCredentialResponse {
    credential_id: Uuid,
    status: String,
}

#[derive(Deserialize)]
struct ProviderErrorEnvelope {
    error: ProviderErrorBody,
}

#[derive(Deserialize)]
struct ProviderErrorBody {
    code: String,
}

async fn decode_response<T: DeserializeOwned>(
    response: Response,
) -> Result<T, SyouyuProviderError> {
    let status = response.status();
    let bytes = response.bytes().await?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(SyouyuProviderError::InvalidResponse);
    }
    if !status.is_success() {
        let code = serde_json::from_slice::<ProviderErrorEnvelope>(&bytes)
            .ok()
            .map(|body| body.error.code);
        return Err(SyouyuProviderError::Rejected(classify_rejection(
            status,
            code.as_deref(),
        )));
    }
    serde_json::from_slice(&bytes).map_err(|_| SyouyuProviderError::InvalidResponse)
}

fn classify_rejection(status: StatusCode, code: Option<&str>) -> SyouyuProviderRejection {
    match code {
        Some("not_found") => return SyouyuProviderRejection::NotFound,
        Some("credential_limit_exceeded" | "idempotency_conflict" | "service_not_ready") => {
            return SyouyuProviderRejection::Conflict;
        }
        Some("invalid_request") => return SyouyuProviderRejection::InvalidRequest,
        Some("invalid_credentials" | "permission_denied") => {
            return SyouyuProviderRejection::Authentication;
        }
        _ => {}
    }
    match status {
        StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY => {
            SyouyuProviderRejection::InvalidRequest
        }
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => SyouyuProviderRejection::Authentication,
        StatusCode::NOT_FOUND => SyouyuProviderRejection::NotFound,
        StatusCode::CONFLICT => SyouyuProviderRejection::Conflict,
        StatusCode::TOO_MANY_REQUESTS => SyouyuProviderRejection::RateLimited,
        _ => SyouyuProviderRejection::Unavailable,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyouyuProviderRejection {
    InvalidRequest,
    Authentication,
    NotFound,
    Conflict,
    RateLimited,
    Unavailable,
}

#[derive(Debug, thiserror::Error)]
pub enum SyouyuProviderError {
    #[error("invalid Syouyu provider configuration: {0}")]
    InvalidConfiguration(&'static str),
    #[error("Syouyu principal context is invalid")]
    InvalidContext,
    #[error("system time cannot be represented for Syouyu authentication")]
    InvalidSystemTime,
    #[error("Syouyu provider response is invalid")]
    InvalidResponse,
    #[error("Syouyu provider rejected the request: {0:?}")]
    Rejected(SyouyuProviderRejection),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Url(#[from] url::ParseError),
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use axum::{
        Json, Router,
        extract::State,
        http::{HeaderMap, StatusCode},
        routing::{delete, get, post},
    };
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use hmac::{Hmac, Mac};
    use secrecy::{ExposeSecret, SecretString};
    use serde_json::{Value, json};
    use sha2::Sha256;
    use tokio::net::TcpListener;
    use url::Url;
    use uuid::Uuid;

    use super::{
        IDEMPOTENCY_KEY_HEADER, PRINCIPAL_HEADER, PRINCIPAL_SIGNATURE_HEADER,
        PRINCIPAL_TIMESTAMP_HEADER, PrincipalPayload, SyouyuCredentialLimits,
        SyouyuPrincipalSigner, SyouyuProviderContext, SyouyuProviderPermissions,
        SyouyuProviderProxy, SyouyuProviderRejection, classify_rejection,
    };
    use heterocloud_domain::{OrganizationId, PrincipalId, ProjectId, ServiceInstanceId};

    const SECRET: &str = "syouyu-proxy-test-secret-at-least-32-bytes";

    fn context(permission: &str) -> SyouyuProviderContext {
        SyouyuProviderContext {
            principal_id: PrincipalId(Uuid::from_u128(1)),
            organization_id: OrganizationId(Uuid::from_u128(2)),
            project_id: ProjectId(Uuid::from_u128(3)),
            service_instance_id: ServiceInstanceId(Uuid::from_u128(4)),
            permissions: BTreeSet::from([permission.to_owned()]),
            credential_limits: SyouyuCredentialLimits {
                max_credentials_per_bucket: 7,
                max_total_credentials: 70,
            },
        }
    }

    #[test]
    fn signer_emits_exact_short_lived_hmac_contract() -> Result<(), Box<dyn std::error::Error>> {
        let signer = SyouyuPrincipalSigner::new(
            "heterocloud",
            "heterocloud-syouyu-data",
            SecretString::from(SECRET),
            Duration::from_secs(60),
        )?;
        let signed = signer.sign_at(&context("syouyu.usage.read"), 1_788_480_000)?;
        let payload_bytes = URL_SAFE_NO_PAD.decode(&signed.encoded)?;
        let payload: Value = serde_json::from_slice(&payload_bytes)?;
        assert_eq!(payload["issuer"], "heterocloud");
        assert_eq!(payload["audience"], "heterocloud-syouyu-data");
        assert_eq!(payload["organization_id"], Uuid::from_u128(2).to_string());
        assert_eq!(payload["project_id"], Uuid::from_u128(3).to_string());
        assert_eq!(
            payload["service_instance_id"],
            Uuid::from_u128(4).to_string()
        );
        assert_eq!(payload["principal_id"], Uuid::from_u128(1).to_string());
        assert_eq!(payload["permissions"], json!(["syouyu.usage.read"]));
        assert_eq!(payload["issued_at"], 1_788_480_000_u64);
        assert_eq!(payload["expires_at"], 1_788_480_060_u64);
        assert_eq!(
            payload["credential_limits"]["max_credentials_per_bucket"],
            7
        );
        assert_eq!(payload["credential_limits"]["max_total_credentials"], 70);
        assert!(payload["context_id"].as_str().is_some());

        let mut mac = Hmac::<Sha256>::new_from_slice(SECRET.as_bytes())?;
        mac.update(signed.timestamp.as_bytes());
        mac.update(b".");
        mac.update(signed.encoded.as_bytes());
        mac.verify_slice(&URL_SAFE_NO_PAD.decode(signed.signature)?)?;
        Ok(())
    }

    #[derive(Clone)]
    struct MockState {
        secret: SecretString,
        expected_create_idempotency_key: Uuid,
        expected_revoke_idempotency_key: Uuid,
        create_calls: Arc<AtomicUsize>,
        revoke_calls: Arc<AtomicUsize>,
    }

    async fn assert_auth(
        headers: &HeaderMap,
        state: &MockState,
        permission: &str,
    ) -> Result<Value, StatusCode> {
        let principal = headers
            .get(PRINCIPAL_HEADER)
            .and_then(|value| value.to_str().ok())
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
        let timestamp = headers
            .get(PRINCIPAL_TIMESTAMP_HEADER)
            .and_then(|value| value.to_str().ok())
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
        let signature = headers
            .get(PRINCIPAL_SIGNATURE_HEADER)
            .and_then(|value| value.to_str().ok())
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
        let mut mac = Hmac::<Sha256>::new_from_slice(state.secret.expose_secret().as_bytes())
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        mac.update(timestamp.as_bytes());
        mac.update(b".");
        mac.update(principal.as_bytes());
        let signature = URL_SAFE_NO_PAD
            .decode(signature)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        mac.verify_slice(&signature)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let principal = URL_SAFE_NO_PAD
            .decode(principal)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let payload: Value =
            serde_json::from_slice(&principal).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        assert_eq!(payload["permissions"], json!([permission]));
        Ok(payload)
    }

    async fn overview(
        State(state): State<MockState>,
        headers: HeaderMap,
    ) -> Result<Json<Value>, StatusCode> {
        let payload = assert_auth(&headers, &state, "syouyu.overview.read").await?;
        Ok(Json(json!({
            "service_instance_id": payload["service_instance_id"],
            "name": "assets",
            "region": "heteronet-global",
            "bucket_name": "assets-0001",
            "phase": "ready",
            "endpoint": "https://s3.example.test/",
            "quota": {"bytes": 1048576, "objects": 100},
            "usage": {"bytes": 42, "objects": 2, "unfinished_upload_bytes": 0, "unfinished_uploads": 0},
            "active_credentials": 1,
            "measured_at": "2026-09-04T00:00:00Z"
        })))
    }

    async fn create(
        State(state): State<MockState>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> Result<(StatusCode, Json<Value>), StatusCode> {
        assert_auth(&headers, &state, "syouyu.credential.create").await?;
        let idempotency_key = headers
            .get(IDEMPOTENCY_KEY_HEADER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| Uuid::parse_str(value).ok());
        assert_eq!(idempotency_key, Some(state.expected_create_idempotency_key));
        state.create_calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(body["permissions"], json!({"read": true, "write": false}));
        Ok((
            StatusCode::CREATED,
            Json(json!({
                "credential": {
                    "id": Uuid::from_u128(5),
                    "name": body["name"],
                    "access_key_id": "GK-test",
                    "permissions": body["permissions"],
                    "status": "active",
                    "created_at": "2026-09-04T00:00:00Z",
                    "revoked_at": null
                },
                "secret_access_key": "one-time-secret",
                "bucket_name": "assets-0001",
                "endpoint": "https://s3.example.test/"
            })),
        ))
    }

    async fn revoke(
        State(state): State<MockState>,
        headers: HeaderMap,
    ) -> Result<Json<Value>, StatusCode> {
        assert_auth(&headers, &state, "syouyu.credential.revoke").await?;
        assert_eq!(
            headers
                .get(IDEMPOTENCY_KEY_HEADER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| Uuid::parse_str(value).ok()),
            Some(state.expected_revoke_idempotency_key)
        );
        state.revoke_calls.fetch_add(1, Ordering::SeqCst);
        Ok(Json(
            json!({"credential_id": Uuid::from_u128(5), "status": "revoked"}),
        ))
    }

    #[tokio::test]
    async fn proxy_forwards_outer_idempotency_key_for_create_and_revoke_replays()
    -> Result<(), Box<dyn std::error::Error>> {
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            let _install_result = rustls::crypto::ring::default_provider().install_default();
        }
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            return Err("failed to install the rustls crypto provider".into());
        }
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let create_idempotency_key = Uuid::from_u128(6);
        let revoke_idempotency_key = Uuid::from_u128(7);
        let create_calls = Arc::new(AtomicUsize::new(0));
        let revoke_calls = Arc::new(AtomicUsize::new(0));
        let router = Router::new()
            .route("/v1/service-overview", get(overview))
            .route("/v1/credentials", post(create))
            .route("/v1/credentials/{credential_id}", delete(revoke))
            .with_state(MockState {
                secret: SecretString::from(SECRET),
                expected_create_idempotency_key: create_idempotency_key,
                expected_revoke_idempotency_key: revoke_idempotency_key,
                create_calls: Arc::clone(&create_calls),
                revoke_calls: Arc::clone(&revoke_calls),
            });
        let server = tokio::spawn(async move { axum::serve(listener, router).await });
        let proxy = SyouyuProviderProxy::new(
            Url::parse(&format!("http://{address}/"))?,
            "heterocloud",
            "heterocloud-syouyu-data",
            SecretString::from(SECRET),
            reqwest::Client::new(),
        )?;

        let overview = proxy
            .service_overview(&context("syouyu.overview.read"))
            .await?;
        assert_eq!(overview.bucket_name, "assets-0001");
        let first = proxy
            .create_credential(
                &context("syouyu.credential.create"),
                create_idempotency_key,
                "flash-workspace",
                SyouyuProviderPermissions {
                    read: true,
                    write: false,
                },
            )
            .await?;
        let replay = proxy
            .create_credential(
                &context("syouyu.credential.create"),
                create_idempotency_key,
                "flash-workspace",
                SyouyuProviderPermissions {
                    read: true,
                    write: false,
                },
            )
            .await?;
        assert_eq!(first.secret_access_key.expose_secret(), "one-time-secret");
        assert_eq!(
            replay.secret_access_key.expose_secret(),
            first.secret_access_key.expose_secret()
        );
        assert_eq!(first.credential.id, replay.credential.id);
        assert_eq!(create_calls.load(Ordering::SeqCst), 2);
        proxy
            .revoke_credential(
                &context("syouyu.credential.revoke"),
                Uuid::from_u128(5),
                revoke_idempotency_key,
            )
            .await?;
        proxy
            .revoke_credential(
                &context("syouyu.credential.revoke"),
                Uuid::from_u128(5),
                revoke_idempotency_key,
            )
            .await?;
        assert_eq!(revoke_calls.load(Ordering::SeqCst), 2);
        server.abort();
        Ok(())
    }

    #[test]
    fn payload_shape_matches_syouyu_authenticator() -> Result<(), Box<dyn std::error::Error>> {
        let payload = PrincipalPayload {
            issuer: "heterocloud",
            audience: "heterocloud-syouyu-data",
            organization_id: Uuid::from_u128(1),
            project_id: Uuid::from_u128(2),
            service_instance_id: Uuid::from_u128(3),
            principal_id: Uuid::from_u128(4),
            permissions: &BTreeSet::from(["syouyu.credential.read".into()]),
            issued_at: 1,
            expires_at: 61,
            context_id: Uuid::from_u128(5),
            credential_limits: SyouyuCredentialLimits {
                max_credentials_per_bucket: 10,
                max_total_credentials: 100,
            },
        };
        let value = serde_json::to_value(payload)?;
        assert_eq!(value.as_object().map(serde_json::Map::len), Some(11));
        Ok(())
    }

    #[test]
    fn structured_provider_errors_map_without_forwarding_provider_messages() {
        assert_eq!(
            classify_rejection(
                StatusCode::SERVICE_UNAVAILABLE,
                Some("credential_limit_exceeded")
            ),
            SyouyuProviderRejection::Conflict
        );
        assert_eq!(
            classify_rejection(StatusCode::BAD_REQUEST, Some("permission_denied")),
            SyouyuProviderRejection::Authentication
        );
        assert_eq!(
            classify_rejection(
                StatusCode::SERVICE_UNAVAILABLE,
                Some("backend-secret-details")
            ),
            SyouyuProviderRejection::Unavailable
        );
    }
}
