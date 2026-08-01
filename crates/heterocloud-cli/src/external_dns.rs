use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use clap::Args;
use serde_json::{Value, json};
use zeroize::Zeroizing;

use super::{
    CliError, DnsSourceArgs, build_records, check_records_with, resolve_source, system_lookup,
};

const EXTERNAL_DNS_REPOSITORY_NAME: &str = "external-dns";
const EXTERNAL_DNS_REPOSITORY_URL: &str = "https://kubernetes-sigs.github.io/external-dns/";
const EXTERNAL_DNS_CHART: &str = "external-dns/external-dns";
const DEFAULT_CHART_VERSION: &str = "1.20.0";
const MAX_CREDENTIAL_BYTES: u64 = 65_536;
const DNS_POLL_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Args)]
pub struct ReconcileArgs {
    #[command(flatten)]
    pub source: DnsSourceArgs,

    /// ExternalDNS provider name, such as cloudflare, aws, google, rfc2136, or webhook.
    #[arg(long)]
    pub provider: String,

    /// Map a UTF-8 token file into a provider environment variable, for example CF_API_TOKEN=/secure/token.
    #[arg(long = "credential-file", value_name = "ENV=PATH")]
    pub credential_files: Vec<String>,

    /// Map an existing Kubernetes Secret key into a provider environment variable.
    #[arg(long = "credential-secret", value_name = "ENV=SECRET:KEY")]
    pub credential_secrets: Vec<String>,

    /// Additional ExternalDNS provider argument, including the leading `--`.
    #[arg(
        long = "provider-arg",
        value_name = "--NAME=VALUE",
        allow_hyphen_values = true
    )]
    pub provider_args: Vec<String>,

    /// Additional non-secret Helm values for a webhook or provider integration.
    #[arg(long = "provider-values", value_name = "PATH")]
    pub provider_values: Vec<PathBuf>,

    /// Namespace for ExternalDNS and the managed DNSEndpoint.
    #[arg(long, default_value = "heterocloud-dns")]
    pub controller_namespace: String,

    /// Helm release and ExternalDNS Deployment name.
    #[arg(long, default_value = "heterocloud-dns")]
    pub controller_release: String,

    /// Secret created from --credential-file entries.
    #[arg(long, default_value = "heterocloud-dns-provider")]
    pub credential_secret_name: String,

    /// Name of the provider-neutral DNSEndpoint resource.
    #[arg(long, default_value = "heterocloud-public")]
    pub endpoint_name: String,

    /// ExternalDNS Helm chart version.
    #[arg(long, default_value = DEFAULT_CHART_VERSION)]
    pub chart_version: String,

    /// ExternalDNS TXT ownership identifier.
    #[arg(long)]
    pub txt_owner_id: Option<String>,

    /// DNS TTL in seconds.
    #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u32).range(30..=86_400))]
    pub ttl: u32,

    /// Timeout for Helm rollout and DNS convergence.
    #[arg(long, default_value_t = 300, value_parser = clap::value_parser!(u64).range(30..=3_600))]
    pub timeout_seconds: u64,

    /// Apply resources but do not wait for public DNS convergence.
    #[arg(long)]
    pub no_wait_dns: bool,

    /// Print the sanitized controller values and DNSEndpoint without changing the cluster.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LocalCredential {
    environment: String,
    path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SecretCredential {
    environment: String,
    secret: String,
    key: String,
}

struct ReconcilePlan {
    domain: String,
    records: Vec<super::DnsRecord>,
    local_credentials: Vec<LocalCredential>,
    helm_values: Value,
    endpoint: Value,
}

