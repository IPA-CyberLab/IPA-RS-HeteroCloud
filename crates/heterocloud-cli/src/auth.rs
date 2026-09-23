use std::{
    collections::BTreeMap,
    env, fs,
    fs::OpenOptions,
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use clap::{Args, Subcommand};
use reqwest::{
    Client, Response, StatusCode, Url,
    header::{AUTHORIZATION, HeaderValue},
};
use serde::{Deserialize, Serialize};
use tokio::time::{Instant, sleep};
use uuid::Uuid;

use crate::CliError;

const USER_AGENT: &str = concat!("heterocloud-cli/", env!("CARGO_PKG_VERSION"));
const CREDENTIAL_FILE_VERSION: u8 = 1;
const API_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const API_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Args)]
pub struct AuthArgs {
    #[command(subcommand)]
    pub command: AuthCommand,
}

#[derive(Debug, Subcommand)]
pub enum AuthCommand {
    /// Sign in through the HeteroCloud browser and configured identity provider.
    Login {
        /// Print the verification URL without opening a browser.
        #[arg(long)]
        no_browser: bool,
    },
    /// Verify the saved login against the HeteroCloud API.
    Status,
    /// Revoke the saved CLI token and remove it from this computer.
    Logout,
}

pub(crate) struct AuthSettings {
    pub endpoint: Option<String>,
    pub organization_id: Option<Uuid>,
    pub allow_insecure_http: bool,
}

#[derive(Clone)]
pub(crate) struct StoredCredential {
    pub endpoint: String,
    pub access_token: String,
    pub organization_id: Uuid,
}

#[derive(Default, Deserialize, Serialize)]
struct CredentialFile {
    #[serde(default = "credential_file_version")]
    version: u8,
    active_endpoint: Option<String>,
    #[serde(default)]
    profiles: BTreeMap<String, CredentialProfile>,
}

#[derive(Clone, Deserialize, Serialize)]
struct CredentialProfile {
    access_token: String,
    organization_id: Uuid,
    user_email: String,
    user_display_name: String,
    organization_name: String,
    organization_slug: String,
    expires_at: String,
}

const fn credential_file_version() -> u8 {
    CREDENTIAL_FILE_VERSION
}

#[derive(Deserialize)]
struct DeviceAuthorization {
    device_code: String,
    user_code: String,
    verification_uri_complete: String,
    expires_in: u64,
    interval: u64,
}

#[derive(Deserialize)]
struct DeviceToken {
    access_token: String,
    token_type: String,
    user: AuthUser,
    organization: AuthOrganization,
}

#[derive(Deserialize)]
struct AuthSession {
    user: AuthUser,
    organization: AuthOrganization,
    expires_at: String,
}

#[derive(Deserialize)]
struct AuthUser {
    email: String,
    display_name: String,
}

#[derive(Deserialize)]
struct AuthOrganization {
    organization_id: Uuid,
    organization_slug: String,
    organization_name: String,
}

#[derive(Deserialize)]
struct DeviceTokenError {
    error: String,
    error_description: Option<String>,
}

#[derive(Deserialize)]
struct ApiErrorEnvelope {
    error: ApiErrorBody,
}

#[derive(Deserialize)]
struct ApiErrorBody {
    code: String,
    message: String,
}

pub(crate) async fn execute(args: AuthArgs, settings: AuthSettings) -> Result<(), CliError> {
    match args.command {
        AuthCommand::Login { no_browser } => login(settings, no_browser).await,
        AuthCommand::Status => status(settings).await,
        AuthCommand::Logout => logout(settings).await,
    }
}

