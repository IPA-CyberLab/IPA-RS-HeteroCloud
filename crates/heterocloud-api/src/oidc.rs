use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum_extra::extract::cookie::{Cookie, SameSite};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use heterocloud_auth::{constant_time_token_eq, generate_token};
use hmac::{Hmac, Mac};
use jsonwebtoken::{
    Algorithm, DecodingKey, Validation, decode, decode_header,
    jwk::{AlgorithmParameters, JwkSet, KeyOperations, PublicKeyUse},
};
use reqwest::{Client, Response, redirect::Policy as RedirectPolicy};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use thiserror::Error;
use time::Duration as CookieDuration;
use url::Url;

pub const OIDC_TRANSACTION_COOKIE: &str = "hc_oidc_transaction";
const OIDC_CALLBACK_PATH: &str = "/api/v1/auth/oidc/callback";
const OIDC_TRANSACTION_TTL_SECONDS: u64 = 5 * 60;
const OIDC_CLOCK_SKEW_SECONDS: u64 = 60;
const MAX_DISCOVERY_BYTES: usize = 256 * 1024;
const MAX_TOKEN_RESPONSE_BYTES: usize = 256 * 1024;
const MAX_JWKS_BYTES: usize = 1024 * 1024;
const MAX_ID_TOKEN_BYTES: usize = 128 * 1024;
const MAX_CALLBACK_CODE_BYTES: usize = 16 * 1024;
const MAX_JWKS_KEYS: usize = 128;

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
pub struct OidcConfig {
    issuer: String,
    discovery_issuer: String,
    discovery_allow_insecure_http: bool,
    client_id: String,
    client_secret: SecretString,
    public_callback_url: Url,
    allow_insecure_http: bool,
    client: Client,
}

impl OidcConfig {
    pub fn new(
        issuer: Url,
        backchannel_issuer: Option<Url>,
        client_id: String,
        client_secret: SecretString,
        public_callback_url: Url,
        allow_insecure_http: bool,
    ) -> Result<Self, OidcConfigError> {
        let issuer = normalize_issuer(issuer, allow_insecure_http)?;
        let (discovery_issuer, discovery_allow_insecure_http) =
            if let Some(backchannel_issuer) = backchannel_issuer {
                (normalize_backchannel_issuer(backchannel_issuer)?, true)
            } else {
                (issuer.clone(), allow_insecure_http)
            };
        let client_id = client_id.trim().to_owned();
        if client_id.is_empty() || client_id.len() > 256 || client_id.chars().any(char::is_control)
        {
            return Err(OidcConfigError::InvalidClientId);
        }
        if client_secret.expose_secret().len() < 16 || client_secret.expose_secret().len() > 4096 {
            return Err(OidcConfigError::InvalidClientSecret);
        }
        validate_callback_url(&public_callback_url, allow_insecure_http)?;
        let client = Client::builder()
            .redirect(RedirectPolicy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .user_agent("heterocloud-api/oidc")
            .build()
            .map_err(|_| OidcConfigError::HttpClient)?;
        Ok(Self {
            issuer,
            discovery_issuer,
            discovery_allow_insecure_http,
            client_id,
            client_secret,
            public_callback_url,
            allow_insecure_http,
            client,
        })
    }

    #[must_use]
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    pub async fn begin_login(
        &self,
        cookie_signing_key: &SecretString,
        secure_cookie: bool,
        intent: OidcLoginIntent,
    ) -> Result<OidcLoginStart, OidcError> {
        let discovery = self.discovery().await?;
        let state = generate_token().map_err(|_| OidcError::Internal)?;
        let nonce = generate_token().map_err(|_| OidcError::Internal)?;
        let verifier = generate_token().map_err(|_| OidcError::Internal)?;
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.expose_secret().as_bytes()));
        let issued_at = unix_timestamp().map_err(|_| OidcError::Internal)?;
        let transaction = OidcTransaction {
            state: state.expose_secret().to_owned(),
            nonce: nonce.expose_secret().to_owned(),
            verifier: verifier.expose_secret().to_owned(),
            issued_at,
        };
        let cookie_value = sign_transaction(&transaction, cookie_signing_key)?;
        let cookie = Cookie::build((OIDC_TRANSACTION_COOKIE, cookie_value))
            .path(OIDC_CALLBACK_PATH)
            .http_only(true)
            .secure(secure_cookie)
            .same_site(SameSite::Lax)
            .max_age(CookieDuration::seconds(
                i64::try_from(OIDC_TRANSACTION_TTL_SECONDS).unwrap_or(i64::MAX),
            ))
            .build();

