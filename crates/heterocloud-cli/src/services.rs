use std::{
    fs,
    io::{self, Read},
    path::PathBuf,
    time::Duration,
};

use clap::{Args, Subcommand, ValueEnum};
use reqwest::{
    Client, Method, Response, StatusCode, Url,
    header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::time::{Instant, sleep, timeout_at};
use uuid::Uuid;

use crate::CliError;

const USER_AGENT: &str = concat!("heterocloud-cli/", env!("CARGO_PKG_VERSION"));
const API_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const API_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum ApiOutputFormat {
    #[default]
    Json,
    Table,
}

#[derive(Debug, Args)]
pub struct ServiceArgs {
    #[command(subcommand)]
    pub command: ServiceCommand,
}

#[derive(Debug, Subcommand)]
pub enum ServiceCommand {
    /// List service instances, optionally within one project.
    List {
        #[arg(long, value_name = "UUID")]
        project_id: Option<Uuid>,
    },
    /// Read one service instance.
    Get {
        #[arg(value_name = "SERVICE_ID")]
        id: Uuid,
    },
    /// Create a service from a JSON manifest or `-` for stdin.
    Create {
        #[arg(short, long, value_name = "PATH", default_value = "-")]
        file: String,
        /// Return after the API accepts the operation.
        #[arg(long)]
        no_wait: bool,
    },
    /// Replace a service name and specification from a JSON manifest or stdin.
    Update {
        #[arg(value_name = "SERVICE_ID")]
        id: Uuid,
        #[arg(short, long, value_name = "PATH", default_value = "-")]
        file: String,
        /// Return after the API accepts the operation.
        #[arg(long)]
        no_wait: bool,
    },
    /// Delete a service instance.
    Delete {
        #[arg(value_name = "SERVICE_ID")]
        id: Uuid,
        /// Confirm destructive deletion.
        #[arg(long)]
        yes: bool,
        /// Return after the API accepts the operation.
        #[arg(long)]
        no_wait: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ServiceKind {
    Flow,
    Flash,
    Syouyu,
}

impl ServiceKind {
    fn collection_path(self, organization_id: Uuid) -> String {
        let suffix = match self {
            Self::Flow => "realtime/services",
            Self::Flash => "flash/services",
            Self::Syouyu => "syouyu/buckets",
        };
        format!("api/v1/organizations/{organization_id}/{suffix}")
    }

    const fn provider(self) -> &'static str {
        match self {
            Self::Flow => "flow",
            Self::Flash => "flash",
            Self::Syouyu => "syouyu",
        }
    }
}

pub(crate) struct ApiSettings {
    pub endpoint: String,
    pub api_key: String,
    pub organization_id: Uuid,
    pub wait_timeout_seconds: u64,
    pub allow_insecure_http: bool,
    pub output: ApiOutputFormat,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CreateManifest {
    project_id: Uuid,
    name: String,
    spec: Value,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct UpdateManifest {
    name: String,
    spec: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ManagedService {
    id: Uuid,
    organization_id: Uuid,
    project_id: Uuid,
    provider: String,
    name: String,
    generation: i64,
    state: String,
    spec: Value,
    status: Value,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
struct ServiceCollection {
    items: Vec<ManagedService>,
}

#[derive(Debug, Deserialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Debug, Deserialize)]
struct ErrorBody {
    code: String,
    message: String,
}

struct ApiClient {
    http: Client,
    endpoint: Url,
    organization_id: Uuid,
    wait_timeout: Duration,
}

pub(crate) async fn execute(
    kind: ServiceKind,
    args: ServiceArgs,
    settings: ApiSettings,
) -> Result<(), CliError> {
    let output = settings.output;
    let client = ApiClient::new(settings)?;
    match args.command {
        ServiceCommand::List { project_id } => {
            let services = client.list(kind, project_id).await?;
            write_services(&services, output)
        }
        ServiceCommand::Get { id } => {
            let service = client.get(kind, id).await?.ok_or_else(|| not_found(id))?;
            ensure_provider(kind, &service)?;
            write_service(&service, output)
        }
        ServiceCommand::Create { file, no_wait } => {
            let manifest: CreateManifest = read_manifest(&file)?;
            validate_manifest(&manifest.name, &manifest.spec)?;
            let service = client.create(kind, &manifest).await?;
            ensure_provider(kind, &service)?;
            let service = if no_wait {
                service
            } else {
                client.wait_ready(kind, service.id).await?
            };
            write_service(&service, output)
        }
        ServiceCommand::Update { id, file, no_wait } => {
            let manifest: UpdateManifest = read_manifest(&file)?;
            validate_manifest(&manifest.name, &manifest.spec)?;
            let service = client.update(kind, id, &manifest).await?;
            ensure_provider(kind, &service)?;
            let service = if no_wait {
                service
            } else {
                client.wait_ready(kind, service.id).await?
            };
            write_service(&service, output)
        }
        ServiceCommand::Delete { id, yes, no_wait } => {
            if !yes {
                return Err(CliError::InvalidManifest(
                    "delete requires --yes to confirm the operation".into(),
                ));
            }
            let service = client.delete(kind, id).await?;
            ensure_provider(kind, &service)?;
            if no_wait {
                write_service(&service, output)
            } else {
                client.wait_deleted(kind, id).await?;
                write_deleted(id, output)
            }
        }
    }
}

impl ApiClient {
    fn new(settings: ApiSettings) -> Result<Self, CliError> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut endpoint = Url::parse(&settings.endpoint)
            .map_err(|error| CliError::InvalidApiEndpoint(error.to_string()))?;
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
            "http" if settings.allow_insecure_http => {}
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

        let authorization = HeaderValue::from_str(&format!("Bearer {}", settings.api_key))
            .map_err(|_| CliError::InvalidApiKey)?;
        let mut authorization = authorization;
        authorization.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, authorization);
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let http = Client::builder()
            .default_headers(headers)
            .user_agent(USER_AGENT)
            .connect_timeout(API_CONNECT_TIMEOUT)
            .timeout(API_REQUEST_TIMEOUT)
            .build()
            .map_err(CliError::ApiTransport)?;
        Ok(Self {
            http,
            endpoint,
            organization_id: settings.organization_id,
            wait_timeout: Duration::from_secs(settings.wait_timeout_seconds),
        })
    }

    fn collection_url(&self, kind: ServiceKind) -> Result<Url, CliError> {
        self.endpoint
            .join(&kind.collection_path(self.organization_id))
            .map_err(|error| CliError::InvalidApiEndpoint(error.to_string()))
    }

    fn service_url(&self, kind: ServiceKind, id: Uuid) -> Result<Url, CliError> {
        self.endpoint
            .join(&format!(
                "{}/{id}",
                kind.collection_path(self.organization_id)
            ))
            .map_err(|error| CliError::InvalidApiEndpoint(error.to_string()))
    }

    async fn list(
        &self,
        kind: ServiceKind,
        project_id: Option<Uuid>,
    ) -> Result<Vec<ManagedService>, CliError> {
        let mut url = self.collection_url(kind)?;
        if let Some(project_id) = project_id {
            url.query_pairs_mut()
                .append_pair("project_id", &project_id.to_string());
        }
        let value =
            self.get_json_with_retry(url, false)
                .await?
                .ok_or_else(|| CliError::ApiResponse {
                    status: 404,
                    code: "not_found".into(),
                    message: "service collection was not found".into(),
                })?;
        serde_json::from_value::<ServiceCollection>(value)
            .map(|collection| collection.items)
            .map_err(CliError::Json)
    }

    async fn get(&self, kind: ServiceKind, id: Uuid) -> Result<Option<ManagedService>, CliError> {
        let value = self
            .get_json_with_retry(self.service_url(kind, id)?, true)
            .await?;
        value
            .map(serde_json::from_value)
            .transpose()
            .map_err(CliError::Json)
    }

    async fn create(
        &self,
        kind: ServiceKind,
        manifest: &CreateManifest,
    ) -> Result<ManagedService, CliError> {
        let value = self
            .send_json(Method::POST, self.collection_url(kind)?, Some(manifest))
            .await?;
        serde_json::from_value(value).map_err(CliError::Json)
    }

    async fn update(
        &self,
        kind: ServiceKind,
        id: Uuid,
        manifest: &UpdateManifest,
    ) -> Result<ManagedService, CliError> {
        let method = match kind {
            ServiceKind::Flow => Method::PATCH,
            ServiceKind::Flash | ServiceKind::Syouyu => Method::PUT,
        };
        let value = self
            .send_json(method, self.service_url(kind, id)?, Some(manifest))
            .await?;
        serde_json::from_value(value).map_err(CliError::Json)
    }

    async fn delete(&self, kind: ServiceKind, id: Uuid) -> Result<ManagedService, CliError> {
        let value = self
            .send_json::<Value>(Method::DELETE, self.service_url(kind, id)?, None)
            .await?;
        serde_json::from_value(value).map_err(CliError::Json)
    }

    async fn wait_ready(&self, kind: ServiceKind, id: Uuid) -> Result<ManagedService, CliError> {
        let deadline = Instant::now() + self.wait_timeout;
        loop {
            match self.get_before_deadline(kind, id, deadline).await {
                Ok(Some(service)) if service.state == "ready" => return Ok(service),
                Ok(Some(service)) if service.state == "error" => {
                    return Err(CliError::ServiceFailed {
                        id,
                        detail: compact_json(&service.status),
                    });
                }
                Ok(Some(_)) => {}
                Ok(None) => return Err(not_found(id)),
                Err(error) if is_transient(&error) => {}
                Err(error) => return Err(error),
            }
            if Instant::now() >= deadline {
                return Err(CliError::ServiceTimeout {
                    id,
                    seconds: self.wait_timeout.as_secs(),
                });
            }
            sleep(POLL_INTERVAL.min(deadline.saturating_duration_since(Instant::now()))).await;
        }
    }

    async fn wait_deleted(&self, kind: ServiceKind, id: Uuid) -> Result<(), CliError> {
        let deadline = Instant::now() + self.wait_timeout;
        loop {
            match self.get_before_deadline(kind, id, deadline).await {
                Ok(None) => return Ok(()),
                Ok(Some(service)) if service.state == "error" => {
                    return Err(CliError::ServiceFailed {
                        id,
                        detail: compact_json(&service.status),
                    });
                }
                Ok(Some(_)) => {}
                Err(error) if is_transient(&error) => {}
                Err(error) => return Err(error),
            }
            if Instant::now() >= deadline {
                return Err(CliError::ServiceTimeout {
                    id,
                    seconds: self.wait_timeout.as_secs(),
                });
            }
            sleep(POLL_INTERVAL.min(deadline.saturating_duration_since(Instant::now()))).await;
        }
    }

    async fn get_before_deadline(
        &self,
        kind: ServiceKind,
        id: Uuid,
        deadline: Instant,
    ) -> Result<Option<ManagedService>, CliError> {
        let expired = || CliError::ServiceTimeout {
            id,
            seconds: self.wait_timeout.as_secs(),
        };
        if Instant::now() >= deadline {
            return Err(expired());
        }
        // Bound the whole read, including HTTP retries, by the operation deadline.
        timeout_at(deadline, self.get(kind, id))
            .await
            .map_err(|_| expired())?
    }

    async fn get_json_with_retry(
        &self,
        url: Url,
        allow_not_found: bool,
    ) -> Result<Option<Value>, CliError> {
        let mut attempts = 0_u8;
        loop {
            attempts += 1;
            let response = self
                .http
                .get(url.clone())
                .send()
                .await
                .map_err(CliError::ApiTransport);
            match response {
                Ok(response) if allow_not_found && response.status() == StatusCode::NOT_FOUND => {
                    return Ok(None);
                }
                Ok(response) => match decode_response(response).await {
                    Ok(value) => return Ok(Some(value)),
                    Err(error) if attempts < 3 && is_transient(&error) => {}
                    Err(error) => return Err(error),
                },
                Err(error) if attempts < 3 => {
                    let _ = error;
                }
                Err(error) => return Err(error),
            }
            sleep(Duration::from_millis(250 * u64::from(attempts))).await;
        }
    }

    async fn send_json<T: Serialize + ?Sized>(
        &self,
        method: Method,
        url: Url,
        body: Option<&T>,
    ) -> Result<Value, CliError> {
        let mut request = self.http.request(method, url);
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().await.map_err(CliError::ApiTransport)?;
        decode_response(response).await
    }
}

async fn decode_response(response: Response) -> Result<Value, CliError> {
    let status = response.status();
    let bytes = response.bytes().await.map_err(CliError::ApiTransport)?;
    if status.is_success() {
        if bytes.is_empty() {
            return Ok(Value::Null);
        }
        return serde_json::from_slice(&bytes).map_err(CliError::Json);
    }
    let (code, message) = serde_json::from_slice::<ErrorEnvelope>(&bytes)
        .map(|envelope| (envelope.error.code, envelope.error.message))
        .unwrap_or_else(|_| {
            let body = String::from_utf8_lossy(&bytes);
            let message = body.trim().chars().take(512).collect::<String>();
            (
                "unexpected_response".to_owned(),
                if message.is_empty() {
                    status
                        .canonical_reason()
                        .unwrap_or("request failed")
                        .to_owned()
                } else {
                    message
                },
            )
        });
    Err(CliError::ApiResponse {
        status: status.as_u16(),
        code,
        message,
    })
}

fn read_manifest<T>(path: &str) -> Result<T, CliError>
where
    T: for<'de> Deserialize<'de>,
{
    let mut input = String::new();
    if path == "-" {
        io::stdin()
            .read_to_string(&mut input)
            .map_err(|source| CliError::InputFile {
                path: "stdin".into(),
                source,
            })?;
    } else {
        input = fs::read_to_string(PathBuf::from(path)).map_err(|source| CliError::InputFile {
            path: path.to_owned(),
            source,
        })?;
    }
    serde_json::from_str(&input).map_err(|error| CliError::InvalidManifest(error.to_string()))
}

fn validate_manifest(name: &str, spec: &Value) -> Result<(), CliError> {
    if name.trim() != name || name.is_empty() || name.len() > 128 {
        return Err(CliError::InvalidManifest(
            "name must contain 1 to 128 trimmed characters".into(),
        ));
    }
    if !spec.is_object() {
        return Err(CliError::InvalidManifest(
            "spec must be a JSON object".into(),
        ));
    }
    Ok(())
}

fn ensure_provider(kind: ServiceKind, service: &ManagedService) -> Result<(), CliError> {
    if service.provider == kind.provider() {
        return Ok(());
    }
    Err(CliError::ApiResponse {
        status: 409,
        code: "provider_mismatch".into(),
        message: format!(
            "service {} belongs to provider {}, not {}",
            service.id,
            service.provider,
            kind.provider()
        ),
    })
}

fn write_services(services: &[ManagedService], format: ApiOutputFormat) -> Result<(), CliError> {
    match format {
        ApiOutputFormat::Json => println!("{}", serde_json::to_string_pretty(services)?),
        ApiOutputFormat::Table => {
            println!("ID\tNAME\tSTATE\tPROJECT\tUPDATED");
            for service in services {
                println!(
                    "{}\t{}\t{}\t{}\t{}",
                    service.id, service.name, service.state, service.project_id, service.updated_at
                );
            }
        }
    }
    Ok(())
}

fn write_service(service: &ManagedService, format: ApiOutputFormat) -> Result<(), CliError> {
    match format {
        ApiOutputFormat::Json => println!("{}", serde_json::to_string_pretty(service)?),
        ApiOutputFormat::Table => {
            println!("FIELD\tVALUE");
            println!("id\t{}", service.id);
            println!("name\t{}", service.name);
            println!("provider\t{}", service.provider);
            println!("state\t{}", service.state);
            println!("project_id\t{}", service.project_id);
            println!("generation\t{}", service.generation);
            println!("spec\t{}", compact_json(&service.spec));
            println!("status\t{}", compact_json(&service.status));
            println!("updated_at\t{}", service.updated_at);
        }
    }
    Ok(())
}

fn write_deleted(id: Uuid, format: ApiOutputFormat) -> Result<(), CliError> {
    match format {
        ApiOutputFormat::Json => println!(
            "{}",
            serde_json::to_string_pretty(&json!({"id": id, "deleted": true}))?
        ),
        ApiOutputFormat::Table => {
            println!("ID\tDELETED");
            println!("{id}\ttrue");
        }
    }
    Ok(())
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".into())
}

fn not_found(id: Uuid) -> CliError {
    CliError::ApiResponse {
        status: 404,
        code: "not_found".into(),
        message: format!("service {id} was not found"),
    }
}

fn is_transient(error: &CliError) -> bool {
    matches!(
        error,
        CliError::ApiTransport(_)
            | CliError::ApiResponse {
                status: 429 | 502 | 503 | 504,
                ..
            }
    )
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read as _, Write as _},
        net::TcpListener,
        thread,
    };

    use super::*;

    fn settings(endpoint: &str, allow_insecure_http: bool) -> ApiSettings {
        ApiSettings {
            endpoint: endpoint.into(),
            api_key: "hc_0123456789_secret".into(),
            organization_id: Uuid::nil(),
            wait_timeout_seconds: 30,
            allow_insecure_http,
            output: ApiOutputFormat::Json,
        }
    }

    #[test]
    fn rejects_plain_http_without_explicit_opt_in() {
        assert!(matches!(
            ApiClient::new(settings("http://127.0.0.1:8080", false)),
            Err(CliError::InvalidApiEndpoint(_))
        ));
        assert!(ApiClient::new(settings("http://127.0.0.1:8080", true)).is_ok());
    }

    #[test]
    fn builds_stable_service_paths() -> Result<(), Box<dyn std::error::Error>> {
        let client = ApiClient::new(settings("https://cloud.example.test", false))?;
        let id = Uuid::parse_str("0198a118-073f-79e4-9ca4-0c1c2501c031")?;
        assert_eq!(
            client.service_url(ServiceKind::Flash, id)?.as_str(),
            "https://cloud.example.test/api/v1/organizations/00000000-0000-0000-0000-000000000000/flash/services/0198a118-073f-79e4-9ca4-0c1c2501c031"
        );
        assert_eq!(
            client.collection_url(ServiceKind::Flow)?.as_str(),
            "https://cloud.example.test/api/v1/organizations/00000000-0000-0000-0000-000000000000/realtime/services"
        );
        assert_eq!(
            client.collection_url(ServiceKind::Syouyu)?.as_str(),
            "https://cloud.example.test/api/v1/organizations/00000000-0000-0000-0000-000000000000/syouyu/buckets"
        );
        Ok(())
    }

    #[test]
    fn validates_manifest_shape() {
        assert!(validate_manifest("game", &json!({"region": "heteronet-global"})).is_ok());
        assert!(validate_manifest(" game", &json!({})).is_err());
        assert!(validate_manifest("game", &json!([])).is_err());
    }

    #[test]
    fn flash_manifests_preserve_fixed_and_autoscaling_specs()
    -> Result<(), Box<dyn std::error::Error>> {
        for source in [
            include_str!("../../../examples/cli/flash.json"),
            include_str!("../../../examples/cli/flash-autoscaling.json"),
            include_str!("../../../examples/cli/flash-web.json"),
        ] {
            let original: Value = serde_json::from_str(source)?;
            let create: CreateManifest = serde_json::from_str(source)?;
            validate_manifest(&create.name, &create.spec)?;
            assert_eq!(serde_json::to_value(&create)?, original);
            let update = UpdateManifest {
                name: create.name,
                spec: create.spec,
            };
            assert_eq!(serde_json::to_value(update)?["spec"], original["spec"]);
        }
        Ok(())
    }

    #[tokio::test]
    async fn sends_service_account_authorization_and_project_filter()
    -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let server = thread::spawn(move || -> io::Result<String> {
            let (mut stream, _) = listener.accept()?;
            stream.set_read_timeout(Some(Duration::from_secs(5)))?;
            let mut buffer = vec![0_u8; 16 * 1024];
            let length = stream.read(&mut buffer)?;
            let request = String::from_utf8_lossy(&buffer[..length]).into_owned();
            let body = r#"{"items":[{"id":"0198a118-073f-79e4-9ca4-0c1c2501c031","organization_id":"00000000-0000-0000-0000-000000000000","project_id":"0198a118-073f-79e4-9ca4-0c1c2501c031","provider":"flow","name":"conference","generation":1,"state":"ready","spec":{},"status":{},"created_at":"2026-09-06T00:00:00Z","updated_at":"2026-09-06T00:00:00Z"}]}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )?;
            Ok(request)
        });

        let client = ApiClient::new(settings(&format!("http://{address}"), true))?;
        let project_id = Uuid::parse_str("0198a118-073f-79e4-9ca4-0c1c2501c031")?;
        let services = client.list(ServiceKind::Flow, Some(project_id)).await?;
        let request = server.join().map_err(|_| "mock server panicked")??;

        assert_eq!(services.len(), 1);
        assert!(request.starts_with(&format!(
            "GET /api/v1/organizations/00000000-0000-0000-0000-000000000000/realtime/services?project_id={project_id} HTTP/1.1"
        )));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer hc_0123456789_secret\r\n")
        );
        Ok(())
    }
}