pub fn reconcile(args: ReconcileArgs) -> Result<(), CliError> {
    let plan = build_plan(&args)?;
    if args.dry_run {
        print_dry_run(&args, &plan)?;
        return Ok(());
    }

    println!(
        "Reconciling {} A records for {} through ExternalDNS provider `{}`.",
        plan.records.len(),
        plan.domain,
        args.provider
    );
    apply_namespace(&args)?;
    if !plan.local_credentials.is_empty() {
        apply_local_credentials(&args, &plan.local_credentials)?;
    }
    install_external_dns(&args, &plan.helm_values)?;
    apply_manifest(&args.source, &plan.endpoint)?;
    restart_controller(&args)?;

    if args.no_wait_dns {
        println!(
            "DNSEndpoint applied. Run `heterocloud dns verify --domain {}` after propagation.",
            plan.domain
        );
        return Ok(());
    }
    wait_for_dns(&plan.records, args.timeout_seconds)?;
    println!("ExternalDNS converged all records for {}.", plan.domain);
    Ok(())
}

fn build_plan(args: &ReconcileArgs) -> Result<ReconcilePlan, CliError> {
    validate_provider(&args.provider)?;
    validate_kubernetes_name(&args.controller_namespace, "controller namespace")?;
    validate_kubernetes_name(&args.controller_release, "controller release")?;
    validate_kubernetes_name(&args.credential_secret_name, "credential Secret")?;
    validate_kubernetes_name(&args.endpoint_name, "DNSEndpoint name")?;
    validate_chart_version(&args.chart_version)?;
    validate_provider_args(&args.provider_args)?;
    validate_values_files(&args.provider_values)?;

    let (domain, addresses) = resolve_source(&args.source)?;
    let records = build_records(&domain, &addresses, args.ttl);
    let local_credentials = parse_local_credentials(&args.credential_files)?;
    let secret_credentials = parse_secret_credentials(&args.credential_secrets)?;
    ensure_unique_environments(&local_credentials, &secret_credentials)?;
    let owner = args
        .txt_owner_id
        .clone()
        .unwrap_or_else(|| format!("heterocloud-{domain}"));
    if owner.is_empty() || owner.len() > 255 || owner.chars().any(char::is_control) {
        return Err(CliError::InvalidValue {
            kind: "TXT owner ID",
            value: owner,
        });
    }

    let environment = provider_environment(
        &local_credentials,
        &secret_credentials,
        &args.credential_secret_name,
    );
    let helm_values = json!({
        "fullnameOverride": args.controller_release,
        "provider": {"name": args.provider},
        "sources": ["crd"],
        "policy": "sync",
        "registry": "txt",
        "txtOwnerId": owner,
        "txtPrefix": "_heterocloud-",
        "domainFilters": [domain],
        "labelFilter": "app.kubernetes.io/managed-by=heterocloud",
        "managedRecordTypes": ["A"],
        "interval": "30s",
        "triggerLoopOnEvent": true,
        "logFormat": "json",
        "env": environment,
        "extraArgs": args.provider_args,
        "resources": {
            "requests": {"cpu": "25m", "memory": "64Mi"},
            "limits": {"memory": "192Mi"}
        }
    });
    let endpoints = records
        .iter()
        .map(|record| {
            json!({
                "dnsName": record.name,
                "recordTTL": record.ttl,
                "recordType": record.record_type,
                "targets": [record.value.to_string()]
            })
        })
        .collect::<Vec<_>>();
    let endpoint = json!({
        "apiVersion": "externaldns.k8s.io/v1alpha1",
        "kind": "DNSEndpoint",
        "metadata": {
            "name": args.endpoint_name,
            "namespace": args.controller_namespace,
            "labels": {
                "app.kubernetes.io/name": "heterocloud-public-dns",
                "app.kubernetes.io/part-of": "heterocloud",
                "app.kubernetes.io/managed-by": "heterocloud"
            }
        },
        "spec": {"endpoints": endpoints}
    });

    Ok(ReconcilePlan {
        domain,
        records,
        local_credentials,
        helm_values,
        endpoint,
    })
}