        let mut authorization_url = discovery.authorization_endpoint;
        authorization_url
            .query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &self.client_id)
            .append_pair("redirect_uri", self.public_callback_url.as_str())
            .append_pair("scope", "openid profile email")
            .append_pair("state", state.expose_secret())
            .append_pair("nonce", nonce.expose_secret())
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256");
        if intent == OidcLoginIntent::Register {
            authorization_url
                .query_pairs_mut()
                .append_pair("prompt", "create");
        }
        Ok(OidcLoginStart {
            authorization_url,
            transaction_cookie: cookie,
        })
    }

    pub async fn complete_login(
        &self,
        query: &OidcCallbackQuery,
        transaction_cookie: Option<&str>,
        cookie_signing_key: &SecretString,
    ) -> Result<OidcIdentity, OidcError> {
        query.validate_size()?;
        let transaction_cookie = transaction_cookie.ok_or(OidcError::InvalidRequest)?;
        let transaction = verify_transaction(transaction_cookie, cookie_signing_key)?;
        let state = query.state.as_deref().ok_or(OidcError::InvalidRequest)?;
        if !constant_time_token_eq(state, &transaction.state) {
            return Err(OidcError::InvalidRequest);
        }
        validate_transaction_age(transaction.issued_at)?;
        if query.error.is_some() {
            return Err(OidcError::AuthorizationRejected);
        }
        let code = query.code.as_deref().ok_or(OidcError::InvalidRequest)?;
        if code.is_empty() {
            return Err(OidcError::InvalidRequest);
        }

        let discovery = self.discovery().await?;
        let token_response = self
            .client
            .post(discovery.token_endpoint)
            .basic_auth(&self.client_id, Some(self.client_secret.expose_secret()))
            .header(reqwest::header::ACCEPT, "application/json")
            .form(&[
                ("grant_type", "authorization_code"),
                ("client_id", self.client_id.as_str()),
                ("code", code),
                ("redirect_uri", self.public_callback_url.as_str()),
                ("code_verifier", transaction.verifier.as_str()),
            ])
            .send()
            .await
            .map_err(|_| OidcError::ProviderUnavailable)?;
        if token_response.status().is_client_error() {
            return Err(OidcError::AuthorizationRejected);
        }
        if !token_response.status().is_success() {
            return Err(OidcError::ProviderUnavailable);
        }
        let tokens: TokenResponse = bounded_json(token_response, MAX_TOKEN_RESPONSE_BYTES).await?;
        if tokens.id_token.is_empty() || tokens.id_token.len() > MAX_ID_TOKEN_BYTES {
            return Err(OidcError::InvalidToken);
        }

        let jwks_response = self
            .client
            .get(discovery.jwks_uri)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|_| OidcError::ProviderUnavailable)?;
        if !jwks_response.status().is_success() {
            return Err(OidcError::ProviderUnavailable);
        }
        let jwks: JwkSet = bounded_json(jwks_response, MAX_JWKS_BYTES).await?;
        if jwks.keys.is_empty() || jwks.keys.len() > MAX_JWKS_KEYS {
            return Err(OidcError::InvalidToken);
        }
        self.validate_id_token(
            &tokens.id_token,
            &transaction.nonce,
            &discovery.id_token_signing_alg_values_supported,
            &jwks,
        )
    }

    fn validate_id_token(
        &self,
        id_token: &str,
        expected_nonce: &str,
        advertised_algorithms: &[String],
        jwks: &JwkSet,
    ) -> Result<OidcIdentity, OidcError> {
        let header = decode_header(id_token).map_err(|_| OidcError::InvalidToken)?;
        if !supported_asymmetric_algorithm(header.alg)
            || (!advertised_algorithms.is_empty()
                && !advertised_algorithms
                    .iter()
                    .any(|value| value == algorithm_name(header.alg)))
        {
            return Err(OidcError::InvalidToken);
        }
        let key_id = header.kid.as_deref().ok_or(OidcError::InvalidToken)?;
        if key_id.is_empty() || key_id.len() > 256 {
            return Err(OidcError::InvalidToken);
        }
        let mut matching_keys = jwks.keys.iter().filter(|key| {
            key.common.key_id.as_deref() == Some(key_id)
                && key
                    .common
                    .public_key_use
                    .as_ref()
                    .is_none_or(|usage| usage == &PublicKeyUse::Signature)
                && key
                    .common
                    .key_operations
                    .as_ref()
                    .is_none_or(|operations| operations.contains(&KeyOperations::Verify))
                && key
                    .common
                    .key_algorithm
                    .is_none_or(|algorithm| algorithm == header.alg.into())
                && !matches!(key.algorithm, AlgorithmParameters::OctetKey(_))
        });
        let key = matching_keys.next().ok_or(OidcError::InvalidToken)?;
        if matching_keys.next().is_some() {
            return Err(OidcError::InvalidToken);
        }
        let decoding_key = DecodingKey::from_jwk(key).map_err(|_| OidcError::InvalidToken)?;
        let mut validation = Validation::new(header.alg);
        validation.leeway = OIDC_CLOCK_SKEW_SECONDS;
        validation.validate_nbf = true;
        validation.set_audience(std::slice::from_ref(&self.client_id));
        validation.set_issuer(std::slice::from_ref(&self.issuer));
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        let claims = decode::<IdTokenClaims>(id_token, &decoding_key, &validation)
            .map_err(|_| OidcError::InvalidToken)?
            .claims;

        if !constant_time_token_eq(&claims.nonce, expected_nonce)
            || claims.subject.is_empty()
            || claims.subject.chars().count() > 255
            || claims.subject.chars().any(char::is_control)
            || claims.email.is_empty()
            || claims.email.len() > 320
        {
            return Err(OidcError::InvalidToken);
        }
        let now = unix_timestamp().map_err(|_| OidcError::Internal)?;
        if claims.issued_at > now.saturating_add(OIDC_CLOCK_SKEW_SECONDS)
            || claims.expires_at <= claims.issued_at
        {
            return Err(OidcError::InvalidToken);
        }
        let audiences = claims.audience.values();
        if audiences.is_empty()
            || (audiences.len() > 1 && claims.authorized_party.as_deref() != Some(&self.client_id))
            || claims
                .authorized_party
                .as_deref()
                .is_some_and(|party| party != self.client_id)
        {
            return Err(OidcError::InvalidToken);
        }
        let display_name = display_name(
            claims.name.as_deref(),
            claims.preferred_username.as_deref(),
            &claims.email,
        );
        Ok(OidcIdentity {
            issuer: self.issuer.clone(),
            subject: claims.subject,
            email: claims.email,
            display_name,
        })
    }

    async fn discovery(&self) -> Result<ProviderMetadata, OidcError> {
        let discovery_url = Url::parse(&format!(
            "{}/.well-known/openid-configuration",
            self.discovery_issuer
        ))
        .map_err(|_| OidcError::Internal)?;
        let response = self
            .client
            .get(discovery_url)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|_| OidcError::ProviderUnavailable)?;
        if !response.status().is_success() {
            return Err(OidcError::ProviderUnavailable);
        }
        let metadata: ProviderMetadata = bounded_json(response, MAX_DISCOVERY_BYTES).await?;
        if metadata.issuer != self.issuer
            || !valid_provider_endpoint(
                &metadata.authorization_endpoint,
                &self.issuer,
                self.allow_insecure_http,
            )
            || !valid_provider_endpoint(
                &metadata.token_endpoint,
                &self.discovery_issuer,
                self.discovery_allow_insecure_http,
            )
            || !valid_provider_endpoint(
                &metadata.jwks_uri,
                &self.discovery_issuer,
                self.discovery_allow_insecure_http,
            )
        {
            return Err(OidcError::ProviderUnavailable);
        }
        Ok(metadata)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum OidcLoginIntent {
    Authenticate,
    Register,
}

