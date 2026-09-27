//! Narrow OpenBao KV v2 client for Flash workload secrets.
//!
//! Secret values and Kubernetes credentials are never stored in the HeteroCloud
//! database or returned to callers after a write.

use std::time::Duration;

use reqwest::{Client, StatusCode};
use serde::Deserialize;
use serde_json::json;
use url::Url;
use uuid::Uuid;

const TOKEN_PATH: &str = "/var/run/secrets/openbao-identity/token";
const ROLE: &str = "heterosecrets-flash-api";

#[derive(Debug)]
pub enum SecretManagerError {
    Unavailable,
    Missing,
}

#[derive(Deserialize)]
struct LoginResponse {
    auth: LoginAuth,
}

#[derive(Deserialize)]
struct LoginAuth {
    client_token: String,
}

#[derive(Deserialize)]
struct ListResponse {
    data: ListData,
}

#[derive(Deserialize)]
struct ListData {
    keys: Vec<String>,
}

pub struct SecretManagerClient {
    origin: Url,
    http: Client,
    token: String,
}

impl SecretManagerClient {
    pub async fn login(origin: &Url) -> Result<Self, SecretManagerError> {
        let jwt = tokio::fs::read_to_string(TOKEN_PATH)
            .await
            .map_err(|_| SecretManagerError::Unavailable)?;
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| SecretManagerError::Unavailable)?;
        let endpoint = origin
            .join("v1/auth/kubernetes/login")
            .map_err(|_| SecretManagerError::Unavailable)?;
        let response = http
            .post(endpoint)
            .json(&json!({ "role": ROLE, "jwt": jwt.trim() }))
            .send()
            .await
            .map_err(|_| SecretManagerError::Unavailable)?;
        if !response.status().is_success() {
            return Err(SecretManagerError::Unavailable);
        }
        let login: LoginResponse = response
            .json()
            .await
            .map_err(|_| SecretManagerError::Unavailable)?;
        if login.auth.client_token.is_empty() {
            return Err(SecretManagerError::Unavailable);
        }
        Ok(Self {
            origin: origin.clone(),
            http,
            token: login.auth.client_token,
        })
    }

    fn endpoint(&self, path: &str) -> Result<Url, SecretManagerError> {
        self.origin
            .join(&format!("v1/secret/{path}"))
            .map_err(|_| SecretManagerError::Unavailable)
    }

    fn service_path(service_id: Uuid) -> String {
        format!("flash-{}", service_id.simple())
    }

    pub async fn list(&self, service_id: Uuid) -> Result<Vec<String>, SecretManagerError> {
        let mut endpoint = self.endpoint(&format!(
            "metadata/flash/{}",
            Self::service_path(service_id)
        ))?;
        endpoint.query_pairs_mut().append_pair("list", "true");
        let response = self
            .http
            .get(endpoint)
            .header("X-Vault-Token", &self.token)
            .send()
            .await
            .map_err(|_| SecretManagerError::Unavailable)?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(Vec::new());
        }
        if !response.status().is_success() {
            return Err(SecretManagerError::Unavailable);
        }
        let listing: ListResponse = response
            .json()
            .await
            .map_err(|_| SecretManagerError::Unavailable)?;
        Ok(listing
            .data
            .keys
            .into_iter()
            .filter(|key| !key.ends_with('/'))
            .collect())
    }

    pub async fn put(
        &self,
        service_id: Uuid,
        name: &str,
        value: &str,
    ) -> Result<(), SecretManagerError> {
        let endpoint = self.endpoint(&format!(
            "data/flash/{}/{}",
            Self::service_path(service_id),
            name
        ))?;
        let response = self
            .http
            .post(endpoint)
            .header("X-Vault-Token", &self.token)
            .json(&json!({ "data": { "value": value } }))
            .send()
            .await
            .map_err(|_| SecretManagerError::Unavailable)?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(SecretManagerError::Unavailable)
        }
    }

    pub async fn delete(&self, service_id: Uuid, name: &str) -> Result<(), SecretManagerError> {
        let endpoint = self.endpoint(&format!(
            "metadata/flash/{}/{}",
            Self::service_path(service_id),
            name
        ))?;
        let response = self
            .http
            .delete(endpoint)
            .header("X-Vault-Token", &self.token)
            .send()
            .await
            .map_err(|_| SecretManagerError::Unavailable)?;
        if response.status() == StatusCode::NOT_FOUND {
            return Err(SecretManagerError::Missing);
        }
        if response.status().is_success() {
            Ok(())
        } else {
            Err(SecretManagerError::Unavailable)
        }
    }
}
