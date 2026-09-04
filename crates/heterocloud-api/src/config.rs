use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};

use clap::Parser;
use ipnet::IpNet;
use secrecy::{ExposeSecret, SecretString};
use thiserror::Error;
use tokio::fs;
use url::Url;

use crate::{
    flow_access::{FlowAccessError, FlowAccessSigner},
    oidc::{OidcConfig, OidcConfigError},
    syouyu_provider::{SyouyuPrincipalSigner, SyouyuProviderError},
};

#[derive(Clone, Debug, Parser)]
#[command(version, about = "HeteroCloud control-plane API")]
pub struct Config {
    #[arg(long, env = "HETEROCLOUD_LISTEN", default_value = "0.0.0.0:8080")]
    pub listen: SocketAddr,

    #[arg(long, env = "HETEROCLOUD_DATABASE_URL_FILE")]
    pub database_url_file: PathBuf,

    #[arg(long, env = "HETEROCLOUD_CSRF_KEY_FILE")]
    pub csrf_key_file: PathBuf,

    #[arg(long, env = "HETEROCLOUD_FLOW_ACCESS_SECRET_FILE")]
    pub flow_access_secret_file: PathBuf,

    #[arg(
        long,
        env = "HETEROCLOUD_FLOW_ACCESS_ISSUER",
        default_value = "heterocloud"
    )]
    pub flow_access_issuer: String,

    #[arg(
        long,
        env = "HETEROCLOUD_FLOW_ACCESS_AUDIENCE",
        default_value = "heterocloud-flow-data"
    )]
    pub flow_access_audience: String,

    #[arg(
        long,
        env = "HETEROCLOUD_FLOW_PUBLIC_ENDPOINTS",
        value_delimiter = ',',
        required = true
    )]
    pub flow_public_endpoints: Vec<Url>,

    #[arg(
        long,
        env = "HETEROCLOUD_FLOW_INTERNAL_ENDPOINT",
        default_value = "http://heterocloud-flow-api.heterocloud-flow.svc.cluster.local:8080/"
    )]
    pub flow_internal_endpoint: Url,

    #[arg(
        long,
        env = "HETEROCLOUD_FLASH_INTERNAL_ENDPOINT",
        default_value = "http://heterocloud-flash-api.heterocloud-flash.svc.cluster.local:8080/"
    )]
    pub flash_internal_endpoint: Url,

    #[arg(
        long,
        env = "HETEROCLOUD_SYOUYU_INTERNAL_ENDPOINT",
        default_value = "http://heterocloud-syouyu-api.heterocloud-syouyu.svc.cluster.local:8080/"
    )]
    pub syouyu_internal_endpoint: Url,

    #[arg(long, env = "HETEROCLOUD_SYOUYU_ACCESS_SECRET_FILE")]
    pub syouyu_access_secret_file: PathBuf,

    #[arg(
        long,
        env = "HETEROCLOUD_SYOUYU_ACCESS_ISSUER",
        default_value = "heterocloud"
    )]
    pub syouyu_access_issuer: String,

    #[arg(
        long,
        env = "HETEROCLOUD_SYOUYU_ACCESS_AUDIENCE",
        default_value = "heterocloud-syouyu-data"
    )]
    pub syouyu_access_audience: String,

    #[arg(long, env = "HETEROCLOUD_PROVIDER_SIGNING_KEY_FILE")]
    pub provider_signing_key_file: PathBuf,

    #[arg(
        long,
        env = "HETEROCLOUD_PROVIDER_ISSUER",
        default_value = "heterocloud"
    )]
    pub provider_issuer: String,

    #[arg(
        long,
        env = "HETEROCLOUD_FLASH_AUDIENCE",
        default_value = "heterocloud-flash"
    )]
    pub flash_audience: String,

    #[arg(
        long,
        env = "HETEROCLOUD_PROVIDER_KEY_ID",
        default_value = "heterocloud-provider-1"
    )]
    pub provider_key_id: String,

    #[arg(
        long,
        env = "HETEROCLOUD_PUBLIC_ORIGIN",
        default_value = "http://localhost:8080"
    )]
    pub public_origin: Url,

    #[arg(long, env = "HETEROCLOUD_ADDITIONAL_ORIGINS", value_delimiter = ',')]
    pub additional_origins: Vec<Url>,

    #[arg(
        long,
        env = "HETEROCLOUD_TRUSTED_PROXY_NETWORKS",
        value_delimiter = ','
    )]
    pub trusted_proxy_networks: Vec<IpNet>,

    #[arg(
        long,
        env = "HETEROCLOUD_SECURE_COOKIE",
        default_value_t = true,
        action = clap::ArgAction::Set
    )]
    pub secure_cookie: bool,

    #[arg(
        long,
        env = "HETEROCLOUD_SESSION_TTL_SECONDS",
        default_value_t = 43_200
    )]
    pub session_ttl_seconds: u64,

    #[arg(
        long,
        env = "HETEROCLOUD_DATABASE_MAX_CONNECTIONS",
        default_value_t = 30
    )]
    pub database_max_connections: u32,

    #[arg(long, env = "HETEROCLOUD_CONSOLE_DIR")]
    pub console_dir: Option<PathBuf>,

    #[arg(long, env = "HETEROCLOUD_TLS_CERT_FILE")]
    pub tls_cert_file: Option<PathBuf>,

    #[arg(long, env = "HETEROCLOUD_TLS_KEY_FILE")]
    pub tls_key_file: Option<PathBuf>,

    #[arg(long, env = "HETEROCLOUD_OIDC_ISSUER_URL")]
    pub oidc_issuer_url: Option<Url>,

    #[arg(long, env = "HETEROCLOUD_OIDC_BACKCHANNEL_ISSUER_URL")]
    pub oidc_backchannel_issuer_url: Option<Url>,

    #[arg(long, env = "HETEROCLOUD_OIDC_CLIENT_ID")]
    pub oidc_client_id: Option<String>,

    #[arg(long, env = "HETEROCLOUD_OIDC_CLIENT_SECRET_FILE")]
    pub oidc_client_secret_file: Option<PathBuf>,

    #[arg(long, env = "HETEROCLOUD_OIDC_PUBLIC_CALLBACK_URL")]
    pub oidc_public_callback_url: Option<Url>,

    #[arg(long, env = "HETEROCLOUD_OWNER_ORIGIN")]
    pub owner_origin: Option<Url>,

    #[arg(long, env = "HETEROCLOUD_OWNER_EMAIL")]
    pub owner_email: Option<String>,

    #[arg(
        long,
        env = "HETEROCLOUD_OWNER_CONSOLE_MODE",
        default_value_t = false,
        action = clap::ArgAction::Set
    )]
    pub owner_console_mode: bool,

    #[arg(
        long,
        env = "HETEROCLOUD_OWNER_ALLOWED_NETWORKS",
        value_delimiter = ',',
        default_value = "10.250.0.0/24"
    )]
    pub owner_allowed_networks: Vec<IpNet>,

    #[arg(long, env = "HETEROCLOUD_REGISTRY_INTERNAL_ENDPOINT")]
    pub registry_internal_endpoint: Option<Url>,

    #[arg(long, env = "HETEROCLOUD_REGISTRY_PUBLIC_ENDPOINT")]
    pub registry_public_endpoint: Option<Url>,

    #[arg(
        long,
        env = "HETEROCLOUD_REGISTRY_ADMIN_USERNAME",
        default_value = "admin"
    )]
    pub registry_admin_username: String,

    #[arg(long, env = "HETEROCLOUD_REGISTRY_ADMIN_PASSWORD_FILE")]
    pub registry_admin_password_file: Option<PathBuf>,

    #[arg(long, env = "HETEROCLOUD_BOOTSTRAP_EMAIL")]
    pub bootstrap_email: Option<String>,

    #[arg(long, env = "HETEROCLOUD_BOOTSTRAP_PASSWORD_FILE")]
    pub bootstrap_password_file: Option<PathBuf>,

    #[arg(
        long,
        env = "HETEROCLOUD_BOOTSTRAP_DISPLAY_NAME",
        default_value = "HeteroCloud Administrator"
    )]
    pub bootstrap_display_name: String,

    #[arg(
        long,
        env = "HETEROCLOUD_BOOTSTRAP_ORGANIZATION_SLUG",
        default_value = "heterocloud"
    )]
    pub bootstrap_organization_slug: String,

    #[arg(
        long,
        env = "HETEROCLOUD_BOOTSTRAP_ORGANIZATION_NAME",
        default_value = "HeteroCloud"
    )]
    pub bootstrap_organization_name: String,
}