fn provider_environment(
    local: &[LocalCredential],
    existing: &[SecretCredential],
    generated_secret: &str,
) -> Vec<Value> {
    let mut environment = local
        .iter()
        .map(|credential| {
            json!({
                "name": credential.environment,
                "valueFrom": {"secretKeyRef": {
                    "name": generated_secret,
                    "key": credential.environment
                }}
            })
        })
        .chain(existing.iter().map(|credential| {
            json!({
                "name": credential.environment,
                "valueFrom": {"secretKeyRef": {
                    "name": credential.secret,
                    "key": credential.key
                }}
            })
        }))
        .collect::<Vec<_>>();
    environment.sort_by(|left, right| left["name"].as_str().cmp(&right["name"].as_str()));
    environment
}

fn parse_local_credentials(values: &[String]) -> Result<Vec<LocalCredential>, CliError> {
    let mut credentials = Vec::with_capacity(values.len());
    for value in values {
        let Some((environment, path)) = value.split_once('=') else {
            return Err(CliError::InvalidCredential(value.clone()));
        };
        validate_environment_name(environment)?;
        if path.is_empty() {
            return Err(CliError::InvalidCredential(value.clone()));
        }
        let path = canonical_credential_path(Path::new(path))?;
        validate_credential_content(&path)?;
        credentials.push(LocalCredential {
            environment: environment.to_owned(),
            path,
        });
    }
    credentials.sort_by(|left, right| left.environment.cmp(&right.environment));
    Ok(credentials)
}

fn parse_secret_credentials(values: &[String]) -> Result<Vec<SecretCredential>, CliError> {
    let mut credentials = Vec::with_capacity(values.len());
    for value in values {
        let Some((environment, source)) = value.split_once('=') else {
            return Err(CliError::InvalidCredential(value.clone()));
        };
        let Some((secret, key)) = source.split_once(':') else {
            return Err(CliError::InvalidCredential(value.clone()));
        };
        validate_environment_name(environment)?;
        validate_kubernetes_name(secret, "credential Secret")?;
        validate_secret_key(key)?;
        credentials.push(SecretCredential {
            environment: environment.to_owned(),
            secret: secret.to_owned(),
            key: key.to_owned(),
        });
    }
    credentials.sort_by(|left, right| left.environment.cmp(&right.environment));
    Ok(credentials)
}

fn ensure_unique_environments(
    local: &[LocalCredential],
    existing: &[SecretCredential],
) -> Result<(), CliError> {
    let mut names = BTreeSet::new();
    for environment in local
        .iter()
        .map(|credential| &credential.environment)
        .chain(existing.iter().map(|credential| &credential.environment))
    {
        if !names.insert(environment) {
            return Err(CliError::InvalidValue {
                kind: "duplicate credential environment variable",
                value: environment.clone(),
            });
        }
    }
    Ok(())
}

fn canonical_credential_path(path: &Path) -> Result<PathBuf, CliError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| CliError::CredentialFile {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(CliError::UnsafeCredentialFile(path.to_path_buf()));
    }
    #[cfg(unix)]
    {
        let mode = metadata.permissions().mode() & 0o777;
        if !matches!(mode, 0o400 | 0o600) {
            return Err(CliError::UnsafeCredentialMode {
                path: path.to_path_buf(),
                mode,
            });
        }
    }
    fs::canonicalize(path).map_err(|source| CliError::CredentialFile {
        path: path.to_path_buf(),
        source,
    })
}

fn validate_credential_content(path: &Path) -> Result<(), CliError> {
    let bytes = Zeroizing::new(fs::read(path).map_err(|source| CliError::CredentialFile {
        path: path.to_path_buf(),
        source,
    })?);
    if bytes.is_empty()
        || bytes.len() as u64 > MAX_CREDENTIAL_BYTES
        || bytes.iter().any(|byte| matches!(byte, 0 | b'\n' | b'\r'))
        || std::str::from_utf8(&bytes).is_err()
    {
        return Err(CliError::InvalidCredentialContent(path.to_path_buf()));
    }
    Ok(())
}

fn validate_provider(provider: &str) -> Result<(), CliError> {
    if provider.is_empty()
        || provider.len() > 63
        || provider.starts_with('-')
        || provider.ends_with('-')
        || !provider
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(CliError::InvalidValue {
            kind: "ExternalDNS provider",
            value: provider.to_owned(),
        });
    }
    Ok(())
}