#[derive(Debug)]
pub struct OidcLoginStart {
    pub authorization_url: Url,
    pub transaction_cookie: Cookie<'static>,
}

#[derive(Debug, Deserialize)]
pub struct OidcCallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

impl OidcCallbackQuery {
    fn validate_size(&self) -> Result<(), OidcError> {
        if self
            .code
            .as_ref()
            .is_some_and(|value| value.len() > MAX_CALLBACK_CODE_BYTES)
            || self.state.as_ref().is_some_and(|value| value.len() > 128)
            || self.error.as_ref().is_some_and(|value| value.len() > 1024)
            || self
                .error_description
                .as_ref()
                .is_some_and(|value| value.len() > 4096)
        {
            return Err(OidcError::InvalidRequest);
        }
        Ok(())
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct OidcIdentity {
    pub issuer: String,
    pub subject: String,
    pub email: String,
    pub display_name: String,
}

#[derive(Debug, Error)]
pub enum OidcConfigError {
    #[error("OIDC issuer URL must be an HTTP(S) URL without credentials, query, or fragment")]
    InvalidIssuer,
    #[error(
        "OIDC backchannel issuer URL must be an HTTP(S) URL without credentials, query, or fragment"
    )]
    InvalidBackchannelIssuer,
    #[error("OIDC client ID must be 1..256 non-control characters")]
    InvalidClientId,
    #[error("OIDC client secret must contain between 16 and 4096 bytes")]
    InvalidClientSecret,
    #[error("OIDC public callback URL must end at /api/v1/auth/oidc/callback")]
    InvalidCallbackUrl,
    #[error("failed to create the OIDC HTTP client")]
    HttpClient,
}