#[derive(Clone)]
pub struct RuntimeConfig {
    pub public_origin: Url,
    pub allowed_origins: Vec<String>,
    pub trusted_proxy_networks: Vec<IpNet>,
    pub secure_cookie: bool,
    pub session_ttl: Duration,
    pub csrf_key: SecretString,
    pub flow_access_signer: FlowAccessSigner,
    pub flow_public_endpoints: Vec<Url>,
    pub flow_internal_endpoint: Url,
    pub oidc: Option<OidcConfig>,
    pub owner_origin: Option<Url>,
    pub owner_email: Option<String>,
    pub owner_console_mode: bool,
    pub owner_allowed_networks: Vec<IpNet>,
}

impl Config {
    pub async fn load_secrets(&self) -> Result<LoadedSecrets, ConfigError> {
        let database_url = read_secret(&self.database_url_file).await?;
        let csrf_key = read_secret(&self.csrf_key_file).await?;
        if csrf_key.expose_secret().len() < 32 {
            return Err(ConfigError::WeakCsrfKey);
        }
        let flow_access_secret = read_secret(&self.flow_access_secret_file).await?;
        let syouyu_access_secret = read_secret(&self.syouyu_access_secret_file).await?;
        let provider_signing_key = read_secret(&self.provider_signing_key_file).await?;
        FlowAccessSigner::new(
            self.flow_access_issuer.clone(),
            self.flow_access_audience.clone(),
            flow_access_secret.clone(),
        )?;
        SyouyuPrincipalSigner::new(
            self.syouyu_access_issuer.clone(),
            self.syouyu_access_audience.clone(),
            syouyu_access_secret.clone(),
            Duration::from_secs(60),
        )?;
        let bootstrap_password = match &self.bootstrap_password_file {
            Some(path) => Some(read_secret(path).await?),
            None => None,
        };
        let oidc_client_secret = match (
            &self.oidc_issuer_url,
            &self.oidc_backchannel_issuer_url,
            &self.oidc_client_id,
            &self.oidc_client_secret_file,
            &self.oidc_public_callback_url,
        ) {
            (None, None, None, None, None) => None,
            (Some(_), _, Some(_), Some(path), Some(_)) => Some(read_secret(path).await?),
            _ => return Err(ConfigError::IncompleteOidc),
        };
        let registry_admin_password = match (
            &self.registry_internal_endpoint,
            &self.registry_public_endpoint,
            &self.registry_admin_password_file,
        ) {
            (None, None, None) => None,
            (Some(_), Some(_), Some(path)) => Some(read_secret(path).await?),
            _ => return Err(ConfigError::IncompleteRegistry),
        };
        if self.bootstrap_email.is_some() != bootstrap_password.is_some() {
            return Err(ConfigError::IncompleteBootstrap);
        }
        if self.tls_cert_file.is_some() != self.tls_key_file.is_some() {
            return Err(ConfigError::IncompleteTls);
        }
        validate_owner_config(self.owner_origin.as_ref(), self.owner_email.as_deref())?;
        if self.secure_cookie
            && (self.public_origin.scheme() != "https"
                || self
                    .additional_origins
                    .iter()
                    .any(|origin| origin.scheme() != "https"))
        {
            return Err(ConfigError::SecureCookieRequiresHttps);
        }
        validate_flow_public_endpoints(&self.flow_public_endpoints, self.secure_cookie)?;
        validate_flow_internal_endpoint(&self.flow_internal_endpoint)?;
        validate_flash_internal_endpoint(&self.flash_internal_endpoint)?;
        validate_syouyu_internal_endpoint(&self.syouyu_internal_endpoint)?;
        validate_registry_config(
            self.registry_internal_endpoint.as_ref(),
            self.registry_public_endpoint.as_ref(),
            &self.registry_admin_username,
        )?;
        Ok(LoadedSecrets {
            database_url,
            csrf_key,
            flow_access_secret,
            syouyu_access_secret,
            provider_signing_key,
            bootstrap_password,
            oidc_client_secret,
            registry_admin_password,
        })
    }

