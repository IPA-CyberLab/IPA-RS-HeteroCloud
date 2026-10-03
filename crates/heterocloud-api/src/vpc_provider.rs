use futures_util::{StreamExt, stream};
use heterocloud_domain::{PrincipalId, ServiceInstance, ServiceState};
use heterocloud_provider::{ProviderContext, ProviderSigner};
use serde_json::{Value, json};
use std::time::Duration;
use url::Url;

pub struct VpcProviderProxy {
    endpoint: Url,
    signer: ProviderSigner,
    client: reqwest::Client,
}
impl VpcProviderProxy {
    pub fn new(endpoint: Url, signer: ProviderSigner, client: reqwest::Client) -> Self {
        Self {
            endpoint,
            signer,
            client,
        }
    }
    async fn status(
        &self,
        principal: PrincipalId,
        instance: &ServiceInstance,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let token = self
            .signer
            .sign(ProviderContext {
                principal_id: principal,
                user_id: None,
                organization_id: instance.organization_id,
                project_id: instance.project_id,
                service_instance_id: instance.id,
                action: "vpc.status.get".into(),
                generation: instance.generation,
            })?
            .token;
        let url = self
            .endpoint
            .join(&format!("internal/v1/service-instances/{}", instance.id))?;
        let mut response = self
            .client
            .get(url)
            .bearer_auth(token)
            .timeout(Duration::from_secs(2))
            .send()
            .await?
            .error_for_status()?;
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if body.len().saturating_add(chunk.len()) > 256 * 1024 {
                return Err("VPC status too large".into());
            }
            body.extend_from_slice(&chunk)
        }
        let value: Value = serde_json::from_slice(&body)?;
        if value["generation"].as_i64() != Some(instance.generation) {
            return Err("VPC status generation mismatch".into());
        }
        Ok(value["status"].clone())
    }
}
pub async fn refresh(
    provider: Option<&VpcProviderProxy>,
    principal: PrincipalId,
    instance: ServiceInstance,
) -> ServiceInstance {
    refresh_many(provider, principal, vec![instance])
        .await
        .remove(0)
}
pub async fn refresh_many(
    provider: Option<&VpcProviderProxy>,
    principal: PrincipalId,
    instances: Vec<ServiceInstance>,
) -> Vec<ServiceInstance> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    stream::iter(instances.into_iter().map(|mut instance| async move {
        let live = async {
            let provider = provider.ok_or("provider is not configured")?;
            provider
                .status(principal, &instance)
                .await
                .map_err(|_| "provider status unavailable")
        };
        match tokio::time::timeout_at(deadline, live).await {
            Ok(Ok(status)) => {
                if instance.state == ServiceState::Ready && status["phase"] != "ready" {
                    instance.state = ServiceState::Updating;
                }
                instance.status = json!({"status":status,"observation":"current"});
            }
            _ => {
                instance.status["observation"] = json!("unavailable");
            }
        }
        instance
    }))
    .buffered(4)
    .collect()
    .await
}