#[derive(Debug, Error)]
pub enum OidcError {
    #[error("OIDC authorization was rejected")]
    AuthorizationRejected,
    #[error("invalid or expired OIDC login transaction")]
    InvalidRequest,
    #[error("OIDC identity token validation failed")]
    InvalidToken,
    #[error("identity provider is unavailable")]
    ProviderUnavailable,
    #[error("internal OIDC error")]
    Internal,
}

#[derive(Deserialize)]
struct ProviderMetadata {
    issuer: String,
    authorization_endpoint: Url,
    token_endpoint: Url,
    jwks_uri: Url,
    id_token_signing_alg_values_supported: Vec<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    id_token: String,
}

#[derive(Deserialize)]
struct IdTokenClaims {
    #[serde(rename = "iss")]
    _issuer: String,
    #[serde(rename = "sub")]
    subject: String,
    #[serde(rename = "aud")]
    audience: Audience,
    #[serde(rename = "exp")]
    expires_at: u64,
    #[serde(rename = "iat")]
    issued_at: u64,
    nonce: String,
    email: String,
    name: Option<String>,
    preferred_username: Option<String>,
    #[serde(rename = "azp")]
    authorized_party: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

impl Audience {
    fn values(&self) -> &[String] {
        match self {
            Self::One(value) => std::slice::from_ref(value),
            Self::Many(values) => values,
        }
    }
}

#[derive(Deserialize, Serialize)]
struct OidcTransaction {
    state: String,
    nonce: String,
    verifier: String,
    issued_at: u64,
}

fn normalize_issuer(issuer: Url, allow_insecure_http: bool) -> Result<String, OidcConfigError> {
    if !matches!(issuer.scheme(), "http" | "https")
        || (!allow_insecure_http && issuer.scheme() != "https")
        || !issuer.has_host()
        || !issuer.username().is_empty()
        || issuer.password().is_some()
        || issuer.query().is_some()
        || issuer.fragment().is_some()
        || issuer.as_str().len() > 2048
    {
        return Err(OidcConfigError::InvalidIssuer);
    }
    Ok(issuer.as_str().trim_end_matches('/').to_owned())
}

fn normalize_backchannel_issuer(issuer: Url) -> Result<String, OidcConfigError> {
    if !matches!(issuer.scheme(), "http" | "https")
        || !issuer.has_host()
        || !issuer.username().is_empty()
        || issuer.password().is_some()
        || issuer.query().is_some()
        || issuer.fragment().is_some()
        || issuer.as_str().len() > 2048
    {
        return Err(OidcConfigError::InvalidBackchannelIssuer);
    }
    Ok(issuer.as_str().trim_end_matches('/').to_owned())
}

fn validate_callback_url(callback: &Url, allow_insecure_http: bool) -> Result<(), OidcConfigError> {
    if !matches!(callback.scheme(), "http" | "https")
        || (!allow_insecure_http && callback.scheme() != "https")
        || !callback.has_host()
        || !callback.username().is_empty()
        || callback.password().is_some()
        || callback.query().is_some()
        || callback.fragment().is_some()
        || callback.path() != OIDC_CALLBACK_PATH
    {
        return Err(OidcConfigError::InvalidCallbackUrl);
    }
    Ok(())
}

fn valid_provider_endpoint(endpoint: &Url, issuer: &str, allow_insecure_http: bool) -> bool {
    let Ok(issuer) = Url::parse(issuer) else {
        return false;
    };
    matches!(endpoint.scheme(), "http" | "https")
        && (allow_insecure_http || endpoint.scheme() == "https")
        && endpoint.has_host()
        && endpoint.username().is_empty()
        && endpoint.password().is_none()
        && endpoint.fragment().is_none()
        && endpoint.origin() == issuer.origin()
}

fn supported_asymmetric_algorithm(algorithm: Algorithm) -> bool {
    matches!(
        algorithm,
        Algorithm::RS256
            | Algorithm::RS384
            | Algorithm::RS512
            | Algorithm::PS256
            | Algorithm::PS384
            | Algorithm::PS512
            | Algorithm::ES256
            | Algorithm::ES384
            | Algorithm::EdDSA
    )
}

fn algorithm_name(algorithm: Algorithm) -> &'static str {
    match algorithm {
        Algorithm::RS256 => "RS256",
        Algorithm::RS384 => "RS384",
        Algorithm::RS512 => "RS512",
        Algorithm::PS256 => "PS256",
        Algorithm::PS384 => "PS384",
        Algorithm::PS512 => "PS512",
        Algorithm::ES256 => "ES256",
        Algorithm::ES384 => "ES384",
        Algorithm::EdDSA => "EdDSA",
        _ => "unsupported",
    }
}

