use std::collections::BTreeSet;

use heterocloud_domain::{OrganizationId, ResourceQuotaLimits};
use reqwest::{Client, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use url::{Host, Url};

const GIB_BYTES: u64 = 1024 * 1024 * 1024;
const HARBOR_PAGE_SIZE: usize = 100;
const MAX_REGISTRY_IMAGES: usize = 500;
const MAX_REGISTRY_REPOSITORIES: usize = 100;

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

    pub async fn list_images(
        &self,
        project: &RegistryProject,
    ) -> Result<Vec<RegistryImage>, RegistryError> {
        let mut repositories_url = self
            .internal_endpoint
            .join(&format!("api/v2.0/projects/{}/repositories", project.name))?;
        repositories_url
            .query_pairs_mut()
            .append_pair("page", "1")
            .append_pair("page_size", &HARBOR_PAGE_SIZE.to_string())
            .append_pair("sort", "-update_time");
        let response = self
            .request(self.client.get(repositories_url))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(RegistryError::Status(response.status().as_u16()));
        }
        let repositories: Vec<RepositoryResponse> = response.json().await?;
        let authority = project.authority()?;
        let mut references = BTreeSet::new();
        let mut images = Vec::new();

        for repository in repositories.into_iter().take(MAX_REGISTRY_REPOSITORIES) {
            let repository_name = relative_repository_name(&project.name, &repository.name);
            if repository_name.is_empty() {
                continue;
            }
            let mut page = 1;
            while images.len() < MAX_REGISTRY_IMAGES {
                let mut artifacts_url =
                    artifact_list_url(&self.internal_endpoint, &project.name, repository_name)?;
                artifacts_url
                    .query_pairs_mut()
                    .append_pair("page", &page.to_string())
                    .append_pair("page_size", &HARBOR_PAGE_SIZE.to_string())
                    .append_pair("q", "tags=*")
                    .append_pair("sort", "-push_time")
                    .append_pair("with_tag", "true")
                    .append_pair("with_label", "false")
                    .append_pair("with_scan_overview", "false")
                    .append_pair("with_sbom_overview", "false");
                let response = self.request(self.client.get(artifacts_url)).send().await?;
                if !response.status().is_success() {
                    return Err(RegistryError::Status(response.status().as_u16()));
                }
                let artifacts: Vec<ArtifactResponse> = response.json().await?;
                let artifact_count = artifacts.len();
                for artifact in artifacts {
                    if artifact
                        .kind
                        .as_deref()
                        .is_some_and(|kind| !kind.eq_ignore_ascii_case("image"))
                    {
                        continue;
                    }
                    for tag in artifact.tags {
                        let reference = format!(
                            "{authority}/{}/{repository_name}:{}",
                            project.name, tag.name
                        );
                        if !references.insert(reference.clone()) {
                            continue;
                        }
                        images.push(RegistryImage {
                            reference,
                            repository: repository_name.to_owned(),
                            tag: tag.name,
                            digest: artifact.digest.clone(),
                            size_bytes: artifact.size.max(0) as u64,
                            pushed_at: artifact.push_time.clone(),
                        });
                        if images.len() >= MAX_REGISTRY_IMAGES {
                            break;
                        }
                    }
                    if images.len() >= MAX_REGISTRY_IMAGES {
                        break;
                    }
                }
                if artifact_count < HARBOR_PAGE_SIZE {
                    break;
                }
                page += 1;
            }
            if images.len() >= MAX_REGISTRY_IMAGES {
                break;
            }
        }

        images.sort_by(|left, right| {
            right
                .pushed_at
                .cmp(&left.pushed_at)
                .then_with(|| left.reference.cmp(&right.reference))
        });
        Ok(images)
    }

    pub async fn delete_image(
        &self,
        project: &RegistryProject,
        repository_name: &str,
        digest: &str,
    ) -> Result<bool, RegistryError> {
        let url = artifact_url(
            &self.internal_endpoint,
            &project.name,
            repository_name,
            digest,
        )?;
        let response = self.request(self.client.delete(url)).send().await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(false);
        }
        if !response.status().is_success() {
            return Err(RegistryError::Status(response.status().as_u16()));
        }
        Ok(true)
    }

    pub async fn storage_usage(&self, project: &RegistryProject) -> Result<u64, RegistryError> {
        let quota = self.project_quota(project.project_id).await?;
        Ok(quota.used.get("storage").copied().unwrap_or(0).max(0) as u64)
    }

    pub async fn delete_credential(&self, robot_id: i64) -> Result<(), RegistryError> {
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

impl RegistryProject {
    pub fn authority(&self) -> Result<String, RegistryError> {
        registry_authority(&self.endpoint)
    }

    pub fn image_prefix(&self) -> Result<String, RegistryError> {
        Ok(format!("{}/{}", self.authority()?, self.name))
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct RegistryImage {
    pub reference: String,
    pub repository: String,
    pub tag: String,
    pub digest: String,
    pub size_bytes: u64,
    pub pushed_at: Option<String>,
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
struct RepositoryResponse {
    name: String,
}

#[derive(Deserialize)]
struct ArtifactResponse {
    #[serde(rename = "type")]
    kind: Option<String>,
    digest: String,
    size: i64,
    push_time: Option<String>,
    #[serde(default)]
    tags: Vec<TagResponse>,
}

#[derive(Deserialize)]
struct TagResponse {
    name: String,
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
    #[error("registry endpoint is invalid")]
    InvalidEndpoint,
    #[error("registry returned HTTP {0}")]
    Status(u16),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error(transparent)]
    Url(#[from] url::ParseError),
}

fn relative_repository_name<'a>(project_name: &str, repository_name: &'a str) -> &'a str {
    repository_name
        .strip_prefix(project_name)
        .and_then(|name| name.strip_prefix('/'))
        .unwrap_or(repository_name)
}

fn artifact_list_url(
    endpoint: &Url,
    project_name: &str,
    repository_name: &str,
) -> Result<Url, RegistryError> {
    let encoded_repository = repository_name.replace('/', "%2F");
    let mut url = endpoint.clone();
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|()| RegistryError::InvalidEndpoint)?;
        segments.pop_if_empty();
        segments.extend(["api", "v2.0", "projects", project_name, "repositories"]);
        segments.push(&encoded_repository);
        segments.push("artifacts");
    }
    Ok(url)
}