async fn login(settings: AuthSettings, no_browser: bool) -> Result<(), CliError> {
    let endpoint = resolve_endpoint(settings.endpoint.as_deref(), settings.allow_insecure_http)?;
    let organization_id = settings
        .organization_id
        .ok_or(CliError::MissingOrganization)?;
    let client = auth_client()?;
    let authorization_url = endpoint
        .join("api/v1/auth/cli/device")
        .map_err(|error| CliError::InvalidApiEndpoint(error.to_string()))?;
    let response = client
        .post(authorization_url)
        .json(&serde_json::json!({ "organization_id": organization_id }))
        .send()
        .await
        .map_err(CliError::ApiTransport)?;
    let authorization: DeviceAuthorization = decode_json_response(response).await?;
    if !valid_device_code(&authorization.device_code)
        || authorization.user_code.is_empty()
        || !(1..=3600).contains(&authorization.expires_in)
        || !(1..=60).contains(&authorization.interval)
    {
        return Err(CliError::Authentication(
            "the server returned an invalid device authorization".into(),
        ));
    }
    let verification_url = Url::parse(&authorization.verification_uri_complete)
        .map_err(|_| CliError::Authentication("the server returned an invalid login URL".into()))?;
    if verification_url.scheme() != endpoint.scheme()
        || verification_url.host_str() != endpoint.host_str()
        || verification_url.port_or_known_default() != endpoint.port_or_known_default()
        || !verification_url.username().is_empty()
        || verification_url.password().is_some()
    {
        return Err(CliError::Authentication(
            "the server returned a login URL on a different origin".into(),
        ));
    }

    println!("Open this URL to sign in and approve the CLI:");
    println!("{}", verification_url.as_str());
    println!("Verification code: {}", authorization.user_code);
    if !no_browser && !open_browser(verification_url.as_str())? {
        eprintln!("Could not open a browser automatically. Open the URL above manually.");
    }

    let token_url = endpoint
        .join("api/v1/auth/cli/token")
        .map_err(|error| CliError::InvalidApiEndpoint(error.to_string()))?;
    let deadline = Instant::now() + Duration::from_secs(authorization.expires_in);
    let mut interval = Duration::from_secs(authorization.interval.clamp(1, 60));
    let token = loop {
        if Instant::now() >= deadline {
            return Err(CliError::Authentication(
                "browser authorization expired; run `heterocloud auth login` again".into(),
            ));
        }
        let response = client
            .post(token_url.clone())
            .json(&serde_json::json!({ "device_code": authorization.device_code }))
            .send()
            .await
            .map_err(CliError::ApiTransport)?;
        if response.status().is_success() {
            break response
                .json::<DeviceToken>()
                .await
                .map_err(CliError::ApiTransport)?;
        }
        if response.status() != StatusCode::BAD_REQUEST {
            return Err(decode_api_error(response).await);
        }
        let status = response.status();
        let bytes = response.bytes().await.map_err(CliError::ApiTransport)?;
        let error = serde_json::from_slice::<DeviceTokenError>(&bytes)
            .map_err(|_| unexpected_response(status, &bytes))?;
        match error.error.as_str() {
            "authorization_pending" => {}
            "slow_down" => {
                interval = (interval + Duration::from_secs(5)).min(Duration::from_secs(60))
            }
            "access_denied" | "expired_token" | "invalid_grant" => {
                return Err(CliError::Authentication(
                    error.error_description.unwrap_or(error.error),
                ));
            }
            _ => {
                return Err(CliError::Authentication(
                    error.error_description.unwrap_or(error.error),
                ));
            }
        }
        sleep(interval).await;
    };
    if token.token_type != "Bearer"
        || !valid_access_token(&token.access_token)
        || token.organization.organization_id != organization_id
    {
        return Err(CliError::Authentication(
            "the server returned an invalid CLI access token".into(),
        ));
    }
    let verified = fetch_session(&client, &endpoint, &token.access_token).await?;
    if verified.organization.organization_id != organization_id
        || !verified.user.email.eq_ignore_ascii_case(&token.user.email)
    {
        return Err(CliError::Authentication(
            "the authenticated identity did not match the authorization response".into(),
        ));
    }
    let path = credential_file_path()?;
    let mut credentials = read_credential_file(&path)?;
    credentials.version = CREDENTIAL_FILE_VERSION;
    credentials.active_endpoint = Some(endpoint.to_string());
    credentials.profiles.insert(
        endpoint.to_string(),
        CredentialProfile {
            access_token: token.access_token,
            organization_id,
            user_email: verified.user.email.clone(),
            user_display_name: verified.user.display_name.clone(),
            organization_name: verified.organization.organization_name.clone(),
            organization_slug: verified.organization.organization_slug.clone(),
            expires_at: verified.expires_at.clone(),
        },
    );
    write_credential_file(&path, &credentials)?;
    println!(
        "Signed in as {} ({}) for {} ({}).",
        verified.user.display_name,
        verified.user.email,
        verified.organization.organization_name,
        verified.organization.organization_id
    );
    println!("Token expires at {}.", verified.expires_at);
    println!("Credentials saved to {}.", path.display());
    Ok(())
}