fn sign_transaction(
    transaction: &OidcTransaction,
    signing_key: &SecretString,
) -> Result<String, OidcError> {
    let payload = serde_json::to_vec(transaction).map_err(|_| OidcError::Internal)?;
    let payload = URL_SAFE_NO_PAD.encode(payload);
    let mut mac = HmacSha256::new_from_slice(signing_key.expose_secret().as_bytes())
        .map_err(|_| OidcError::Internal)?;
    mac.update(b"heterocloud-oidc-transaction-v1\0");
    mac.update(payload.as_bytes());
    let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    Ok(format!("{payload}.{signature}"))
}

fn verify_transaction(
    value: &str,
    signing_key: &SecretString,
) -> Result<OidcTransaction, OidcError> {
    if value.len() > 2048 {
        return Err(OidcError::InvalidRequest);
    }
    let (payload, signature) = value.split_once('.').ok_or(OidcError::InvalidRequest)?;
    if signature.contains('.') {
        return Err(OidcError::InvalidRequest);
    }
    let signature = URL_SAFE_NO_PAD
        .decode(signature)
        .map_err(|_| OidcError::InvalidRequest)?;
    let mut mac = HmacSha256::new_from_slice(signing_key.expose_secret().as_bytes())
        .map_err(|_| OidcError::Internal)?;
    mac.update(b"heterocloud-oidc-transaction-v1\0");
    mac.update(payload.as_bytes());
    mac.verify_slice(&signature)
        .map_err(|_| OidcError::InvalidRequest)?;
    let payload = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| OidcError::InvalidRequest)?;
    let transaction: OidcTransaction =
        serde_json::from_slice(&payload).map_err(|_| OidcError::InvalidRequest)?;
    if transaction.state.len() != 43
        || transaction.nonce.len() != 43
        || transaction.verifier.len() != 43
    {
        return Err(OidcError::InvalidRequest);
    }
    Ok(transaction)
}

fn validate_transaction_age(issued_at: u64) -> Result<(), OidcError> {
    let now = unix_timestamp().map_err(|_| OidcError::Internal)?;
    if issued_at > now.saturating_add(OIDC_CLOCK_SKEW_SECONDS)
        || now.saturating_sub(issued_at) > OIDC_TRANSACTION_TTL_SECONDS
    {
        return Err(OidcError::InvalidRequest);
    }
    Ok(())
}

fn display_name(name: Option<&str>, username: Option<&str>, email: &str) -> String {
    for candidate in [name, username, Some(email)].into_iter().flatten() {
        let normalized = candidate.trim();
        if !normalized.is_empty() && !normalized.chars().any(char::is_control) {
            return normalized.chars().take(120).collect();
        }
    }
    "OIDC user".to_owned()
}

async fn bounded_json<T: DeserializeOwned>(
    mut response: Response,
    limit: usize,
) -> Result<T, OidcError> {
    if response
        .content_length()
        .is_some_and(|length| length > u64::try_from(limit).unwrap_or(u64::MAX))
    {
        return Err(OidcError::ProviderUnavailable);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| OidcError::ProviderUnavailable)?
    {
        if body.len().saturating_add(chunk.len()) > limit {
            return Err(OidcError::ProviderUnavailable);
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|_| OidcError::ProviderUnavailable)
}

fn unix_timestamp() -> Result<u64, std::time::SystemTimeError> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}

