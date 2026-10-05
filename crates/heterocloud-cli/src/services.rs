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

#[derive(Debug, Args)]
pub struct FlashArgs {
    #[command(subcommand)]
    pub command: FlashCommand,
}

#[derive(Debug, Subcommand)]
pub enum FlashCommand {
    /// Write or remove credentials for optional HTTP load balancer OIDC.
    LoadBalancerSecret {
        #[command(subcommand)]
        command: LoadBalancerSecretCommand,
    },
    /// Start a stopped service with its existing settings and persistent home.
    Start {
        #[arg(value_name = "SERVICE_ID")]
        id: Uuid,
        #[arg(long)]
        no_wait: bool,
    },
    /// Stop all replicas and retain the service, secrets and persistent home.
    Stop {
        #[arg(value_name = "SERVICE_ID")]
        id: Uuid,
        #[arg(long)]
        no_wait: bool,
    },
    #[command(flatten)]
    Service(ServiceCommand),
}

#[derive(Debug, Subcommand)]
pub enum LoadBalancerSecretCommand {
    /// Read the client secret from a file or stdin, never a command line argument.
    Set {
        id: Uuid,
        name: String,
        #[arg(short, long, default_value = "-", value_name = "PATH")]
        file: String,
    },
    /// Remove a detached client secret from both credential stores.
    Delete {
        id: Uuid,
        name: String,
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Debug, Args)]
pub struct IamArgs {
    #[command(subcommand)]
    pub command: IamCommand,
}
#[derive(Debug, Subcommand)]
pub enum IamCommand {
    /// Show the current user, service account, or workload identity.
    Whoami,
    ServiceAccounts {
        #[command(subcommand)]
        command: IamAccountCommand,
    },
    Policies {
        #[command(subcommand)]
        command: IamPolicyCommand,
    },
    Bindings {
        #[command(subcommand)]
        command: IamBindingCommand,
    },
    ApiKeys {
        #[command(subcommand)]
        command: IamApiKeyCommand,
    },
}
#[derive(Debug, Subcommand)]
pub enum IamAccountCommand {
    List,
    Create {
        #[arg(long)]
        name: String,
    },
    SetEnabled {
        id: Uuid,
        #[arg(long, action=clap::ArgAction::Set)]
        enabled: bool,
    },
}
#[derive(Debug, Subcommand)]
pub enum IamPolicyCommand {
    List,
    Create {
        #[arg(short, long, default_value = "-")]
        file: String,
    },
}
#[derive(Debug, Subcommand)]
pub enum IamBindingCommand {
    List,
    Create {
        #[arg(long)]
        principal_id: Uuid,
        #[arg(long)]
        policy_id: Uuid,
    },
    Delete {
        id: Uuid,
        #[arg(long)]
        yes: bool,
    },
}
#[derive(Debug, Subcommand)]
pub enum IamApiKeyCommand {
    List {
        principal_id: Uuid,
    },
    Create {
        principal_id: Uuid,
        #[arg(long)]
        name: String,
        #[arg(long, default_value_t=30, value_parser=clap::value_parser!(u16).range(1..=365))]
        expires_in_days: u16,
        /// Save the one-time response to a new private file; never print its key.
        #[arg(long)]
        output_file: PathBuf,
    },
    Revoke {
        principal_id: Uuid,
        id: Uuid,
        #[arg(long)]
        yes: bool,
    },
}