    pub fn runtime(
        &self,
        csrf_key: SecretString,
        flow_access_secret: SecretString,
        oidc_client_secret: Option<SecretString>,
    ) -> Result<RuntimeConfig, ConfigError> {
        validate_owner_config(self.owner_origin.as_ref(), self.owner_email.as_deref())?;
        validate_flow_public_endpoints(&self.flow_public_endpoints, self.secure_cookie)?;
        validate_flow_internal_endpoint(&self.flow_internal_endpoint)?;
        validate_flash_internal_endpoint(&self.flash_internal_endpoint)?;
        validate_syouyu_internal_endpoint(&self.syouyu_internal_endpoint)?;
        let mut allowed_origins = vec![self.public_origin.origin().ascii_serialization()];
        for origin in &self.additional_origins {
            let serialized = origin.origin().ascii_serialization();
            if !allowed_origins.contains(&serialized) {
                allowed_origins.push(serialized);
            }
        }
        let mut flow_public_endpoints = Vec::with_capacity(self.flow_public_endpoints.len());
        for endpoint in &self.flow_public_endpoints {
            if !flow_public_endpoints.contains(endpoint) {
                flow_public_endpoints.push(endpoint.clone());
            }
        }
        let oidc = match (
            &self.oidc_issuer_url,
            &self.oidc_backchannel_issuer_url,
            &self.oidc_client_id,
            oidc_client_secret,
            &self.oidc_public_callback_url,
        ) {
            (None, None, None, None, None) => None,
            (
                Some(issuer),
                backchannel_issuer,
                Some(client_id),
                Some(client_secret),
                Some(callback),
            ) => Some(OidcConfig::new(
                issuer.clone(),
                backchannel_issuer.clone(),
                client_id.clone(),
                client_secret,
                callback.clone(),
                !self.secure_cookie,
            )?),
            _ => return Err(ConfigError::IncompleteOidc),
        };
        Ok(RuntimeConfig {
            public_origin: self.public_origin.clone(),
            allowed_origins,
            trusted_proxy_networks: self.trusted_proxy_networks.clone(),
            secure_cookie: self.secure_cookie,
            session_ttl: Duration::from_secs(self.session_ttl_seconds.clamp(300, 86_400)),
            csrf_key,
            flow_access_signer: FlowAccessSigner::new(
                self.flow_access_issuer.clone(),
                self.flow_access_audience.clone(),
                flow_access_secret,
            )?,
            flow_public_endpoints,
            flow_internal_endpoint: self.flow_internal_endpoint.clone(),
            oidc,
            owner_origin: self.owner_origin.clone(),
            owner_email: self.owner_email.as_ref().map(|email| email.to_lowercase()),
            owner_console_mode: self.owner_console_mode,
            owner_allowed_networks: self.owner_allowed_networks.clone(),
        })
    }
}