#[must_use]
pub fn clear_transaction_cookie(secure: bool) -> Cookie<'static> {
    Cookie::build((OIDC_TRANSACTION_COOKIE, ""))
        .path(OIDC_CALLBACK_PATH)
        .http_only(true)
        .secure(secure)
        .same_site(SameSite::Lax)
        .max_age(CookieDuration::ZERO)
        .build()
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, error::Error};

    use axum::{
        Form, Json, Router,
        extract::State,
        http::StatusCode,
        routing::{get, post},
    };
    use axum_extra::extract::cookie::SameSite;
    use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
    use secrecy::SecretString;
    use serde::Serialize;
    use serde_json::{Value, json};
    use tokio::net::TcpListener;

    use super::{OidcCallbackQuery, OidcConfig, OidcError, OidcLoginIntent, unix_timestamp};

    const TEST_CLIENT_ID: &str = "heterocloud-test";
    const TEST_KEY_ID: &str = "test-rsa-key";
    const TEST_RSA_MODULUS: &str = "yRE6rHuNR0QbHO3H3Kt2pOKGVhQqGZXInOduQNxXzuKlvQTLUTv4l4sggh5_CYYi_cvI-SXVT9kPWSKXxJXBXd_4LkvcPuUakBoAkfh-eiFVMh2VrUyWyj3MFl0HTVF9KwRXLAcwkREiS3npThHRyIxuy0ZMeZfxVL5arMhw1SRELB8HoGfG_AtH89BIE9jDBHZ9dLelK9a184zAf8LwoPLxvJb3Il5nncqPcSfKDDodMFBIMc4lQzDKL5gvmiXLXB1AGLm8KBjfE8s3L5xqi-yUod-j8MtvIj812dkS4QMiRVN_by2h3ZY8LYVGrqZXZTcgn2ujn8uKjXLZVD5TdQ";
    const TEST_RSA_PRIVATE_KEY: &str = r#"-----BEGIN PRIVATE KEY-----
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQDJETqse41HRBsc
7cfcq3ak4oZWFCoZlcic525A3FfO4qW9BMtRO/iXiyCCHn8JhiL9y8j5JdVP2Q9Z
IpfElcFd3/guS9w+5RqQGgCR+H56IVUyHZWtTJbKPcwWXQdNUX0rBFcsBzCRESJL
eelOEdHIjG7LRkx5l/FUvlqsyHDVJEQsHwegZ8b8C0fz0EgT2MMEdn10t6Ur1rXz
jMB/wvCg8vG8lvciXmedyo9xJ8oMOh0wUEgxziVDMMovmC+aJctcHUAYubwoGN8T
yzcvnGqL7JSh36Pwy28iPzXZ2RLhAyJFU39vLaHdljwthUaupldlNyCfa6Ofy4qN
ctlUPlN1AgMBAAECggEAdESTQjQ70O8QIp1ZSkCYXeZjuhj081CK7jhhp/4ChK7J
GlFQZMwiBze7d6K84TwAtfQGZhQ7km25E1kOm+3hIDCoKdVSKch/oL54f/BK6sKl
qlIzQEAenho4DuKCm3I4yAw9gEc0DV70DuMTR0LEpYyXcNJY3KNBOTjN5EYQAR9s
2MeurpgK2MdJlIuZaIbzSGd+diiz2E6vkmcufJLtmYUT/k/ddWvEtz+1DnO6bRHh
xuuDMeJA/lGB/EYloSLtdyCF6sII6C6slJJtgfb0bPy7l8VtL5iDyz46IKyzdyzW
tKAn394dm7MYR1RlUBEfqFUyNK7C+pVMVoTwCC2V4QKBgQD64syfiQ2oeUlLYDm4
CcKSP3RnES02bcTyEDFSuGyyS1jldI4A8GXHJ/lG5EYgiYa1RUivge4lJrlNfjyf
dV230xgKms7+JiXqag1FI+3mqjAgg4mYiNjaao8N8O3/PD59wMPeWYImsWXNyeHS
55rUKiHERtCcvdzKl4u35ZtTqQKBgQDNKnX2bVqOJ4WSqCgHRhOm386ugPHfy+8j
m6cicmUR46ND6ggBB03bCnEG9OtGisxTo/TuYVRu3WP4KjoJs2LD5fwdwJqpgtHl
yVsk45Y1Hfo+7M6lAuR8rzCi6kHHNb0HyBmZjysHWZsn79ZM+sQnLpgaYgQGRbKV
DZWlbw7g7QKBgQCl1u+98UGXAP1jFutwbPsx40IVszP4y5ypCe0gqgon3UiY/G+1
zTLp79GGe/SjI2VpQ7AlW7TI2A0bXXvDSDi3/5Dfya9ULnFXv9yfvH1QwWToySpW
Kvd1gYSoiX84/WCtjZOr0e0HmLIb0vw0hqZA4szJSqoxQgvF22EfIWaIaQKBgQCf
34+OmMYw8fEvSCPxDxVvOwW2i7pvV14hFEDYIeZKW2W1HWBhVMzBfFB5SE8yaCQy
pRfOzj9aKOCm2FjjiErVNpkQoi6jGtLvScnhZAt/lr2TXTrl8OwVkPrIaN0bG/AS
aUYxmBPCpXu3UjhfQiWqFq/mFyzlqlgvuCc9g95HPQKBgAscKP8mLxdKwOgX8yFW
GcZ0izY/30012ajdHY+/QK5lsMoxTnn0skdS+spLxaS5ZEO4qvPVb8RAoCkWMMal
2pOhmquJQVDPDLuZHdrIiKiDM20dy9sMfHygWcZjQ4WSxf/J7T9canLZIXFhHAZT
3wc9h4G8BBCtWN2TN/LsGZdB
-----END PRIVATE KEY-----"#;

    #[derive(Clone)]
    struct TestProvider {
        issuer: String,
        endpoint_origin: String,
    }

    #[derive(Serialize)]
    struct TestClaims<'a> {
        iss: &'a str,
        sub: &'a str,
        aud: &'a str,
        exp: u64,
        iat: u64,
        nonce: &'a str,
        email: &'a str,
        name: &'a str,
    }

    #[tokio::test]
    async fn authorization_code_pkce_validates_state_signature_audience_and_nonce()
    -> Result<(), Box<dyn Error>> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let issuer = format!("http://{}", listener.local_addr()?);
        let provider = TestProvider {
            issuer: issuer.clone(),
            endpoint_origin: issuer.clone(),
        };
        let app = Router::new()
            .route("/.well-known/openid-configuration", get(discovery))
            .route("/token", post(token))
            .route("/jwks", get(jwks))
            .with_state(provider);
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let config = OidcConfig::new(
            issuer.parse()?,
            None,
            TEST_CLIENT_ID.to_owned(),
            SecretString::from("test-client-secret-value"),
            "http://console.example.test/api/v1/auth/oidc/callback".parse()?,
            true,
        )?;
        let cookie_key = SecretString::from("test-cookie-key-with-at-least-32-bytes");
        let start = config
            .begin_login(&cookie_key, false, OidcLoginIntent::Authenticate)
            .await?;
        let parameters: HashMap<String, String> =
            start.authorization_url.query_pairs().into_owned().collect();
        let state = parameters.get("state").ok_or("missing state")?.clone();
        let nonce = parameters.get("nonce").ok_or("missing nonce")?.clone();
        assert_eq!(
            parameters.get("response_type").map(String::as_str),
            Some("code")
        );
        assert_eq!(
            parameters.get("code_challenge_method").map(String::as_str),
            Some("S256")
        );
        assert!(parameters.contains_key("code_challenge"));
        assert!(!parameters.contains_key("prompt"));
        assert_eq!(start.transaction_cookie.http_only(), Some(true));
        assert_eq!(start.transaction_cookie.same_site(), Some(SameSite::Lax));

        let register_start = config
            .begin_login(&cookie_key, false, OidcLoginIntent::Register)
            .await?;
        let register_parameters: HashMap<String, String> = register_start
            .authorization_url
            .query_pairs()
            .into_owned()
            .collect();
        assert_eq!(
            register_parameters.get("prompt").map(String::as_str),
            Some("create")
        );

        let identity = config
            .complete_login(
                &OidcCallbackQuery {
                    code: Some(nonce.clone()),
                    state: Some(state.clone()),
                    error: None,
                    error_description: None,
                },
                Some(start.transaction_cookie.value()),
                &cookie_key,
            )
            .await?;
        assert_eq!(identity.issuer, issuer);
        assert_eq!(identity.subject, "keycloak-subject");
        assert_eq!(identity.email, "oidc-user@example.test");
        assert_eq!(identity.display_name, "OIDC Test User");

        let wrong_nonce = config
            .complete_login(
                &OidcCallbackQuery {
                    code: Some("wrong-nonce".to_owned()),
                    state: Some(state.clone()),
                    error: None,
                    error_description: None,
                },
                Some(start.transaction_cookie.value()),
                &cookie_key,
            )
            .await;
        assert!(matches!(wrong_nonce, Err(OidcError::InvalidToken)));

        let audience_start = config
            .begin_login(&cookie_key, false, OidcLoginIntent::Authenticate)
            .await?;
        let audience_parameters: HashMap<String, String> = audience_start
            .authorization_url
            .query_pairs()
            .into_owned()
            .collect();
        let audience_state = audience_parameters
            .get("state")
            .ok_or("missing audience state")?;
        let audience_nonce = audience_parameters
            .get("nonce")
            .ok_or("missing audience nonce")?;
        let wrong_audience = config
            .complete_login(
                &OidcCallbackQuery {
                    code: Some(format!("wrong-audience:{audience_nonce}")),
                    state: Some(audience_state.clone()),
                    error: None,
                    error_description: None,
                },
                Some(audience_start.transaction_cookie.value()),
                &cookie_key,
            )
            .await;
        assert!(matches!(wrong_audience, Err(OidcError::InvalidToken)));

        let signature_start = config
            .begin_login(&cookie_key, false, OidcLoginIntent::Authenticate)
            .await?;
        let signature_parameters: HashMap<String, String> = signature_start
            .authorization_url
            .query_pairs()
            .into_owned()
            .collect();
        let signature_state = signature_parameters
            .get("state")
            .ok_or("missing signature state")?;
        let signature_nonce = signature_parameters
            .get("nonce")
            .ok_or("missing signature nonce")?;
        let invalid_signature = config
            .complete_login(
                &OidcCallbackQuery {
                    code: Some(format!("invalid-signature:{signature_nonce}")),
                    state: Some(signature_state.clone()),
                    error: None,
                    error_description: None,
                },
                Some(signature_start.transaction_cookie.value()),
                &cookie_key,
            )
            .await;
        assert!(matches!(invalid_signature, Err(OidcError::InvalidToken)));

        let mut tampered_cookie = start.transaction_cookie.value().to_owned();
        tampered_cookie.push('x');
        let tampered = config
            .complete_login(
                &OidcCallbackQuery {
                    code: Some(nonce),
                    state: Some(state),
                    error: None,
                    error_description: None,
                },
                Some(&tampered_cookie),
                &cookie_key,
            )
            .await;
        assert!(matches!(tampered, Err(OidcError::InvalidRequest)));

        server.abort();
        Ok(())
    }

    #[tokio::test]
    async fn backchannel_discovery_keeps_browser_authorization_public() -> Result<(), Box<dyn Error>>
    {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let endpoint_origin = format!("http://{}", listener.local_addr()?);
        let issuer = "https://identity.example.test/realms/heterocloud".to_owned();
        let provider = TestProvider {
            issuer: issuer.clone(),
            endpoint_origin: endpoint_origin.clone(),
        };
        let app = Router::new()
            .route("/.well-known/openid-configuration", get(discovery))
            .route("/token", post(token))
            .route("/jwks", get(jwks))
            .with_state(provider);
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let config = OidcConfig::new(
            issuer.parse()?,
            Some(endpoint_origin.parse()?),
            TEST_CLIENT_ID.to_owned(),
            SecretString::from("test-client-secret-value"),
            "https://console.example.test/api/v1/auth/oidc/callback".parse()?,
            false,
        )?;
        let cookie_key = SecretString::from("test-cookie-key-with-at-least-32-bytes");
        let start = config
            .begin_login(&cookie_key, true, OidcLoginIntent::Authenticate)
            .await?;
        assert_eq!(
            start.authorization_url.origin(),
            url::Url::parse(&issuer)?.origin()
        );
        let parameters: HashMap<String, String> =
            start.authorization_url.query_pairs().into_owned().collect();
        let identity = config
            .complete_login(
                &OidcCallbackQuery {
                    code: Some(parameters.get("nonce").ok_or("missing nonce")?.clone()),
                    state: Some(parameters.get("state").ok_or("missing state")?.clone()),
                    error: None,
                    error_description: None,
                },
                Some(start.transaction_cookie.value()),
                &cookie_key,
            )
            .await?;
        assert_eq!(identity.issuer, issuer);

        server.abort();
        Ok(())
    }

    async fn discovery(State(provider): State<TestProvider>) -> Json<Value> {
        Json(json!({
            "issuer": provider.issuer,
            "authorization_endpoint": format!("{}/authorize", provider.issuer),
            "token_endpoint": format!("{}/token", provider.endpoint_origin),
            "jwks_uri": format!("{}/jwks", provider.endpoint_origin),
            "id_token_signing_alg_values_supported": ["RS256"]
        }))
    }

    async fn token(
        State(provider): State<TestProvider>,
        Form(form): Form<HashMap<String, String>>,
    ) -> Result<Json<Value>, StatusCode> {
        let code = form.get("code").ok_or(StatusCode::BAD_REQUEST)?;
        let (nonce, audience, invalidate_signature) =
            if let Some(nonce) = code.strip_prefix("wrong-audience:") {
                (nonce, "another-client", false)
            } else if let Some(nonce) = code.strip_prefix("invalid-signature:") {
                (nonce, TEST_CLIENT_ID, true)
            } else {
                (code.as_str(), TEST_CLIENT_ID, false)
            };
        let now = unix_timestamp().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(TEST_KEY_ID.to_owned());
        let mut id_token = encode(
            &header,
            &TestClaims {
                iss: &provider.issuer,
                sub: "keycloak-subject",
                aud: audience,
                exp: now + 300,
                iat: now,
                nonce,
                email: "oidc-user@example.test",
                name: "OIDC Test User",
            },
            &EncodingKey::from_rsa_pem(TEST_RSA_PRIVATE_KEY.as_bytes())
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if invalidate_signature {
            let replacement = if id_token.ends_with('a') { 'b' } else { 'a' };
            let _ = id_token.pop();
            id_token.push(replacement);
        }
        Ok(Json(json!({"id_token": id_token})))
    }

    async fn jwks() -> Json<Value> {
        Json(json!({
            "keys": [{
                "kty": "RSA",
                "use": "sig",
                "alg": "RS256",
                "kid": TEST_KEY_ID,
                "n": TEST_RSA_MODULUS,
                "e": "AQAB"
            }]
        }))
    }
}