pub(crate) async fn execute_iam(args: IamArgs, settings: ApiSettings) -> Result<(), CliError> {
    let client = ApiClient::new(settings)?;
    let mut output_file = None;
    let (method, path, body) = match args.command {
        IamCommand::Whoami => (Method::GET, "api/v1/auth/identity".into(), None),
        IamCommand::ServiceAccounts { command } => match command {
            IamAccountCommand::List => (Method::GET, "iam/principals".into(), None),
            IamAccountCommand::Create { name } => (
                Method::POST,
                "iam/principals".into(),
                Some(json!({"name":name})),
            ),
            IamAccountCommand::SetEnabled { id, enabled } => (
                Method::PATCH,
                format!("iam/principals/{id}"),
                Some(json!({"enabled":enabled})),
            ),
        },
        IamCommand::Policies { command } => match command {
            IamPolicyCommand::List => (Method::GET, "iam/policies".into(), None),
            IamPolicyCommand::Create { file } => (
                Method::POST,
                "iam/policies".into(),
                Some(read_manifest::<Value>(&file)?),
            ),
        },
        IamCommand::Bindings { command } => match command {
            IamBindingCommand::List => (Method::GET, "iam/bindings".into(), None),
            IamBindingCommand::Create {
                principal_id,
                policy_id,
            } => (
                Method::POST,
                "iam/bindings".into(),
                Some(json!({"principal_id":principal_id,"policy_id":policy_id})),
            ),
            IamBindingCommand::Delete { id, yes } => {
                require_yes(yes)?;
                (Method::DELETE, format!("iam/bindings/{id}"), None)
            }
        },
        IamCommand::ApiKeys { command } => match command {
            IamApiKeyCommand::List { principal_id } => (
                Method::GET,
                format!("iam/principals/{principal_id}/api-keys"),
                None,
            ),
            IamApiKeyCommand::Create {
                principal_id,
                name,
                expires_in_days,
                output_file: file,
            } => {
                if file.exists() {
                    return Err(CliError::InvalidManifest(
                        "output file already exists".into(),
                    ));
                }
                output_file = Some(file);
                (
                    Method::POST,
                    format!("iam/principals/{principal_id}/api-keys"),
                    Some(json!({"name":name,"expires_in_days":expires_in_days})),
                )
            }
            IamApiKeyCommand::Revoke {
                principal_id,
                id,
                yes,
            } => {
                require_yes(yes)?;
                (
                    Method::DELETE,
                    format!("iam/principals/{principal_id}/api-keys/{id}"),
                    None,
                )
            }
        },
    };
    let path = if path.starts_with("api/") {
        path
    } else {
        format!("api/v1/organizations/{}/{path}", client.organization_id)
    };
    let url = client
        .endpoint
        .join(&path)
        .map_err(|e| CliError::InvalidApiEndpoint(e.to_string()))?;
    let value = client.send_json(method, url, body.as_ref()).await?;
    if let Some(path) = output_file {
        use std::io::Write;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| std::path::Path::new("."));
        let mut file =
            tempfile::NamedTempFile::new_in(parent).map_err(|source| CliError::InputFile {
                path: path.display().to_string(),
                source,
            })?;
        file.write_all(serde_json::to_string_pretty(&value)?.as_bytes())
            .map_err(|source| CliError::InputFile {
                path: path.display().to_string(),
                source,
            })?;
        file.as_file()
            .sync_all()
            .map_err(|source| CliError::InputFile {
                path: path.display().to_string(),
                source,
            })?;
        file.persist_noclobber(&path)
            .map_err(|e| CliError::InputFile {
                path: path.display().to_string(),
                source: e.error,
            })?;
        println!("{{\"saved\":true}}");
    } else {
        println!("{}", serde_json::to_string_pretty(&value)?);
    }
    Ok(())
}
fn require_yes(yes: bool) -> Result<(), CliError> {
    if yes {
        Ok(())
    } else {
        Err(CliError::InvalidManifest("operation requires --yes".into()))
    }
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
    Vpc,
    Syouyu,
}

impl ServiceKind {
    fn collection_path(self, organization_id: Uuid) -> String {
        let suffix = match self {
            Self::Flow => "realtime/services",
            Self::Flash => "flash/services",
            Self::Vpc => "vpc/networks",
            Self::Syouyu => "syouyu/buckets",
        };
        format!("api/v1/organizations/{organization_id}/{suffix}")
    }

    const fn provider(self) -> &'static str {
        match self {
            Self::Flow => "flow",
            Self::Flash => "flash",
            Self::Vpc => "vpc",
            Self::Syouyu => "syouyu",
        }
    }
}

