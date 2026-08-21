use heterocloud_domain::{OrganizationId, ResourceQuotaLimits};
use reqwest::{Client, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use url::Url;

const GIB_BYTES: u64 = 1024 * 1024 * 1024;

pub struct RegistryClient {
    internal_endpoint: Url,
    public_endpoint: Url,
    username: String,
    password: SecretString,
    client: Client,
}

impl RegistryClient {
    pub fn new(
        internal_endpoint: Url,
        public_endpoint: Url,
        username: String,
        password: SecretString,
        client: Client,
    ) -> Self {
        Self {
            internal_endpoint,
            public_endpoint,
            username,
            password,
            client,
        }
    }

    pub async fn ensure_project(
        &self,
        organization_id: OrganizationId,
        limits: &ResourceQuotaLimits,
    ) -> Result<RegistryProject, RegistryError> {
        let name = project_name(organization_id);
        let storage_bytes = u64::from(limits.registry.storage_gib)
            .checked_mul(GIB_BYTES)
            .and_then(|value| i64::try_from(value).ok())
            .ok_or(RegistryError::InvalidQuota)?;
        let project = match self.project(&name).await? {
            Some(project) => project,
            None => {
                let url = self.internal_endpoint.join("api/v2.0/projects")?;
                let response = self
                    .request(self.client.post(url))
                    .json(&json!({
                        "project_name": name,
                        "metadata": {"public": "false"},
                        "storage_limit": storage_bytes,
                    }))
                    .send()
                    .await?;
                if response.status() != StatusCode::CREATED
                    && response.status() != StatusCode::CONFLICT
                {
                    return Err(RegistryError::Status(response.status().as_u16()));
                }
                self.project(&name)
                    .await?
                    .ok_or(RegistryError::MissingProject)?
            }
        };
        let mut quota = self.project_quota(project.project_id).await?;
        if quota.hard.get("storage").copied() != Some(storage_bytes) {
            let url = self
                .internal_endpoint
                .join(&format!("api/v2.0/quotas/{}", quota.id))?;
            let response = self
                .request(self.client.put(url))
                .json(&json!({"hard": {"storage": storage_bytes}}))
                .send()
                .await?;
            if !response.status().is_success() {
                return Err(RegistryError::Status(response.status().as_u16()));
            }
            quota.hard.insert("storage".into(), storage_bytes);
        }
        Ok(RegistryProject {
            name,
            project_id: project.project_id,
            quota_id: quota.id,
            storage_limit_bytes: storage_bytes,
            storage_used_bytes: quota.used.get("storage").copied().unwrap_or(0).max(0) as u64,
            endpoint: self.public_endpoint.clone(),
        })
    }

    pub async fn create_push_credential(
        &self,
        project: &RegistryProject,
        name: &str,
    ) -> Result<RegistryCredentialSecret, RegistryError> {
        let url = self.internal_endpoint.join("api/v2.0/robots")?;
        let response = self
            .request(self.client.post(url))
            .json(&json!({
                "name": name,
                "description": "Managed by HeteroCloud",
                "level": "project",
                "disable": false,
                "duration": -1,
                "permissions": [{
                    "kind": "project",
                    "namespace": project.name,
                    "access": [
                        {"resource": "repository", "action": "pull", "effect": "allow"},
                        {"resource": "repository", "action": "push", "effect": "allow"},
                        {"resource": "artifact", "action": "read", "effect": "allow"},
                        {"resource": "artifact", "action": "create", "effect": "allow"}
                    ]
                }]
            }))
            .send()
            .await?;
        if response.status() != StatusCode::CREATED {
            return Err(RegistryError::Status(response.status().as_u16()));
        }
        let created: RobotCreated = response.json().await?;
        Ok(RegistryCredentialSecret {
            robot_id: created.id,
            username: created.name,
            password: created.secret,
        })
    }

    pub async fn revoke_credential(&self, robot_id: i64) -> Result<(), RegistryError> {
        let url = self
            .internal_endpoint
            .join(&format!("api/v2.0/robots/{robot_id}"))?;
        let response = self.request(self.client.delete(url)).send().await?;
        if response.status() != StatusCode::OK && response.status() != StatusCode::NOT_FOUND {
            return Err(RegistryError::Status(response.status().as_u16()));
        }
        Ok(())
    }

    async fn project(&self, name: &str) -> Result<Option<ProjectResponse>, RegistryError> {
        let url = self
            .internal_endpoint
            .join(&format!("api/v2.0/projects/{name}"))?;
        let response = self
            .request(self.client.get(url).header("X-Is-Resource-Name", "true"))
            .send()
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(RegistryError::Status(response.status().as_u16()));
        }
        Ok(Some(response.json().await?))
    }

    async fn project_quota(&self, project_id: i64) -> Result<QuotaResponse, RegistryError> {
        let mut url = self.internal_endpoint.join("api/v2.0/quotas")?;
        url.query_pairs_mut()
            .append_pair("reference", "project")
            .append_pair("reference_id", &project_id.to_string())
            .append_pair("page_size", "10");
        let response = self.request(self.client.get(url)).send().await?;
        if !response.status().is_success() {
            return Err(RegistryError::Status(response.status().as_u16()));
        }
        let quotas: Vec<QuotaResponse> = response.json().await?;
        quotas.into_iter().next().ok_or(RegistryError::MissingQuota)
    }

    fn request(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        request.basic_auth(&self.username, Some(self.password.expose_secret()))
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct RegistryProject {
    pub name: String,
    #[serde(skip_serializing)]
    pub project_id: i64,
    #[serde(skip_serializing)]
    pub quota_id: i64,
    pub storage_limit_bytes: i64,
    pub storage_used_bytes: u64,
    pub endpoint: Url,
}

#[derive(Clone, Debug, Serialize)]
pub struct RegistryCredentialSecret {
    #[serde(skip_serializing)]
    pub robot_id: i64,
    pub username: String,
    pub password: String,
}

#[derive(Deserialize)]
struct ProjectResponse {
    project_id: i64,
}

#[derive(Deserialize)]
struct QuotaResponse {
    id: i64,
    #[serde(default)]
    hard: std::collections::BTreeMap<String, i64>,
    #[serde(default)]
    used: std::collections::BTreeMap<String, i64>,
    #[allow(dead_code)]
    ref_data: Option<Value>,
}

#[derive(Deserialize)]
struct RobotCreated {
    id: i64,
    name: String,
    secret: String,
}

pub fn project_name(organization_id: OrganizationId) -> String {
    format!("hc-{}", organization_id.0.simple())
}

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("registry quota is invalid")]
    InvalidQuota,
    #[error("registry project was not returned after creation")]
    MissingProject,
    #[error("registry quota was not found")]
    MissingQuota,
    #[error("registry returned HTTP {0}")]
    Status(u16),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error(transparent)]
    Url(#[from] url::ParseError),
}