fn validate_kubernetes_name(value: &str, kind: &'static str) -> Result<(), CliError> {
    if value.is_empty()
        || value.len() > 63
        || value.starts_with('-')
        || value.ends_with('-')
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(CliError::InvalidValue {
            kind,
            value: value.to_owned(),
        });
    }
    Ok(())
}

fn validate_environment_name(value: &str) -> Result<(), CliError> {
    let mut bytes = value.bytes();
    let valid_first = bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_');
    if !valid_first || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_') {
        return Err(CliError::InvalidValue {
            kind: "credential environment variable",
            value: value.to_owned(),
        });
    }
    Ok(())
}

fn validate_secret_key(value: &str) -> Result<(), CliError> {
    if value.is_empty()
        || value.len() > 253
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err(CliError::InvalidValue {
            kind: "Secret data key",
            value: value.to_owned(),
        });
    }
    Ok(())
}

fn validate_chart_version(value: &str) -> Result<(), CliError> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
    {
        return Err(CliError::InvalidValue {
            kind: "ExternalDNS chart version",
            value: value.to_owned(),
        });
    }
    Ok(())
}

fn validate_provider_args(values: &[String]) -> Result<(), CliError> {
    for value in values {
        if !value.starts_with("--")
            || value.len() < 3
            || value.len() > 4_096
            || value.chars().any(char::is_control)
        {
            return Err(CliError::InvalidValue {
                kind: "ExternalDNS provider argument",
                value: value.clone(),
            });
        }
    }
    Ok(())
}

fn validate_values_files(paths: &[PathBuf]) -> Result<(), CliError> {
    for path in paths {
        let metadata = fs::metadata(path).map_err(|source| CliError::CredentialFile {
            path: path.clone(),
            source,
        })?;
        if !metadata.is_file() {
            return Err(CliError::InvalidValue {
                kind: "provider values file",
                value: path.display().to_string(),
            });
        }
    }
    Ok(())
}