async fn status(settings: AuthSettings) -> Result<(), CliError> {
    let credential =
        load_stored_credential(settings.endpoint.as_deref(), settings.allow_insecure_http)?;
    let endpoint = normalize_endpoint(&credential.endpoint, settings.allow_insecure_http)?;
    let client = auth_client()?;
    let session = fetch_session(&client, &endpoint, &credential.access_token).await?;
    if session.organization.organization_id != credential.organization_id {
        return Err(CliError::Authentication(
            "the saved organization does not match the authenticated token".into(),
        ));
    }
    println!(
        "Signed in: {} <{}>",
        session.user.display_name, session.user.email
    );
    println!(
        "Organization: {} ({})",
        session.organization.organization_name, session.organization.organization_id
    );
    println!("Endpoint: {}", endpoint);
    println!("Token expires at: {}", session.expires_at);
    Ok(())
}

async fn logout(settings: AuthSettings) -> Result<(), CliError> {
    let credential =
        load_stored_credential(settings.endpoint.as_deref(), settings.allow_insecure_http)?;
    let endpoint = normalize_endpoint(&credential.endpoint, settings.allow_insecure_http)?;
    let client = auth_client()?;
    let logout_url = endpoint
        .join("api/v1/auth/cli/logout")
        .map_err(|error| CliError::InvalidApiEndpoint(error.to_string()))?;
    let response = client
        .post(logout_url)
        .header(AUTHORIZATION, bearer_header(&credential.access_token)?)
        .send()
        .await
        .map_err(CliError::ApiTransport)?;
    if !response.status().is_success() && response.status() != StatusCode::UNAUTHORIZED {
        return Err(decode_api_error(response).await);
    }
    remove_stored_credential(endpoint.as_str())?;
    println!(
        "Signed out from {} and removed the local CLI credential.",
        endpoint
    );
    Ok(())
}

async fn fetch_session(
    client: &Client,
    endpoint: &Url,
    access_token: &str,
) -> Result<AuthSession, CliError> {
    let url = endpoint
        .join("api/v1/auth/cli/session")
        .map_err(|error| CliError::InvalidApiEndpoint(error.to_string()))?;
    let response = client
        .get(url)
        .header(AUTHORIZATION, bearer_header(access_token)?)
        .send()
        .await
        .map_err(CliError::ApiTransport)?;
    decode_json_response(response).await
}

fn auth_client() -> Result<Client, CliError> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(API_CONNECT_TIMEOUT)
        .timeout(API_REQUEST_TIMEOUT)
        .build()
        .map_err(CliError::ApiTransport)
}

fn bearer_header(access_token: &str) -> Result<HeaderValue, CliError> {
    let mut value = HeaderValue::from_str(&format!("Bearer {access_token}"))
        .map_err(|_| CliError::Authentication("invalid saved CLI access token".into()))?;
    value.set_sensitive(true);
    Ok(value)
}

pub(crate) fn load_stored_credential(
    requested_endpoint: Option<&str>,
    allow_insecure_http: bool,
) -> Result<StoredCredential, CliError> {
    let path = credential_file_path()?;
    let credentials = read_credential_file(&path)?;
    let endpoint = match requested_endpoint {
        Some(endpoint) => normalize_endpoint(endpoint, allow_insecure_http)?.to_string(),
        None => credentials
            .active_endpoint
            .clone()
            .ok_or(CliError::MissingEndpoint)?,
    };
    let profile = credentials
        .profiles
        .get(&endpoint)
        .ok_or_else(|| CliError::LoginRequired(endpoint.clone()))?;
    if !valid_access_token(&profile.access_token) {
        return Err(CliError::InvalidStoredCredential(path));
    }
    Ok(StoredCredential {
        endpoint,
        access_token: profile.access_token.clone(),
        organization_id: profile.organization_id,
    })
}