pub struct LoadedSecrets {
    pub database_url: SecretString,
    pub csrf_key: SecretString,
    pub flow_access_secret: SecretString,
    pub syouyu_access_secret: SecretString,
    pub provider_signing_key: SecretString,
    pub bootstrap_password: Option<SecretString>,
    pub oidc_client_secret: Option<SecretString>,
    pub registry_admin_password: Option<SecretString>,
}

async fn read_secret(path: &Path) -> Result<SecretString, ConfigError> {
    let metadata = fs::metadata(path)
        .await
        .map_err(|source| ConfigError::ReadSecret {
            path: path.to_path_buf(),
            source,
        })?;
    if !metadata.file_type().is_file() {
        return Err(ConfigError::NotRegularFile(path.to_path_buf()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.mode() & 0o037 != 0 {
            return Err(ConfigError::UnsafePermissions(path.to_path_buf()));
        }
    }
    let value = fs::read_to_string(path)
        .await
        .map_err(|source| ConfigError::ReadSecret {
            path: path.to_path_buf(),
            source,
        })?;
    let value = value.trim_end_matches(['\r', '\n']);
    if value.is_empty() {
        return Err(ConfigError::EmptySecret(path.to_path_buf()));
    }
    Ok(SecretString::from(value.to_owned()))
}

fn validate_flow_public_endpoints(endpoints: &[Url], secure_mode: bool) -> Result<(), ConfigError> {
    if endpoints.is_empty() || endpoints.len() > 16 {
        return Err(ConfigError::MissingFlowPublicEndpoints);
    }
    let mut normalized = Vec::with_capacity(endpoints.len());
    for endpoint in endpoints {
        if !matches!(endpoint.scheme(), "http" | "https")
            || !endpoint.has_host()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || !matches!(endpoint.path(), "" | "/")
        {
            return Err(ConfigError::InvalidFlowPublicEndpoint);
        }
        let origin = endpoint.origin().ascii_serialization();
        if normalized.contains(&origin) {
            return Err(ConfigError::InvalidFlowPublicEndpoint);
        }
        normalized.push(origin);
    }
    if secure_mode
        && endpoints
            .iter()
            .any(|endpoint| endpoint.scheme() != "https")
    {
        return Err(ConfigError::SecureModeRequiresHttpsFlowEndpoints);
    }
    Ok(())
}

fn validate_flow_internal_endpoint(endpoint: &Url) -> Result<(), ConfigError> {
    if !matches!(endpoint.scheme(), "http" | "https")
        || !endpoint.has_host()
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
        || !matches!(endpoint.path(), "" | "/")
    {
        return Err(ConfigError::InvalidFlowInternalEndpoint);
    }
    Ok(())
}

fn validate_flash_internal_endpoint(endpoint: &Url) -> Result<(), ConfigError> {
    if !matches!(endpoint.scheme(), "http" | "https")
        || !endpoint.has_host()
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
        || !matches!(endpoint.path(), "" | "/")
    {
        return Err(ConfigError::InvalidFlashInternalEndpoint);
    }
    Ok(())
}

fn validate_syouyu_internal_endpoint(endpoint: &Url) -> Result<(), ConfigError> {
    if !matches!(endpoint.scheme(), "http" | "https")
        || !endpoint.has_host()
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
        || !matches!(endpoint.path(), "" | "/")
    {
        return Err(ConfigError::InvalidSyouyuInternalEndpoint);
    }
    Ok(())
}

fn validate_owner_config(origin: Option<&Url>, email: Option<&str>) -> Result<(), ConfigError> {
    if origin.is_some() != email.is_some() {
        return Err(ConfigError::IncompleteOwner);
    }
    if let Some(origin) = origin
        && (!matches!(origin.scheme(), "http" | "https")
            || !origin.has_host()
            || !origin.username().is_empty()
            || origin.password().is_some()
            || origin.query().is_some()
            || origin.fragment().is_some()
            || !matches!(origin.path(), "" | "/"))
    {
        return Err(ConfigError::InvalidOwnerOrigin);
    }
    if email.is_some_and(|email| !email_address::EmailAddress::is_valid(email)) {
        return Err(ConfigError::InvalidOwnerEmail);
    }
    Ok(())
}

fn validate_registry_config(
    internal_endpoint: Option<&Url>,
    public_endpoint: Option<&Url>,
    username: &str,
) -> Result<(), ConfigError> {
    if internal_endpoint.is_some() != public_endpoint.is_some() {
        return Err(ConfigError::IncompleteRegistry);
    }
    if username.trim().is_empty() || username.len() > 255 {
        return Err(ConfigError::InvalidRegistryUsername);
    }
    for endpoint in [internal_endpoint, public_endpoint].into_iter().flatten() {
        if !matches!(endpoint.scheme(), "http" | "https")
            || !endpoint.has_host()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || !matches!(endpoint.path(), "" | "/")
        {
            return Err(ConfigError::InvalidRegistryEndpoint);
        }
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("owner origin and owner email must be configured together")]
    IncompleteOwner,
    #[error("owner origin must be an absolute HTTP(S) origin")]
    InvalidOwnerOrigin,
    #[error("owner email is invalid")]
    InvalidOwnerEmail,
    #[error("registry endpoints and administrator password file must be configured together")]
    IncompleteRegistry,
    #[error("registry endpoints must be absolute HTTP(S) origins")]
    InvalidRegistryEndpoint,
    #[error("registry administrator username is invalid")]
    InvalidRegistryUsername,
    #[error("bootstrap email and password file must be configured together")]
    IncompleteBootstrap,
    #[error("TLS certificate and key files must be configured together")]
    IncompleteTls,
    #[error(
        "OIDC issuer, optional backchannel issuer, client ID, client secret file, and public callback URL must be configured together"
    )]
    IncompleteOidc,
    #[error(transparent)]
    FlowAccess(#[from] FlowAccessError),
    #[error(transparent)]
    Oidc(#[from] OidcConfigError),
    #[error(transparent)]
    SyouyuProvider(#[from] SyouyuProviderError),
    #[error("Flow public endpoints must be absolute HTTP(S) URLs")]
    InvalidFlowPublicEndpoint,
    #[error("Flow internal endpoint must be an absolute HTTP(S) base URL")]
    InvalidFlowInternalEndpoint,
    #[error("Flash internal endpoint must be an absolute HTTP(S) base URL")]
    InvalidFlashInternalEndpoint,
    #[error("Syouyu internal endpoint must be an absolute HTTP(S) base URL")]
    InvalidSyouyuInternalEndpoint,
    #[error("between one and sixteen Flow public endpoints are required")]
    MissingFlowPublicEndpoints,
    #[error("secret file is empty: {0}")]
    EmptySecret(PathBuf),
    #[error("secret path is not a regular file: {0}")]
    NotRegularFile(PathBuf),
    #[error("failed to read secret file {path}: {source}")]
    ReadSecret {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("secret file must not be group-writable/executable or accessible by other users: {0}")]
    UnsafePermissions(PathBuf),
    #[error("CSRF key must contain at least 32 bytes")]
    WeakCsrfKey,
    #[error("secure cookies require an https public origin")]
    SecureCookieRequiresHttps,
    #[error("secure mode requires https Flow public endpoints")]
    SecureModeRequiresHttpsFlowEndpoints,
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::{fs, os::unix::fs::PermissionsExt};

    #[cfg(unix)]
    use secrecy::ExposeSecret;
    use url::Url;
    #[cfg(unix)]
    use uuid::Uuid;

    #[cfg(unix)]
    use super::read_secret;
    use super::validate_flow_public_endpoints;

    #[test]
    fn secure_mode_requires_nonempty_https_flow_endpoints() -> Result<(), Box<dyn std::error::Error>>
    {
        let https = Url::parse("https://flow-a.example.test")?;
        let http = Url::parse("http://flow-a.example.test")?;
        let file = Url::parse("file:///tmp/flow.sock")?;
        assert!(validate_flow_public_endpoints(&[], true).is_err());
        assert!(validate_flow_public_endpoints(std::slice::from_ref(&https), true).is_ok());
        assert!(validate_flow_public_endpoints(std::slice::from_ref(&http), true).is_err());
        assert!(validate_flow_public_endpoints(&[http], false).is_ok());
        assert!(validate_flow_public_endpoints(&[file], false).is_err());
        assert!(
            validate_flow_public_endpoints(
                &[https.clone(), Url::parse("https://flow-a.example.test/")?],
                true,
            )
            .is_err()
        );
        assert!(
            validate_flow_public_endpoints(
                &[Url::parse(
                    "https://user:secret@flow-a.example.test/path?query=1"
                )?],
                true,
            )
            .is_err()
        );
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn projected_secret_symlink_accepts_dedicated_group_read_only()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory =
            std::env::temp_dir().join(format!("heterocloud-secret-test-{}", Uuid::now_v7()));
        let data_directory = directory.join("..data");
        fs::create_dir_all(&data_directory)?;
        let target = data_directory.join("hmac-secret");
        fs::write(&target, "0123456789abcdef0123456789abcdef\n")?;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o440))?;
        let link = directory.join("hmac-secret");
        std::os::unix::fs::symlink("..data/hmac-secret", &link)?;

        let secret = read_secret(&link).await?;
        assert_eq!(secret.expose_secret(), "0123456789abcdef0123456789abcdef");
        fs::set_permissions(&target, fs::Permissions::from_mode(0o444))?;
        assert!(read_secret(&link).await.is_err());
        fs::remove_dir_all(directory)?;
        Ok(())
    }
}