fn print_dry_run(args: &ReconcileArgs, plan: &ReconcilePlan) -> Result<(), CliError> {
    let credentials = plan
        .helm_values
        .get("env")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let output = json!({
        "controller": {
            "chart": EXTERNAL_DNS_CHART,
            "chartVersion": args.chart_version,
            "namespace": args.controller_namespace,
            "release": args.controller_release,
            "provider": args.provider,
            "credentials": credentials,
            "values": plan.helm_values
        },
        "dnsEndpoint": plan.endpoint
    });
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn apply_namespace(args: &ReconcileArgs) -> Result<(), CliError> {
    let manifest = json!({
        "apiVersion": "v1",
        "kind": "Namespace",
        "metadata": {
            "name": args.controller_namespace,
            "labels": {
                "app.kubernetes.io/part-of": "heterocloud",
                "app.kubernetes.io/managed-by": "heterocloud"
            }
        }
    });
    apply_manifest(&args.source, &manifest)
}

fn apply_local_credentials(
    args: &ReconcileArgs,
    credentials: &[LocalCredential],
) -> Result<(), CliError> {
    let mut create_args = kubectl_prefix(&args.source);
    create_args.extend([
        OsString::from("create"),
        OsString::from("secret"),
        OsString::from("generic"),
        OsString::from(&args.credential_secret_name),
        OsString::from("--namespace"),
        OsString::from(&args.controller_namespace),
        OsString::from("--dry-run=client"),
        OsString::from("--output=json"),
    ]);
    for credential in credentials {
        create_args.push(OsString::from(format!(
            "--from-file={}={}",
            credential.environment,
            credential.path.display()
        )));
    }

    let mut creator = Command::new("kubectl");
    creator
        .args(&create_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut creator = creator.spawn().map_err(|source| CliError::CommandStart {
        program: "kubectl",
        source,
    })?;
    let Some(secret_output) = creator.stdout.take() else {
        return Err(CliError::CommandStart {
            program: "kubectl",
            source: io::Error::other("secret renderer stdout was not piped"),
        });
    };

    let mut apply_args = kubectl_prefix(&args.source);
    apply_args.extend([
        OsString::from("apply"),
        OsString::from("--server-side=true"),
        OsString::from("--field-manager=heterocloud-cli"),
        OsString::from("--filename=-"),
    ]);
    let apply_output = Command::new("kubectl")
        .args(&apply_args)
        .stdin(Stdio::from(secret_output))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|source| CliError::CommandStart {
            program: "kubectl",
            source,
        })?;
    let creator_output = creator
        .wait_with_output()
        .map_err(|source| CliError::CommandStart {
            program: "kubectl",
            source,
        })?;
    ensure_success("kubectl", creator_output)?;
    ensure_success("kubectl", apply_output)?;
    println!(
        "Updated provider credential Secret {}/{}.",
        args.controller_namespace, args.credential_secret_name
    );
    Ok(())
}

fn install_external_dns(args: &ReconcileArgs, values: &Value) -> Result<(), CliError> {
    run_command(
        "helm",
        &[
            OsString::from("repo"),
            OsString::from("add"),
            OsString::from(EXTERNAL_DNS_REPOSITORY_NAME),
            OsString::from(EXTERNAL_DNS_REPOSITORY_URL),
            OsString::from("--force-update"),
        ],
        None,
    )?;

    let mut helm_args = vec![
        OsString::from("upgrade"),
        OsString::from("--install"),
        OsString::from(&args.controller_release),
        OsString::from(EXTERNAL_DNS_CHART),
        OsString::from("--version"),
        OsString::from(&args.chart_version),
        OsString::from("--namespace"),
        OsString::from(&args.controller_namespace),
        OsString::from("--atomic"),
        OsString::from("--timeout"),
        OsString::from(format!("{}s", args.timeout_seconds)),
    ];
    if let Some(kubeconfig) = &args.source.kubeconfig {
        helm_args.push(OsString::from("--kubeconfig"));
        helm_args.push(kubeconfig.as_os_str().to_owned());
    }
    if let Some(context) = &args.source.context {
        helm_args.push(OsString::from("--kube-context"));
        helm_args.push(OsString::from(context));
    }
    for path in &args.provider_values {
        helm_args.push(OsString::from("--values"));
        helm_args.push(path.as_os_str().to_owned());
    }
    helm_args.extend([OsString::from("--values"), OsString::from("-")]);
    let input = serde_json::to_vec(values)?;
    run_command("helm", &helm_args, Some(&input))?;
    println!(
        "ExternalDNS {} installed with provider `{}`.",
        args.chart_version, args.provider
    );
    Ok(())
}

fn restart_controller(args: &ReconcileArgs) -> Result<(), CliError> {
    let mut restart_args = kubectl_prefix(&args.source);
    restart_args.extend([
        OsString::from("rollout"),
        OsString::from("restart"),
        OsString::from(format!("deployment/{}", args.controller_release)),
        OsString::from("--namespace"),
        OsString::from(&args.controller_namespace),
    ]);
    run_command("kubectl", &restart_args, None)?;

    let mut status_args = kubectl_prefix(&args.source);
    status_args.extend([
        OsString::from("rollout"),
        OsString::from("status"),
        OsString::from(format!("deployment/{}", args.controller_release)),
        OsString::from("--namespace"),
        OsString::from(&args.controller_namespace),
        OsString::from(format!("--timeout={}s", args.timeout_seconds)),
    ]);
    run_command("kubectl", &status_args, None)?;
    Ok(())
}

fn apply_manifest(source: &DnsSourceArgs, manifest: &Value) -> Result<(), CliError> {
    let mut args = kubectl_prefix(source);
    args.extend([
        OsString::from("apply"),
        OsString::from("--server-side=true"),
        OsString::from("--field-manager=heterocloud-cli"),
        OsString::from("--filename=-"),
    ]);
    let input = serde_json::to_vec(manifest)?;
    run_command("kubectl", &args, Some(&input))?;
    Ok(())
}

fn kubectl_prefix(source: &DnsSourceArgs) -> Vec<OsString> {
    let mut args = Vec::new();
    if let Some(kubeconfig) = &source.kubeconfig {
        args.push(OsString::from("--kubeconfig"));
        args.push(kubeconfig.as_os_str().to_owned());
    }
    if let Some(context) = &source.context {
        args.push(OsString::from("--context"));
        args.push(OsString::from(context));
    }
    args
}

fn run_command(
    program: &'static str,
    args: &[OsString],
    input: Option<&[u8]>,
) -> Result<Vec<u8>, CliError> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if input.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command
        .spawn()
        .map_err(|source| CliError::CommandStart { program, source })?;
    if let Some(input) = input {
        let Some(mut stdin) = child.stdin.take() else {
            return Err(CliError::CommandInput {
                program,
                source: io::Error::other("child stdin was not piped"),
            });
        };
        stdin
            .write_all(input)
            .map_err(|source| CliError::CommandInput { program, source })?;
    }
    let output = child
        .wait_with_output()
        .map_err(|source| CliError::CommandStart { program, source })?;
    ensure_success(program, output)
}

