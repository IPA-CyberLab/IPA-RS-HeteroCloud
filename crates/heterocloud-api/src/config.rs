use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};

use clap::Parser;
use secrecy::{ExposeSecret, SecretString};
use thiserror::Error;
use tokio::fs;
use url::Url;

#[derive(Clone, Debug, Parser)]
#[command(version, about = "HeteroCloud control-plane API")]
pub struct Config {
    #[arg(long, env = "HETEROCLOUD_LISTEN", default_value = "0.0.0.0:8080")]
    pub listen: SocketAddr,

    #[arg(long, env = "HETEROCLOUD_DATABASE_URL_FILE")]
    pub database_url_file: PathBuf,

    #[arg(long, env = "HETEROCLOUD_CSRF_KEY_FILE")]
    pub csrf_key_file: PathBuf,

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
    pub secure_cookie: bool,
    pub session_ttl: Duration,
    pub csrf_key: SecretString,
}

impl Config {
    pub async fn load_secrets(&self) -> Result<LoadedSecrets, ConfigError> {
        let database_url = read_secret(&self.database_url_file).await?;
        let csrf_key = read_secret(&self.csrf_key_file).await?;
        if csrf_key.expose_secret().len() < 32 {
            return Err(ConfigError::WeakCsrfKey);
        }
        let bootstrap_password = match &self.bootstrap_password_file {
            Some(path) => Some(read_secret(path).await?),
            None => None,
        };
        if self.bootstrap_email.is_some() != bootstrap_password.is_some() {
            return Err(ConfigError::IncompleteBootstrap);
        }
        if self.tls_cert_file.is_some() != self.tls_key_file.is_some() {
            return Err(ConfigError::IncompleteTls);
        }
        if self.secure_cookie
            && (self.public_origin.scheme() != "https"
                || self
                    .additional_origins
                    .iter()
                    .any(|origin| origin.scheme() != "https"))
        {
            return Err(ConfigError::SecureCookieRequiresHttps);
        }
        Ok(LoadedSecrets {
            database_url,
            csrf_key,
            bootstrap_password,
        })
    }

    pub fn runtime(&self, csrf_key: SecretString) -> RuntimeConfig {
        let mut allowed_origins = vec![self.public_origin.origin().ascii_serialization()];
        for origin in &self.additional_origins {
            let serialized = origin.origin().ascii_serialization();
            if !allowed_origins.contains(&serialized) {
                allowed_origins.push(serialized);
            }
        }
        RuntimeConfig {
            public_origin: self.public_origin.clone(),
            allowed_origins,
            secure_cookie: self.secure_cookie,
            session_ttl: Duration::from_secs(self.session_ttl_seconds.clamp(300, 86_400)),
            csrf_key,
        }
    }
}

pub struct LoadedSecrets {
    pub database_url: SecretString,
    pub csrf_key: SecretString,
    pub bootstrap_password: Option<SecretString>,
}

async fn read_secret(path: &Path) -> Result<SecretString, ConfigError> {
    let metadata = fs::symlink_metadata(path)
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
        if metadata.mode() & 0o077 != 0 {
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

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("bootstrap email and password file must be configured together")]
    IncompleteBootstrap,
    #[error("TLS certificate and key files must be configured together")]
    IncompleteTls,
    #[error("secret file is empty: {0}")]
    EmptySecret(PathBuf),
    #[error("secret path is not a regular file: {0}")]
    NotRegularFile(PathBuf),
    #[error("failed to read secret file {path}: {source}")]
    ReadSecret {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("secret file must not be accessible by group or other users: {0}")]
    UnsafePermissions(PathBuf),
    #[error("CSRF key must contain at least 32 bytes")]
    WeakCsrfKey,
    #[error("secure cookies require an https public origin")]
    SecureCookieRequiresHttps,
}