pub(crate) fn resolve_endpoint(
    requested_endpoint: Option<&str>,
    allow_insecure_http: bool,
) -> Result<Url, CliError> {
    if let Some(endpoint) = requested_endpoint {
        return normalize_endpoint(endpoint, allow_insecure_http);
    }
    let path = credential_file_path()?;
    let credentials = read_credential_file(&path)?;
    let endpoint = credentials
        .active_endpoint
        .ok_or(CliError::MissingEndpoint)?;
    normalize_endpoint(&endpoint, allow_insecure_http)
}

pub(crate) fn normalize_endpoint(
    endpoint: &str,
    allow_insecure_http: bool,
) -> Result<Url, CliError> {
    let mut endpoint =
        Url::parse(endpoint).map_err(|error| CliError::InvalidApiEndpoint(error.to_string()))?;
    if endpoint.host_str().is_none()
        || endpoint.cannot_be_a_base()
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
    {
        return Err(CliError::InvalidApiEndpoint(
            "use an absolute origin without credentials, query, or fragment".into(),
        ));
    }
    match endpoint.scheme() {
        "https" => {}
        "http" if allow_insecure_http => {}
        "http" => {
            return Err(CliError::InvalidApiEndpoint(
                "plain HTTP requires --allow-insecure-http".into(),
            ));
        }
        scheme => {
            return Err(CliError::InvalidApiEndpoint(format!(
                "unsupported URL scheme {scheme}"
            )));
        }
    }
    endpoint.set_path("/");
    Ok(endpoint)
}

fn credential_file_path() -> Result<PathBuf, CliError> {
    if let Some(path) = env::var_os("HETEROCLOUD_CREDENTIALS_FILE") {
        if path.is_empty() {
            return Err(CliError::CredentialStore(
                "HETEROCLOUD_CREDENTIALS_FILE is empty".into(),
            ));
        }
        return Ok(PathBuf::from(path));
    }
    #[cfg(target_os = "windows")]
    let base = env::var_os("APPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| CliError::CredentialStore("APPDATA is not set".into()))?;
    #[cfg(target_os = "macos")]
    let base = env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join("Library").join("Application Support"))
        .ok_or_else(|| CliError::CredentialStore("HOME is not set".into()))?;
    #[cfg(all(unix, not(target_os = "macos")))]
    let base = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or_else(|| CliError::CredentialStore("HOME and XDG_CONFIG_HOME are not set".into()))?;
    Ok(base.join("heterocloud").join("credentials.json"))
}