fn ensure_success(program: &'static str, output: Output) -> Result<Vec<u8>, CliError> {
    if output.status.success() {
        return Ok(output.stdout);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr = stderr.trim();
    let stderr = if stderr.is_empty() {
        "no error output".to_owned()
    } else {
        stderr.chars().take(4_096).collect()
    };
    Err(CliError::CommandFailed {
        program,
        status: output.status.to_string(),
        stderr,
    })
}

fn wait_for_dns(records: &[super::DnsRecord], timeout_seconds: u64) -> Result<(), CliError> {
    let deadline = Instant::now() + Duration::from_secs(timeout_seconds);
    loop {
        let (successes, failures) = check_records_with(records, system_lookup);
        if failures.is_empty() {
            for (name, address) in successes {
                println!("OK   {name} -> {address}");
            }
            return Ok(());
        }
        if Instant::now() >= deadline {
            for failure in &failures {
                eprintln!("FAIL {failure}");
            }
            return Err(CliError::DnsConvergenceTimeout {
                seconds: timeout_seconds,
                failures: failures.len(),
            });
        }
        thread::sleep(DNS_POLL_INTERVAL);
    }
}

#[cfg(test)]
mod tests {
    use std::{
        net::Ipv4Addr,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;

    static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct TemporaryCredential(PathBuf);

    impl TemporaryCredential {
        #[cfg(unix)]
        fn create(contents: &str, mode: u32) -> io::Result<Self> {
            let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "heterocloud-credential-test-{}-{sequence}",
                std::process::id()
            ));
            fs::write(&path, contents)?;
            fs::set_permissions(&path, fs::Permissions::from_mode(mode))?;
            Ok(Self(path))
        }
    }

    impl Drop for TemporaryCredential {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    fn base_args(provider: &str) -> ReconcileArgs {
        ReconcileArgs {
            source: DnsSourceArgs {
                domain: "heterocloud.example.com".into(),
                public_ip: vec![
                    Ipv4Addr::new(163, 220, 236, 51),
                    Ipv4Addr::new(163, 220, 236, 52),
                    Ipv4Addr::new(163, 220, 236, 53),
                ],
                allow_non_public: false,
                kubeconfig: None,
                context: None,
                namespace: "heterocloud-flow".into(),
                service: "heterocloud-flow-livekit-rtc".into(),
            },
            provider: provider.into(),
            credential_files: Vec::new(),
            credential_secrets: Vec::new(),
            provider_args: Vec::new(),
            provider_values: Vec::new(),
            controller_namespace: "heterocloud-dns".into(),
            controller_release: "heterocloud-dns".into(),
            credential_secret_name: "heterocloud-dns-provider".into(),
            endpoint_name: "heterocloud-public".into(),
            chart_version: DEFAULT_CHART_VERSION.into(),
            txt_owner_id: None,
            ttl: 60,
            timeout_seconds: 300,
            no_wait_dns: false,
            dry_run: true,
        }
    }

    #[test]
    fn provider_choice_does_not_change_desired_dns() -> Result<(), Box<dyn std::error::Error>> {
        let cloudflare = build_plan(&base_args("cloudflare"))?;
        let rfc2136 = build_plan(&base_args("rfc2136"))?;
        assert_eq!(cloudflare.endpoint, rfc2136.endpoint);
        assert_eq!(cloudflare.records, rfc2136.records);
        assert_eq!(cloudflare.helm_values["provider"]["name"], "cloudflare");
        assert_eq!(rfc2136.helm_values["provider"]["name"], "rfc2136");
        Ok(())
    }

    #[test]
    fn endpoint_is_scoped_and_contains_all_public_services()
    -> Result<(), Box<dyn std::error::Error>> {
        let plan = build_plan(&base_args("inmemory"))?;
        let endpoints = plan.endpoint["spec"]["endpoints"]
            .as_array()
            .ok_or("endpoints must be an array")?;
        assert_eq!(endpoints.len(), 12);
        assert_eq!(endpoints[0]["dnsName"], "cloud-a.heterocloud.example.com");
        assert_eq!(endpoints[11]["dnsName"], "turn-c.heterocloud.example.com");
        assert_eq!(
            plan.helm_values["labelFilter"],
            "app.kubernetes.io/managed-by=heterocloud"
        );
        assert_eq!(plan.helm_values["policy"], "sync");
        assert_eq!(plan.helm_values["managedRecordTypes"], json!(["A"]));
        Ok(())
    }

    #[test]
    fn existing_secret_mapping_stays_out_of_provider_contract()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut args = base_args("cloudflare");
        args.credential_secrets = vec!["CF_API_TOKEN=cloudflare-credentials:api-token".into()];
        let plan = build_plan(&args)?;
        assert_eq!(
            plan.helm_values["env"],
            json!([{
                "name": "CF_API_TOKEN",
                "valueFrom": {"secretKeyRef": {
                    "name": "cloudflare-credentials",
                    "key": "api-token"
                }}
            }])
        );
        Ok(())
    }

    #[test]
    fn rejects_duplicate_or_malformed_provider_inputs() {
        let mut duplicate = base_args("cloudflare");
        duplicate.credential_secrets =
            vec!["CF_API_TOKEN=one:key".into(), "CF_API_TOKEN=two:key".into()];
        assert!(build_plan(&duplicate).is_err());

        let mut malformed = base_args("CloudFlare");
        assert!(build_plan(&malformed).is_err());
        malformed.provider = "cloudflare".into();
        malformed.provider_args = vec!["cloudflare-proxied".into()];
        assert!(build_plan(&malformed).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn credential_contents_never_enter_rendered_values() -> Result<(), Box<dyn std::error::Error>> {
        let credential = TemporaryCredential::create("sensitive-test-token", 0o600)?;
        let mut args = base_args("cloudflare");
        args.credential_files = vec![format!("CF_API_TOKEN={}", credential.0.display())];

        let plan = build_plan(&args)?;
        let rendered = serde_json::to_string(&plan.helm_values)?;
        assert!(!rendered.contains("sensitive-test-token"));
        assert_eq!(
            plan.helm_values["env"],
            json!([{
                "name": "CF_API_TOKEN",
                "valueFrom": {"secretKeyRef": {
                    "name": "heterocloud-dns-provider",
                    "key": "CF_API_TOKEN"
                }}
            }])
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn credential_file_mode_must_be_read_write_or_read_only_for_owner()
    -> Result<(), Box<dyn std::error::Error>> {
        for mode in [0o400, 0o600] {
            let credential = TemporaryCredential::create("test-token", mode)?;
            assert!(canonical_credential_path(&credential.0).is_ok());
        }
        for mode in [0o200, 0o700, 0o640, 0o604] {
            let credential = TemporaryCredential::create("test-token", mode)?;
            assert!(matches!(
                canonical_credential_path(&credential.0),
                Err(CliError::UnsafeCredentialMode { .. })
            ));
        }
        Ok(())
    }
}
