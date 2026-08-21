use axum::extract::ws::{Message as BrowserMessage, WebSocket};
use futures_util::{SinkExt, StreamExt};
use heterocloud_domain::{OrganizationId, PrincipalId, ProjectId, ServiceInstanceId};
use heterocloud_provider::{ProviderContext, ProviderSigner};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message as ProviderMessage, client::IntoClientRequest},
};
use url::Url;

const LIST_CONTAINERS_ACTION: &str = "flash.containers.list";
const EXEC_ACTION: &str = "flash.exec";

pub type ProviderWebSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub struct FlashProviderProxy {
    endpoint: Url,
    signer: ProviderSigner,
    client: Client,
}

impl FlashProviderProxy {
    pub fn new(endpoint: Url, signer: ProviderSigner, client: Client) -> Self {
        Self {
            endpoint,
            signer,
            client,
        }
    }

    pub async fn list_containers(
        &self,
        context: FlashProviderContext,
    ) -> Result<FlashContainerList, FlashProviderError> {
        let signed = self.sign(&context, LIST_CONTAINERS_ACTION)?;
        let mut url = self.endpoint.join(&format!(
            "internal/v1/service-instances/{}/containers",
            context.service_instance_id
        ))?;
        url.query_pairs_mut()
            .append_pair("generation", &context.generation.to_string());
        let response = self.client.get(url).bearer_auth(signed).send().await?;
        if !response.status().is_success() {
            return Err(FlashProviderError::ProviderStatus(
                response.status().as_u16(),
            ));
        }
        Ok(response.json().await?)
    }

    pub async fn connect_exec(
        &self,
        context: FlashProviderContext,
        pod: &str,
    ) -> Result<ProviderWebSocket, FlashProviderError> {
        let signed = self.sign(&context, EXEC_ACTION)?;
        let mut url = self.endpoint.join(&format!(
            "internal/v1/service-instances/{}/exec",
            context.service_instance_id
        ))?;
        url.query_pairs_mut()
            .append_pair("generation", &context.generation.to_string())
            .append_pair("pod", pod);
        let websocket_scheme = match url.scheme() {
            "http" => "ws",
            "https" => "wss",
            _ => return Err(FlashProviderError::InvalidEndpoint),
        };
        url.set_scheme(websocket_scheme)
            .map_err(|_| FlashProviderError::InvalidEndpoint)?;
        let mut request = url.as_str().into_client_request()?;
        request.headers_mut().insert(
            http::header::AUTHORIZATION,
            format!("Bearer {signed}").parse()?,
        );
        let (socket, _) = connect_async(request).await?;
        Ok(socket)
    }

    fn sign(
        &self,
        context: &FlashProviderContext,
        action: &str,
    ) -> Result<String, FlashProviderError> {
        Ok(self
            .signer
            .sign(ProviderContext {
                principal_id: context.principal_id,
                organization_id: context.organization_id,
                project_id: context.project_id,
                service_instance_id: context.service_instance_id,
                action: action.into(),
                generation: context.generation,
            })?
            .token)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FlashProviderContext {
    pub principal_id: PrincipalId,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub service_instance_id: ServiceInstanceId,
    pub generation: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FlashContainerList {
    pub items: Vec<FlashContainer>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FlashContainer {
    pub name: String,
    pub phase: String,
    pub ready: bool,
}

pub async fn bridge_websockets(browser: WebSocket, provider: ProviderWebSocket) {
    let (mut browser_send, mut browser_receive) = browser.split();
    let (mut provider_send, mut provider_receive) = provider.split();
    loop {
        tokio::select! {
            message = browser_receive.next() => {
                let Some(Ok(message)) = message else {
                    let _result = provider_send.send(ProviderMessage::Close(None)).await;
                    break;
                };
                let outbound = match message {
                    BrowserMessage::Text(value) => ProviderMessage::Text(value.as_str().into()),
                    BrowserMessage::Binary(value) => ProviderMessage::Binary(value.to_vec().into()),
                    BrowserMessage::Ping(value) => ProviderMessage::Ping(value.to_vec().into()),
                    BrowserMessage::Pong(value) => ProviderMessage::Pong(value.to_vec().into()),
                    BrowserMessage::Close(_) => {
                        let _result = provider_send.send(ProviderMessage::Close(None)).await;
                        break;
                    }
                };
                if provider_send.send(outbound).await.is_err() {
                    break;
                }
            }
            message = provider_receive.next() => {
                let Some(Ok(message)) = message else {
                    let _result = browser_send.send(BrowserMessage::Close(None)).await;
                    break;
                };
                let outbound = match message {
                    ProviderMessage::Text(value) => BrowserMessage::Text(value.as_str().into()),
                    ProviderMessage::Binary(value) => BrowserMessage::Binary(value.to_vec().into()),
                    ProviderMessage::Ping(value) => BrowserMessage::Ping(value.to_vec().into()),
                    ProviderMessage::Pong(value) => BrowserMessage::Pong(value.to_vec().into()),
                    ProviderMessage::Close(_) => {
                        let _result = browser_send.send(BrowserMessage::Close(None)).await;
                        break;
                    }
                    ProviderMessage::Frame(_) => continue,
                };
                if browser_send.send(outbound).await.is_err() {
                    break;
                }
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FlashProviderError {
    #[error("Flash provider endpoint is invalid")]
    InvalidEndpoint,
    #[error("Flash provider returned HTTP {0}")]
    ProviderStatus(u16),
    #[error(transparent)]
    Header(#[from] http::header::InvalidHeaderValue),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error(transparent)]
    Provider(#[from] heterocloud_provider::ProviderError),
    #[error(transparent)]
    Url(#[from] url::ParseError),
    #[error(transparent)]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),
}

#[cfg(test)]
mod tests {
    use super::{FlashContainer, FlashContainerList};

    #[test]
    fn container_list_contract_is_stable() -> Result<(), Box<dyn std::error::Error>> {
        let list: FlashContainerList = serde_json::from_value(serde_json::json!({
            "items": [{"name": "flash-a-1", "phase": "Running", "ready": true}]
        }))?;
        assert_eq!(
            list.items,
            vec![FlashContainer {
                name: "flash-a-1".into(),
                phase: "Running".into(),
                ready: true,
            }]
        );
        Ok(())
    }
}