fn artifact_url(
    endpoint: &Url,
    project_name: &str,
    repository_name: &str,
    reference: &str,
) -> Result<Url, RegistryError> {
    let mut url = artifact_list_url(endpoint, project_name, repository_name)?;
    url.path_segments_mut()
        .map_err(|()| RegistryError::InvalidEndpoint)?
        .push(reference);
    Ok(url)
}

fn registry_authority(endpoint: &Url) -> Result<String, RegistryError> {
    let host = match endpoint.host().ok_or(RegistryError::InvalidEndpoint)? {
        Host::Domain(host) => host.to_owned(),
        Host::Ipv4(host) => host.to_string(),
        Host::Ipv6(host) => format!("[{host}]"),
    };
    Ok(match endpoint.port() {
        Some(port) => format!("{host}:{port}"),
        None => host,
    })
}

#[cfg(test)]
mod tests {
    use super::{artifact_list_url, artifact_url, registry_authority, relative_repository_name};
    use url::Url;

    #[test]
    fn registry_image_references_omit_the_url_scheme() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            registry_authority(&Url::parse("https://registry.example.com/")?)?,
            "registry.example.com"
        );
        assert_eq!(
            registry_authority(&Url::parse("https://registry.example.com:5443/")?)?,
            "registry.example.com:5443"
        );
        Ok(())
    }

    #[test]
    fn nested_harbor_repository_names_are_double_encoded() -> Result<(), Box<dyn std::error::Error>>
    {
        let repository = relative_repository_name("hc-tenant", "hc-tenant/team/game");
        assert_eq!(repository, "team/game");
        assert_eq!(
            artifact_list_url(&Url::parse("http://harbor-core/")?, "hc-tenant", repository)?
                .as_str(),
            "http://harbor-core/api/v2.0/projects/hc-tenant/repositories/team%252Fgame/artifacts"
        );
        Ok(())
    }

    #[test]
    fn artifact_delete_url_preserves_the_digest_reference() -> Result<(), Box<dyn std::error::Error>>
    {
        assert_eq!(
            artifact_url(
                &Url::parse("http://harbor-core/")?,
                "hc-tenant",
                "team/game",
                "sha256:0123456789abcdef",
            )?
            .as_str(),
            "http://harbor-core/api/v2.0/projects/hc-tenant/repositories/team%252Fgame/artifacts/sha256:0123456789abcdef"
        );
        Ok(())
    }
}
