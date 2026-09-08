use std::time::Duration;

use axum::extract::ws::{Message as BrowserMessage, WebSocket};
use futures_util::{SinkExt, StreamExt, stream};
use heterocloud_domain::{
    OrganizationId, PrincipalId, ProjectId, ServiceInstance, ServiceInstanceId, ServiceState,
};
use heterocloud_provider::{ProviderContext, ProviderSigner};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio::time::{Instant, timeout_at};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message as ProviderMessage, client::IntoClientRequest},
};
use url::Url;

const LIST_CONTAINERS_ACTION: &str = "flash.containers.list";
const EXEC_ACTION: &str = "flash.exec";
const STATUS_ACTION: &str = "flash.status.get";
const LIVE_STATUS_BUDGET: Duration = Duration::from_secs(2);
const LIVE_STATUS_CONCURRENCY: usize = 4;
const MAX_LIVE_STATUS_BYTES: usize = 64 * 1024;

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

    pub async fn get_status(
        &self,
        context: FlashProviderContext,
    ) -> Result<Value, FlashProviderError> {
        let signed = self.sign(&context, STATUS_ACTION)?;
        let mut url = self.endpoint.join(&format!(
            "internal/v1/service-instances/{}",
            context.service_instance_id
        ))?;
        url.query_pairs_mut()
            .append_pair("generation", &context.generation.to_string());
        let mut response = self
            .client
            .get(url)
            .bearer_auth(signed)
            .timeout(LIVE_STATUS_BUDGET)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(FlashProviderError::ProviderStatus(
                response.status().as_u16(),
            ));
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if chunk.len() > MAX_LIVE_STATUS_BYTES - body.len() {
                return Err(FlashProviderError::InvalidLiveStatus);
            }
            body.extend_from_slice(&chunk);
        }
        let status: Value =
            serde_json::from_slice(&body).map_err(|_| FlashProviderError::InvalidLiveStatus)?;
        if status.get("observed_generation").and_then(Value::as_i64) != Some(context.generation)
            || status
                .get("desired_replicas")
                .and_then(Value::as_u64)
                .is_none()
            || status
                .get("ready_replicas")
                .and_then(Value::as_u64)
                .is_none()
            || status.get("phase").and_then(Value::as_str).is_none()
            || status.get("endpoints").and_then(Value::as_array).is_none()
        {
            return Err(FlashProviderError::InvalidLiveStatus);
        }
        Ok(status)
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

pub async fn refresh_autoscaled_status(
    provider: Option<&FlashProviderProxy>,
    principal_id: PrincipalId,
    instance: ServiceInstance,
) -> ServiceInstance {
    refresh_status_before(
        provider,
        principal_id,
        instance,
        Instant::now() + LIVE_STATUS_BUDGET,
    )
    .await
}

pub async fn refresh_autoscaled_statuses(
    provider: Option<&FlashProviderProxy>,
    principal_id: PrincipalId,
    instances: Vec<ServiceInstance>,
) -> Vec<ServiceInstance> {
    // One deadline includes queue time: a large/unavailable fleet cannot extend the request.
    let deadline = Instant::now() + LIVE_STATUS_BUDGET;
    stream::iter(instances)
        .map(|instance| refresh_status_before(provider, principal_id, instance, deadline))
        .buffered(LIVE_STATUS_CONCURRENCY)
        .collect()
        .await
}