fn read_credential_file(path: &Path) -> Result<CredentialFile, CliError> {
    if !path.exists() {
        return Ok(CredentialFile {
            version: CREDENTIAL_FILE_VERSION,
            ..CredentialFile::default()
        });
    }
    let metadata = fs::symlink_metadata(path).map_err(|source| CliError::CredentialStoreIo {
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(CliError::UnsafeCredentialStore(path.to_path_buf()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode() & 0o777;
        if mode & 0o077 != 0 || mode & 0o400 == 0 {
            return Err(CliError::UnsafeCredentialStoreMode {
                path: path.to_path_buf(),
                mode,
            });
        }
    }
    let bytes = fs::read(path).map_err(|source| CliError::CredentialStoreIo {
        path: path.to_path_buf(),
        source,
    })?;
    let credentials: CredentialFile = serde_json::from_slice(&bytes)
        .map_err(|_| CliError::InvalidStoredCredential(path.to_path_buf()))?;
    if credentials.version != CREDENTIAL_FILE_VERSION {
        return Err(CliError::InvalidStoredCredential(path.to_path_buf()));
    }
    Ok(credentials)
}

fn write_credential_file(path: &Path, credentials: &CredentialFile) -> Result<(), CliError> {
    let parent = path.parent().ok_or_else(|| {
        CliError::CredentialStore("credential file must have a parent directory".into())
    })?;
    fs::create_dir_all(parent).map_err(|source| CliError::CredentialStoreIo {
        path: parent.to_path_buf(),
        source,
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).map_err(|source| {
            CliError::CredentialStoreIo {
                path: parent.to_path_buf(),
                source,
            }
        })?;
    }
    if path.exists() {
        let metadata =
            fs::symlink_metadata(path).map_err(|source| CliError::CredentialStoreIo {
                path: path.to_path_buf(),
                source,
            })?;
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            return Err(CliError::UnsafeCredentialStore(path.to_path_buf()));
        }
    }
    let temporary = parent.join(format!(".credentials-{}.tmp", Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|source| CliError::CredentialStoreIo {
            path: temporary.clone(),
            source,
        })?;
    let result = (|| -> Result<(), CliError> {
        serde_json::to_writer_pretty(&mut file, credentials)?;
        file.write_all(b"\n")
            .and_then(|()| file.sync_all())
            .map_err(|source| CliError::CredentialStoreIo {
                path: temporary.clone(),
                source,
            })?;
        replace_file(&temporary, path)?;
        restrict_windows_acl(path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn replace_file(temporary: &Path, target: &Path) -> Result<(), CliError> {
    #[cfg(target_os = "windows")]
    if target.exists() {
        fs::remove_file(target).map_err(|source| CliError::CredentialStoreIo {
            path: target.to_path_buf(),
            source,
        })?;
    }
    fs::rename(temporary, target).map_err(|source| CliError::CredentialStoreIo {
        path: target.to_path_buf(),
        source,
    })
}

#[cfg(target_os = "windows")]
fn restrict_windows_acl(path: &Path) -> Result<(), CliError> {
    let username = env::var("USERNAME")
        .map_err(|_| CliError::CredentialStore("USERNAME is not set".into()))?;
    let account = env::var("USERDOMAIN")
        .map(|domain| format!("{domain}\\{username}"))
        .unwrap_or(username);
    let output = Command::new("icacls")
        .arg(path)
        .arg("/inheritance:r")
        .arg("/grant:r")
        .arg(format!("{account}:(F)"))
        .output()
        .map_err(|source| CliError::CredentialStoreIo {
            path: path.to_path_buf(),
            source,
        })?;
    if output.status.success() {
        Ok(())
    } else {
        let _ = fs::remove_file(path);
        Err(CliError::CredentialStore(
            "Windows could not restrict the credential file ACL".into(),
        ))
    }
}

#[cfg(not(target_os = "windows"))]
fn restrict_windows_acl(_path: &Path) -> Result<(), CliError> {
    Ok(())
}

fn remove_stored_credential(endpoint: &str) -> Result<(), CliError> {
    let path = credential_file_path()?;
    let mut credentials = read_credential_file(&path)?;
    credentials.profiles.remove(endpoint);
    if credentials.active_endpoint.as_deref() == Some(endpoint) {
        credentials.active_endpoint = credentials.profiles.keys().next().cloned();
    }
    if credentials.profiles.is_empty() {
        if path.exists() {
            fs::remove_file(&path).map_err(|source| CliError::CredentialStoreIo {
                path: path.clone(),
                source,
            })?;
        }
        return Ok(());
    }
    write_credential_file(&path, &credentials)
}

fn valid_device_code(value: &str) -> bool {
    value.strip_prefix("hcd_").is_some_and(|secret| {
        secret.len() >= 32
            && secret.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
            })
    })
}

fn valid_access_token(value: &str) -> bool {
    let Some((prefix, secret)) = value
        .strip_prefix("hcu_")
        .and_then(|value| Some((value.get(..16)?, value.get(16..)?.strip_prefix('_')?)))
    else {
        return false;
    };
    prefix.len() == 16
        && secret.len() >= 32
        && prefix
            .chars()
            .chain(secret.chars())
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
        && !value.chars().any(char::is_whitespace)
        && !value.chars().any(char::is_control)
}

#[cfg(target_os = "windows")]
fn open_browser(url: &str) -> Result<bool, CliError> {
    Command::new("rundll32")
        .arg("url.dll,FileProtocolHandler")
        .arg(url)
        .spawn()
        .map(|_| true)
        .or_else(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                Ok(false)
            } else {
                Err(CliError::Browser(error))
            }
        })
}

#[cfg(target_os = "macos")]
fn open_browser(url: &str) -> Result<bool, CliError> {
    open_browser_command("open", url)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_browser(url: &str) -> Result<bool, CliError> {
    open_browser_command("xdg-open", url)
}

#[cfg(not(any(unix, target_os = "windows")))]
fn open_browser(_url: &str) -> Result<bool, CliError> {
    Ok(false)
}

#[cfg(unix)]
fn open_browser_command(program: &'static str, url: &str) -> Result<bool, CliError> {
    Command::new(program)
        .arg(url)
        .spawn()
        .map(|_| true)
        .or_else(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                Ok(false)
            } else {
                Err(CliError::Browser(error))
            }
        })
}

async fn decode_json_response<T: for<'de> Deserialize<'de>>(
    response: Response,
) -> Result<T, CliError> {
    if !response.status().is_success() {
        return Err(decode_api_error(response).await);
    }
    response.json::<T>().await.map_err(CliError::ApiTransport)
}

async fn decode_api_error(response: Response) -> CliError {
    let status = response.status();
    let bytes = match response.bytes().await {
        Ok(bytes) => bytes,
        Err(error) => return CliError::ApiTransport(error),
    };
    if let Ok(envelope) = serde_json::from_slice::<ApiErrorEnvelope>(&bytes) {
        return CliError::ApiResponse {
            status: status.as_u16(),
            code: envelope.error.code,
            message: envelope.error.message,
        };
    }
    unexpected_response(status, &bytes)
}

fn unexpected_response(status: StatusCode, bytes: &[u8]) -> CliError {
    let message = String::from_utf8_lossy(bytes)
        .trim()
        .chars()
        .take(512)
        .collect::<String>();
    CliError::ApiResponse {
        status: status.as_u16(),
        code: "unexpected_response".into(),
        message: if message.is_empty() {
            status.canonical_reason().unwrap_or("request failed").into()
        } else {
            message
        },
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{
        CredentialFile, CredentialProfile, read_credential_file, valid_access_token,
        write_credential_file,
    };
    use uuid::Uuid;

    #[test]
    fn credential_file_is_private_and_round_trips() -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!("heterocloud-cli-test-{}", Uuid::new_v4()));
        let path = root.join("credentials.json");
        let organization_id = Uuid::new_v4();
        let mut credentials = CredentialFile {
            version: 1,
            active_endpoint: Some("https://cloud.example.test/".into()),
            ..CredentialFile::default()
        };
        credentials.profiles.insert(
            "https://cloud.example.test/".into(),
            CredentialProfile {
                access_token: format!("hcu_1234567890abcdef_{}", "x".repeat(43)),
                organization_id,
                user_email: "user@example.test".into(),
                user_display_name: "Example User".into(),
                organization_name: "Example".into(),
                organization_slug: "example".into(),
                expires_at: "2026-10-23T00:00:00Z".into(),
            },
        );
        write_credential_file(&path, &credentials)?;
        let restored = read_credential_file(&path)?;
        assert_eq!(restored.active_endpoint, credentials.active_endpoint);
        assert_eq!(
            restored
                .profiles
                .get("https://cloud.example.test/")
                .map(|profile| profile.organization_id),
            Some(organization_id)
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o600);
        }
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn access_token_prefix_can_contain_url_safe_separators() {
        assert!(valid_access_token(&format!(
            "hcu_0123456789abc_de_{}",
            "x".repeat(43)
        )));
        assert!(!valid_access_token("hcu_short_secret"));
    }
}