pub(crate) struct ApiSettings {
    pub endpoint: String,
    pub bearer_token: String,
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
    workload: Option<crate::workload::WorkloadCredentials>,
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

pub(crate) async fn execute_flash(args: FlashArgs, settings: ApiSettings) -> Result<(), CliError> {
    let (id, stopped, no_wait) = match args.command {
        FlashCommand::LoadBalancerSecret { command } => {
            let client = ApiClient::new(settings)?;
            let (id, name, value) = match command {
                LoadBalancerSecretCommand::Set { id, name, file } => {
                    (id, name, Some(read_client_secret(&file)?))
                }
                LoadBalancerSecretCommand::Delete { id, name, yes } => {
                    if !yes {
                        return Err(CliError::InvalidManifest(
                            "secret delete requires --yes".into(),
                        ));
                    }
                    (id, name, None)
                }
            };
            if name.is_empty()
                || name.len() > 63
                || !name.as_bytes()[0].is_ascii_alphanumeric()
                || !name.as_bytes()[name.len() - 1].is_ascii_alphanumeric()
                || name
                    .bytes()
                    .any(|b| !b.is_ascii_lowercase() && !b.is_ascii_digit() && b != b'-')
            {
                return Err(CliError::InvalidManifest(
                    "client secret reference must be a lowercase DNS label".into(),
                ));
            }
            let url = client
                .endpoint
                .join(&format!(
                    "{}/{id}/load-balancer/secrets/{name}",
                    ServiceKind::Flash.collection_path(client.organization_id)
                ))
                .map_err(|e| CliError::InvalidApiEndpoint(e.to_string()))?;
            let (method, body) = match value {
                Some(value) => (Method::PUT, Some(json!({"value":value}))),
                None => (Method::DELETE, None),
            };
            client.send_json(method, url, body.as_ref()).await?;
            println!("{{\"updated\":true}}");
            return Ok(());
        }
        FlashCommand::Service(command) => {
            return execute(ServiceKind::Flash, ServiceArgs { command }, settings).await;
        }
        FlashCommand::Start { id, no_wait } => (id, false, no_wait),
        FlashCommand::Stop { id, no_wait } => (id, true, no_wait),
    };
    let output = settings.output;
    let client = ApiClient::new(settings)?;
    let service = client.set_flash_stopped(id, stopped).await?;
    ensure_provider(ServiceKind::Flash, &service)?;
    let service = if no_wait {
        service
    } else {
        client.wait_ready(ServiceKind::Flash, service.id).await?
    };
    write_service(&service, output)
}

fn read_client_secret(path: &str) -> Result<String, CliError> {
    let mut reader: Box<dyn Read> = if path == "-" {
        Box::new(io::stdin())
    } else {
        Box::new(fs::File::open(path).map_err(|source| CliError::InputFile {
            path: path.into(),
            source,
        })?)
    };
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take(16_385)
        .read_to_end(&mut bytes)
        .map_err(|source| CliError::InputFile {
            path: path.into(),
            source,
        })?;
    if bytes.is_empty() || bytes.len() > 16_384 || bytes.contains(&0) {
        return Err(CliError::InvalidManifest(
            "client secret must contain 1 to 16384 bytes without NUL".into(),
        ));
    }
    String::from_utf8(bytes)
        .map_err(|_| CliError::InvalidManifest("client secret must be UTF-8".into()))
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

        let mut headers = HeaderMap::new();
        let workload = if settings.bearer_token.is_empty() {
            Some(crate::workload::WorkloadCredentials::from_environment(
                &endpoint,
                settings.organization_id,
            )?)
        } else {
            let mut authorization =
                HeaderValue::from_str(&format!("Bearer {}", settings.bearer_token))
                    .map_err(|_| CliError::InvalidApiKey)?;
            authorization.set_sensitive(true);
            headers.insert(AUTHORIZATION, authorization);
            None
        };
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let http = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
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
            workload,
        })
    }

    async fn authorized_request(
        &self,
        method: Method,
        url: Url,
    ) -> Result<reqwest::RequestBuilder, CliError> {
        let request = self.http.request(method, url);
        if let Some(workload) = &self.workload {
            Ok(request.bearer_auth(workload.token(&self.http).await?))
        } else {
            Ok(request)
        }
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
            ServiceKind::Flash | ServiceKind::Syouyu | ServiceKind::Vpc => Method::PUT,
        };
        let value = self
            .send_json(method, self.service_url(kind, id)?, Some(manifest))
            .await?;
        serde_json::from_value(value).map_err(CliError::Json)
    }

    async fn set_flash_stopped(&self, id: Uuid, stopped: bool) -> Result<ManagedService, CliError> {
        let action = if stopped { "stop" } else { "start" };
        let url = self
            .endpoint
            .join(&format!(
                "{}/{id}/{action}",
                ServiceKind::Flash.collection_path(self.organization_id)
            ))
            .map_err(|error| CliError::InvalidApiEndpoint(error.to_string()))?;
        let value = self.send_json::<Value>(Method::POST, url, None).await?;
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
                .authorized_request(Method::GET, url.clone())
                .await?
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
        let mut request = self.authorized_request(method, url).await?;
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
                    service.id,
                    service.name,
                    display_state(service),
                    service.project_id,
                    service.updated_at
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
            println!("state\t{}", display_state(service));
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

fn display_state(service: &ManagedService) -> &str {
    if service.provider == "flash"
        && service.spec.get("stopped").and_then(Value::as_bool) == Some(true)
        && !matches!(service.state.as_str(), "error" | "deleting")
    {
        let status = service.status.get("status").unwrap_or(&service.status);
        if service.state == "ready" && status.get("stopped").and_then(Value::as_bool) == Some(true)
        {
            "stopped"
        } else {
            "stopping"
        }
    } else {
        &service.state
    }
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

    #[test]
    fn flash_lifecycle_commands_do_not_leak_to_other_providers() {
        use clap::Parser;
        for action in ["stop", "start"] {
            assert!(
                crate::Cli::try_parse_from([
                    "heterocloud",
                    "flash",
                    action,
                    "00000000-0000-0000-0000-000000000007",
                    "--no-wait"
                ])
                .is_ok()
            );
            assert!(
                crate::Cli::try_parse_from([
                    "heterocloud",
                    "flow",
                    action,
                    "00000000-0000-0000-0000-000000000007"
                ])
                .is_err()
            );
        }
        assert!(crate::Cli::try_parse_from(["heterocloud", "flash", "list"]).is_ok());
    }

    fn settings(endpoint: &str, allow_insecure_http: bool) -> ApiSettings {
        ApiSettings {
            endpoint: endpoint.into(),
            bearer_token: "hc_0123456789_secret".into(),
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