async fn refresh_status_before(
    provider: Option<&FlashProviderProxy>,
    principal_id: PrincipalId,
    mut instance: ServiceInstance,
    deadline: Instant,
) -> ServiceInstance {
    if instance.provider != "flash"
        || !instance
            .spec
            .get("autoscaling")
            .is_some_and(Value::is_object)
    {
        return instance;
    }
    if !instance.status.is_object() {
        instance.status = json!({});
    }
    if !instance.status["status"].is_object() {
        instance.status["status"] = json!({});
    }
    let inner = instance.status["status"].as_object_mut();
    if let Some(inner) = inner {
        inner.insert("live_status_unavailable".into(), json!(true));
        inner.remove("desired_replicas");
        inner.remove("ready_replicas");
    }
    let Some(provider) = provider else {
        return instance;
    };
    if instance.state == ServiceState::Deleting || Instant::now() >= deadline {
        return instance;
    }
    let context = FlashProviderContext {
        principal_id,
        organization_id: instance.organization_id,
        project_id: instance.project_id,
        service_instance_id: instance.id,
        generation: instance.generation,
    };
    if let Ok(Ok(mut status)) = timeout_at(deadline, provider.get_status(context)).await {
        if let Some(status) = status.as_object_mut() {
            status.remove("live_status_unavailable");
        }
        // Response-only overlay: never reconcile or change persisted lifecycle/generation.
        instance.status["status"] = status;
    }
    instance
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
    #[error("Flash provider live status is malformed or has a different observed generation")]
    InvalidLiveStatus,
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
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use axum::{Router, routing::any};
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    use chrono::Utc;
    use http::{HeaderMap, Method, StatusCode, Uri};
    use tokio::{net::TcpListener, task::JoinHandle};
    use uuid::Uuid;

    use super::*;

    const TEST_KEY: &[u8] = b"-----BEGIN PRIVATE KEY-----\n\
MC4CAQAwBQYDK2VwBCIEIG45L/crBYvUcHKXo1ZbNr3YBSD3wPhsGq7IKyuU2+ei\n\
-----END PRIVATE KEY-----\n";

    struct MockProvider {
        proxy: FlashProviderProxy,
        requests: Arc<Mutex<Vec<(Method, Uri, HeaderMap)>>>,
        peak: Arc<AtomicUsize>,
        task: JoinHandle<()>,
    }

    impl Drop for MockProvider {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn mock_provider(
        code: StatusCode,
        body: String,
        delay: Duration,
    ) -> Result<MockProvider, Box<dyn std::error::Error>> {
        let _installed = rustls::crypto::ring::default_provider().install_default();
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let endpoint = Url::parse(&format!("http://{}/", listener.local_addr()?))?;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let app = Router::new().fallback(any({
            let requests = requests.clone();
            let peak = peak.clone();
            move |method: Method, uri: Uri, headers: HeaderMap| {
                let requests = requests.clone();
                let active = active.clone();
                let peak = peak.clone();
                let body = body.clone();
                async move {
                    if let Ok(mut requests) = requests.lock() {
                        requests.push((method, uri, headers));
                    }
                    let count = active.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(count, Ordering::SeqCst);
                    tokio::time::sleep(delay).await;
                    active.fetch_sub(1, Ordering::SeqCst);
                    (
                        code,
                        [(http::header::CONTENT_TYPE, "application/json")],
                        body,
                    )
                }
            }
        }));
        let task = tokio::spawn(async move {
            let _result = axum::serve(listener, app).await;
        });
        Ok(MockProvider {
            proxy: FlashProviderProxy::new(
                endpoint,
                ProviderSigner::from_ed25519_pem(
                    "heterocloud",
                    "heterocloud-flash",
                    "test",
                    TEST_KEY,
                )?,
                Client::builder().no_proxy().build()?,
            ),
            requests,
            peak,
            task,
        })
    }

    fn service(id: u128) -> ServiceInstance {
        ServiceInstance {
            id: ServiceInstanceId(Uuid::from_u128(id)),
            organization_id: OrganizationId(Uuid::from_u128(2)),
            project_id: ProjectId(Uuid::from_u128(3)),
            provider: "flash".into(),
            name: "scaling".into(),
            generation: 7,
            state: ServiceState::Ready,
            spec: json!({"replicas": 1, "autoscaling": {"min_replicas": 1, "max_replicas": 8, "target_cpu_utilization_percent": 70}}),
            status: json!({"operation_id": "original", "status": {
                "phase": "ready", "observed_generation": 7,
                "desired_replicas": 1, "ready_replicas": 1,
                "endpoints": [{"hostname": "cached.example.test"}]
            }}),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn live_status() -> Value {
        json!({"observed_generation": 7, "desired_replicas": 6, "ready_replicas": 4,
            "phase": "provisioning", "endpoints": [{"hostname": "live.example.test"}],
            "runtime_class": "gvisor", "message": "scaling"})
    }

    fn assert_stale(instance: &ServiceInstance, original: &ServiceInstance) {
        assert_eq!(instance.status["status"]["live_status_unavailable"], true);
        assert!(instance.status["status"].get("desired_replicas").is_none());
        assert!(instance.status["status"].get("ready_replicas").is_none());
        assert_eq!(
            instance.status["status"]["endpoints"],
            original.status["status"]["endpoints"]
        );
        assert_eq!(instance.state, original.state);
        assert_eq!(instance.generation, original.generation);
        assert_eq!(instance.spec, original.spec);
        assert_eq!(instance.updated_at, original.updated_at);
    }

    #[tokio::test]
    async fn live_status_uses_signed_get_and_only_overlays_status()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut status = live_status();
        status["live_status_unavailable"] = json!(true);
        let mock = mock_provider(StatusCode::OK, status.to_string(), Duration::ZERO).await?;
        let original = service(1);
        let principal = PrincipalId(Uuid::from_u128(4));
        let refreshed =
            refresh_autoscaled_status(Some(&mock.proxy), principal, original.clone()).await;
        assert_eq!(refreshed.status["status"], live_status());
        assert_eq!(refreshed.status["operation_id"], "original");
        let mut expected = serde_json::to_value(&original)?;
        expected["status"] = refreshed.status.clone();
        assert_eq!(serde_json::to_value(&refreshed)?, expected);
        let requests = mock.requests.lock().map_err(|_| "request lock poisoned")?;
        let (method, uri, headers) = requests.first().ok_or("missing request")?;
        assert_eq!(*method, Method::GET);
        assert_eq!(
            uri.path(),
            format!("/internal/v1/service-instances/{}", original.id)
        );
        assert_eq!(uri.query(), Some("generation=7"));
        let token = headers
            .get(http::header::AUTHORIZATION)
            .ok_or("missing authorization")?
            .to_str()?
            .strip_prefix("Bearer ")
            .ok_or("not bearer")?;
        let payload = token.split('.').nth(1).ok_or("missing JWT payload")?;
        let claims: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload)?)?;
        assert_eq!(claims["action"], "flash.status.get");
        assert_eq!(claims["generation"], 7);
        assert_eq!(claims["sub"], principal.to_string());
        assert_eq!(
            claims["organization_id"],
            original.organization_id.to_string()
        );
        assert_eq!(claims["project_id"], original.project_id.to_string());
        assert_eq!(claims["service_instance_id"], original.id.to_string());
        assert_eq!(claims["aud"], "heterocloud-flash");
        Ok(())
    }

    #[tokio::test]
    async fn unavailable_invalid_and_wrong_generation_statuses_are_stale_not_errors()
    -> Result<(), Box<dyn std::error::Error>> {
        let original = service(1);
        let mut old = live_status();
        old["observed_generation"] = json!(6);
        let mut future = live_status();
        future["observed_generation"] = json!(8);
        let mut missing = live_status();
        missing["ready_replicas"] = Value::Null;
        for (code, body) in [
            (StatusCode::SERVICE_UNAVAILABLE, live_status().to_string()),
            (StatusCode::FORBIDDEN, live_status().to_string()),
            (StatusCode::NOT_FOUND, live_status().to_string()),
            (StatusCode::OK, "not json".into()),
            (StatusCode::OK, old.to_string()),
            (StatusCode::OK, future.to_string()),
            (StatusCode::OK, missing.to_string()),
            (StatusCode::OK, " ".repeat(MAX_LIVE_STATUS_BYTES + 1)),
        ] {
            let mock = mock_provider(code, body, Duration::ZERO).await?;
            let refreshed = refresh_autoscaled_status(
                Some(&mock.proxy),
                PrincipalId(Uuid::nil()),
                original.clone(),
            )
            .await;
            assert_stale(&refreshed, &original);
        }
        let refreshed =
            refresh_autoscaled_status(None, PrincipalId(Uuid::nil()), original.clone()).await;
        assert_stale(&refreshed, &original);
        Ok(())
    }

    #[tokio::test]
    async fn fixed_services_are_unchanged_and_deleting_services_are_not_fetched()
    -> Result<(), Box<dyn std::error::Error>> {
        let mock = mock_provider(StatusCode::OK, live_status().to_string(), Duration::ZERO).await?;
        let mut fixed = service(1);
        fixed.spec = json!({"replicas": 1});
        let refreshed =
            refresh_autoscaled_status(Some(&mock.proxy), PrincipalId(Uuid::nil()), fixed.clone())
                .await;
        assert_eq!(
            serde_json::to_value(refreshed)?,
            serde_json::to_value(fixed)?
        );
        let mut deleting = service(2);
        deleting.state = ServiceState::Deleting;
        let refreshed = refresh_autoscaled_status(
            Some(&mock.proxy),
            PrincipalId(Uuid::nil()),
            deleting.clone(),
        )
        .await;
        assert_stale(&refreshed, &deleting);
        assert!(
            mock.requests
                .lock()
                .map_err(|_| "request lock poisoned")?
                .is_empty()
        );
        Ok(())
    }

    #[tokio::test]
    async fn list_refresh_is_concurrent_and_preserves_order()
    -> Result<(), Box<dyn std::error::Error>> {
        let mock = mock_provider(
            StatusCode::OK,
            live_status().to_string(),
            Duration::from_millis(30),
        )
        .await?;
        let originals: Vec<_> = (1..=12).map(service).collect();
        let refreshed = refresh_autoscaled_statuses(
            Some(&mock.proxy),
            PrincipalId(Uuid::nil()),
            originals.clone(),
        )
        .await;
        assert_eq!(mock.peak.load(Ordering::SeqCst), LIVE_STATUS_CONCURRENCY);
        for (current, original) in refreshed.iter().zip(&originals) {
            assert_eq!(current.id, original.id);
            assert_eq!(current.status["status"], live_status());
        }
        Ok(())
    }

    #[tokio::test]
    async fn unavailable_large_list_has_one_absolute_two_second_budget()
    -> Result<(), Box<dyn std::error::Error>> {
        let mock = mock_provider(
            StatusCode::OK,
            live_status().to_string(),
            Duration::from_secs(10),
        )
        .await?;
        let originals: Vec<_> = (1..=100).map(service).collect();
        let started = Instant::now();
        let refreshed = refresh_autoscaled_statuses(
            Some(&mock.proxy),
            PrincipalId(Uuid::nil()),
            originals.clone(),
        )
        .await;
        assert!(started.elapsed() < Duration::from_secs(3));
        assert_eq!(refreshed.len(), 100);
        assert_eq!(
            mock.requests
                .lock()
                .map_err(|_| "request lock poisoned")?
                .len(),
            LIVE_STATUS_CONCURRENCY
        );
        for (current, original) in refreshed.iter().zip(&originals) {
            assert_eq!(current.id, original.id);
            assert_stale(current, original);
        }
        Ok(())
    }

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
