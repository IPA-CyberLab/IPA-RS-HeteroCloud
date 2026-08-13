use std::{
    collections::BTreeSet,
    io,
    net::{IpAddr, Ipv4Addr, ToSocketAddrs},
    path::PathBuf,
    process::Command,
};

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use thiserror::Error;

mod external_dns;

pub use external_dns::ReconcileArgs;

const NODE_SCOPED_SERVICE_PREFIXES: [&str; 1] = ["cloud"];
const FLOW_SERVICE_PREFIX: &str = "flow";
const MAX_BASE_DOMAIN_LENGTH: usize = 230;

#[derive(Debug, Parser)]
#[command(
    name = "heterocloud",
    version,
    about = "Operate HeteroCloud managed services"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: TopLevelCommand,
}

#[derive(Debug, Subcommand)]
pub enum TopLevelCommand {
    /// Generate or verify public DNS records.
    Dns(DnsArgs),
}

#[derive(Debug, Args)]
pub struct DnsArgs {
    #[command(subcommand)]
    pub command: DnsCommand,
}

#[derive(Debug, Subcommand)]
pub enum DnsCommand {
    /// Print copy-paste-ready A records for every public service endpoint.
    Records(RecordsArgs),
    /// Verify that every generated hostname resolves to its expected address.
    Verify(VerifyArgs),
    /// Reconcile records through a provider-neutral ExternalDNS controller.
    Reconcile(Box<ReconcileArgs>),
}

#[derive(Clone, Debug, Args)]
pub struct DnsSourceArgs {
    /// Base DNS zone, for example heterocloud.example.com.
    #[arg(long)]
    pub domain: String,

    /// Public IPv4 address. Repeat for every public HeteroNetwork node.
    #[arg(long, value_name = "IP")]
    pub public_ip: Vec<Ipv4Addr>,

    /// Permit private, shared, documentation, or otherwise non-public IPv4 addresses.
    #[arg(long)]
    pub allow_non_public: bool,

    /// Kubeconfig used for automatic public-IP discovery.
    #[arg(long, value_name = "PATH")]
    pub kubeconfig: Option<PathBuf>,

    /// Kubernetes context used for automatic public-IP discovery.
    #[arg(long)]
    pub context: Option<String>,

    /// Namespace containing the HeteroCloud Flow public Service.
    #[arg(long, default_value = "heterocloud-flow")]
    pub namespace: String,

    /// LoadBalancer Service used to discover HeteroNetwork public addresses.
    #[arg(long, default_value = "heterocloud-flow-turn")]
    pub service: String,
}

#[derive(Debug, Args)]
pub struct RecordsArgs {
    #[command(flatten)]
    pub source: DnsSourceArgs,

    /// DNS TTL in seconds.
    #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u32).range(30..=86_400))]
    pub ttl: u32,

    /// Output format. Zone output is accepted by BIND-compatible bulk importers.
    #[arg(long, value_enum, default_value_t = OutputFormat::Zone)]
    pub format: OutputFormat,
}

#[derive(Debug, Args)]
pub struct VerifyArgs {
    #[command(flatten)]
    pub source: DnsSourceArgs,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum OutputFormat {
    #[default]
    Zone,
    Table,
    Json,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DnsRecord {
    #[serde(rename = "type")]
    pub record_type: &'static str,
    pub name: String,
    pub value: Ipv4Addr,
    pub ttl: u32,
    pub service: &'static str,
    pub node: String,
}

#[derive(Debug, Error)]
pub enum CliError {
    #[error(
        "invalid DNS domain `{0}`: use a public ASCII DNS name such as heterocloud.example.com"
    )]
    InvalidDomain(String),
    #[error(
        "no public IPv4 addresses were found; pass --public-ip or check the LoadBalancer Service status"
    )]
    NoPublicAddresses,
    #[error(
        "{0} is not a globally routable IPv4 address; use --allow-non-public only for a private lab"
    )]
    NonPublicAddress(Ipv4Addr),
    #[error("failed to execute kubectl: {0}")]
    KubectlStart(#[source] io::Error),
    #[error("kubectl failed: {0}")]
    KubectlFailed(String),
    #[error("failed to process JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("LoadBalancer Service returned an invalid IPv4 address `{0}`")]
    InvalidIngressAddress(String),
    #[error("DNS verification failed for {0} record(s)")]
    VerificationFailed(usize),
    #[error("invalid {kind} `{value}`")]
    InvalidValue { kind: &'static str, value: String },
    #[error("invalid credential mapping `{0}`; expected ENV=PATH or ENV=SECRET:KEY")]
    InvalidCredential(String),
    #[error("failed to inspect credential file {path}: {source}")]
    CredentialFile {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("credential file {0} must be a regular file and not a symbolic link")]
    UnsafeCredentialFile(PathBuf),
    #[error("credential file {path} has unsafe mode {mode:o}; use chmod 600 or chmod 400")]
    UnsafeCredentialMode { path: PathBuf, mode: u32 },
    #[error("credential file {0} must contain 1 to 65536 bytes without NUL or line breaks")]
    InvalidCredentialContent(PathBuf),
    #[error("failed to start {program}: {source}")]
    CommandStart {
        program: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("failed to write input to {program}: {source}")]
    CommandInput {
        program: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("{program} failed ({status}): {stderr}")]
    CommandFailed {
        program: &'static str,
        status: String,
        stderr: String,
    },
    #[error("DNS did not converge within {seconds} seconds ({failures} record(s) still invalid)")]
    DnsConvergenceTimeout { seconds: u64, failures: usize },
}

#[derive(Debug, Deserialize)]
struct KubernetesService {
    status: Option<KubernetesServiceStatus>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KubernetesServiceStatus {
    load_balancer: Option<KubernetesLoadBalancerStatus>,
}

#[derive(Debug, Deserialize)]
struct KubernetesLoadBalancerStatus {
    #[serde(default)]
    ingress: Vec<KubernetesLoadBalancerIngress>,
}

#[derive(Debug, Deserialize)]
struct KubernetesLoadBalancerIngress {
    ip: Option<String>,
}

pub fn execute(cli: Cli) -> Result<(), CliError> {
    match cli.command {
        TopLevelCommand::Dns(args) => match args.command {
            DnsCommand::Records(args) => print_records(args),
            DnsCommand::Verify(args) => verify_records(args),
            DnsCommand::Reconcile(args) => external_dns::reconcile(*args),
        },
    }
}

fn print_records(args: RecordsArgs) -> Result<(), CliError> {
    let (domain, addresses) = resolve_source(&args.source)?;
    let records = build_records(&domain, &addresses, args.ttl);
    print!(
        "{}",
        render_records(&records, args.format, &domain, &addresses)?
    );
    Ok(())
}

fn verify_records(args: VerifyArgs) -> Result<(), CliError> {
    let (domain, addresses) = resolve_source(&args.source)?;
    let records = build_records(&domain, &addresses, 60);
    let failures = verify_with(&records, system_lookup);
    if failures.is_empty() {
        println!("Verified {} A records for {domain}.", records.len());
        return Ok(());
    }

    for failure in &failures {
        eprintln!("FAIL {failure}");
    }
    Err(CliError::VerificationFailed(failures.len()))
}

fn resolve_source(args: &DnsSourceArgs) -> Result<(String, Vec<Ipv4Addr>), CliError> {
    let domain = normalize_domain(&args.domain)?;
    let addresses = if args.public_ip.is_empty() {
        discover_public_addresses(args)?
    } else {
        normalize_addresses(args.public_ip.clone())
    };
    validate_addresses(&addresses, args.allow_non_public)?;
    Ok((domain, addresses))
}

fn discover_public_addresses(args: &DnsSourceArgs) -> Result<Vec<Ipv4Addr>, CliError> {
    let mut command = Command::new("kubectl");
    if let Some(kubeconfig) = &args.kubeconfig {
        command.arg("--kubeconfig").arg(kubeconfig);
    }
    if let Some(context) = &args.context {
        command.arg("--context").arg(context);
    }
    let output = command
        .arg("get")
        .arg("service")
        .arg(&args.service)
        .arg("--namespace")
        .arg(&args.namespace)
        .arg("--output=json")
        .output()
        .map_err(CliError::KubectlStart)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let detail = if stderr.is_empty() {
            format!("process exited with {}", output.status)
        } else {
            stderr
        };
        return Err(CliError::KubectlFailed(detail));
    }
    public_addresses_from_service_json(&output.stdout)
}

pub fn public_addresses_from_service_json(input: &[u8]) -> Result<Vec<Ipv4Addr>, CliError> {
    let service: KubernetesService = serde_json::from_slice(input)?;
    let mut addresses = Vec::new();
    if let Some(load_balancer) = service.status.and_then(|status| status.load_balancer) {
        for ingress in load_balancer.ingress {
            if let Some(ip) = ingress.ip {
                addresses.push(
                    ip.parse::<Ipv4Addr>()
                        .map_err(|_| CliError::InvalidIngressAddress(ip))?,
                );
            }
        }
    }
    let addresses = normalize_addresses(addresses);
    if addresses.is_empty() {
        return Err(CliError::NoPublicAddresses);
    }
    Ok(addresses)
}

pub fn normalize_domain(input: &str) -> Result<String, CliError> {
    let domain = input.trim().trim_end_matches('.').to_ascii_lowercase();
    if domain.is_empty()
        || domain.len() > MAX_BASE_DOMAIN_LENGTH
        || !domain.is_ascii()
        || !domain.contains('.')
        || domain.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
    {
        return Err(CliError::InvalidDomain(input.to_owned()));
    }
    Ok(domain)
}

fn normalize_addresses(addresses: Vec<Ipv4Addr>) -> Vec<Ipv4Addr> {
    addresses
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn validate_addresses(addresses: &[Ipv4Addr], allow_non_public: bool) -> Result<(), CliError> {
    if addresses.is_empty() {
        return Err(CliError::NoPublicAddresses);
    }
    if !allow_non_public
        && let Some(address) = addresses
            .iter()
            .copied()
            .find(|address| !is_public_ipv4(*address))
    {
        return Err(CliError::NonPublicAddress(address));
    }
    Ok(())
}

pub fn build_records(domain: &str, addresses: &[Ipv4Addr], ttl: u32) -> Vec<DnsRecord> {
    let mut records = Vec::with_capacity(addresses.len() * 3);
    for (index, address) in addresses.iter().enumerate() {
        let node = node_label(index);
        for service in NODE_SCOPED_SERVICE_PREFIXES {
            records.push(DnsRecord {
                record_type: "A",
                name: format!("{service}-{node}.{domain}"),
                value: *address,
                ttl,
                service,
                node: node.clone(),
            });
        }
    }
    for address in addresses {
        records.push(DnsRecord {
            record_type: "A",
            name: domain.to_owned(),
            value: *address,
            ttl,
            service: "cloud",
            node: "cluster".to_owned(),
        });
    }
    for address in addresses {
        records.push(DnsRecord {
            record_type: "A",
            name: format!("{FLOW_SERVICE_PREFIX}.{domain}"),
            value: *address,
            ttl,
            service: FLOW_SERVICE_PREFIX,
            node: "cluster".to_owned(),
        });
    }
    records
}

pub fn render_records(
    records: &[DnsRecord],
    format: OutputFormat,
    domain: &str,
    addresses: &[Ipv4Addr],
) -> Result<String, CliError> {
    match format {
        OutputFormat::Zone => {
            let mut output = format!(
                "; HeteroCloud public endpoints for {domain}\n; Paste this complete block into a BIND-compatible zone importer.\n"
            );
            for record in records {
                output.push_str(&format!(
                    "{}.	{}	IN	{}	{}\n",
                    record.name, record.ttl, record.record_type, record.value
                ));
            }
            output.push_str("; Verify after DNS propagation:\n; heterocloud dns verify");
            output.push_str(&format!(" --domain {domain}"));
            for address in addresses {
                output.push_str(&format!(" --public-ip {address}"));
            }
            output.push('\n');
            Ok(output)
        }
        OutputFormat::Table => {
            let mut output = "TYPE\tNAME\tVALUE\tTTL\n".to_owned();
            for record in records {
                output.push_str(&format!(
                    "{}\t{}\t{}\t{}\n",
                    record.record_type, record.name, record.value, record.ttl
                ));
            }
            Ok(output)
        }
        OutputFormat::Json => Ok(format!("{}\n", serde_json::to_string_pretty(records)?)),
    }
}

pub fn verify_with<F>(records: &[DnsRecord], mut lookup: F) -> Vec<String>
where
    F: FnMut(&str) -> io::Result<Vec<IpAddr>>,
{
    let (successes, failures) = check_records_with(records, &mut lookup);
    for (name, address) in successes {
        println!("OK   {name} -> {address}");
    }
    failures
}

pub(crate) fn check_records_with<F>(
    records: &[DnsRecord],
    mut lookup: F,
) -> (Vec<(String, Ipv4Addr)>, Vec<String>)
where
    F: FnMut(&str) -> io::Result<Vec<IpAddr>>,
{
    let mut successes = Vec::new();
    let mut failures = Vec::new();
    let mut expected_by_name = std::collections::BTreeMap::<String, BTreeSet<IpAddr>>::new();
    for record in records {
        expected_by_name
            .entry(record.name.clone())
            .or_default()
            .insert(IpAddr::V4(record.value));
    }
    for (name, expected) in expected_by_name {
        match lookup(&name) {
            Ok(addresses) => {
                let actual = addresses.into_iter().collect::<BTreeSet<_>>();
                if actual != expected {
                    failures.push(format!(
                        "{} expected {}, resolved {}",
                        name,
                        render_ip_set(&expected),
                        render_ip_set(&actual)
                    ));
                } else {
                    successes.extend(expected.iter().filter_map(|address| match address {
                        IpAddr::V4(address) => Some((name.clone(), *address)),
                        IpAddr::V6(_) => None,
                    }));
                }
            }
            Err(error) => failures.push(format!("{name} lookup failed: {error}")),
        }
    }
    (successes, failures)
}

pub(crate) fn system_lookup(host: &str) -> io::Result<Vec<IpAddr>> {
    (host, 0)
        .to_socket_addrs()
        .map(|addresses| addresses.map(|address| address.ip()).collect())
}

fn render_ip_set(addresses: &BTreeSet<IpAddr>) -> String {
    if addresses.is_empty() {
        return "no addresses".to_owned();
    }
    addresses
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn node_label(index: usize) -> String {
    let mut number = index + 1;
    let mut label = Vec::new();
    while number > 0 {
        number -= 1;
        label.push((b'a' + (number % 26) as u8) as char);
        number /= 26;
    }
    label.into_iter().rev().collect()
}

fn is_public_ipv4(address: Ipv4Addr) -> bool {
    let [a, b, c, _] = address.octets();
    !(a == 0
        || a == 10
        || a == 127
        || (a == 100 && (64..=127).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 192 && b == 0 && c == 2)
        || (a == 192 && b == 88 && c == 99)
        || (a == 192 && b == 168)
        || (a == 198 && (b == 18 || b == 19))
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113)
        || a >= 224)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_and_validates_domains() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(normalize_domain("Cloud.Example.COM.")?, "cloud.example.com");
        assert!(normalize_domain("localhost").is_err());
        assert!(normalize_domain("-cloud.example.com").is_err());
        assert!(normalize_domain("cloud_.example.com").is_err());
        assert!(normalize_domain(&format!("{}com", "a.".repeat(115))).is_err());
        Ok(())
    }

    #[test]
    fn parses_sorts_and_deduplicates_load_balancer_addresses()
    -> Result<(), Box<dyn std::error::Error>> {
        let input = br#"{
          "status": {"loadBalancer": {"ingress": [
            {"ip": "163.220.236.53"},
            {"hostname": "ignored.example.com"},
            {"ip": "163.220.236.51"},
            {"ip": "163.220.236.51"}
          ]}}
        }"#;
        assert_eq!(
            public_addresses_from_service_json(input)?,
            vec![
                Ipv4Addr::new(163, 220, 236, 51),
                Ipv4Addr::new(163, 220, 236, 53)
            ]
        );
        Ok(())
    }

    #[test]
    fn rejects_malformed_load_balancer_addresses() {
        let input = br#"{
          "status": {"loadBalancer": {"ingress": [{"ip": "not-an-ip"}]}}
        }"#;
        assert!(matches!(
            public_addresses_from_service_json(input),
            Err(CliError::InvalidIngressAddress(_))
        ));
    }

    #[test]
    fn generates_node_scoped_cloud_and_cluster_scoped_flow_records() {
        let addresses = vec![
            Ipv4Addr::new(163, 220, 236, 51),
            Ipv4Addr::new(163, 220, 236, 52),
            Ipv4Addr::new(163, 220, 236, 53),
        ];
        let records = build_records("hc.example.com", &addresses, 60);
        assert_eq!(records.len(), 9);
        assert_eq!(records[0].name, "cloud-a.hc.example.com");
        assert_eq!(records[1].name, "cloud-b.hc.example.com");
        assert_eq!(records[2].name, "cloud-c.hc.example.com");
        assert_eq!(records[3].name, "hc.example.com");
        assert_eq!(records[5].name, "hc.example.com");
        assert_eq!(records[6].name, "flow.hc.example.com");
        assert_eq!(records[8].name, "flow.hc.example.com");
        assert!(records.iter().all(|record| {
            !record.name.starts_with("flow-")
                && !record.name.starts_with("rtc-")
                && !record.name.starts_with("turn-")
        }));
    }

    #[test]
    fn verifies_cluster_records_as_one_multi_address_rrset() {
        let records = build_records(
            "hc.example.com",
            &[
                Ipv4Addr::new(163, 220, 236, 51),
                Ipv4Addr::new(163, 220, 236, 52),
            ],
            60,
        );
        let cluster_records = records
            .into_iter()
            .filter(|record| record.name == "hc.example.com")
            .collect::<Vec<_>>();
        let failures = verify_with(&cluster_records, |_| {
            Ok(vec![
                IpAddr::V4(Ipv4Addr::new(163, 220, 236, 52)),
                IpAddr::V4(Ipv4Addr::new(163, 220, 236, 51)),
            ])
        });
        assert!(failures.is_empty());
    }

    #[test]
    fn verifies_flow_as_one_multi_address_rrset() {
        let records = build_records(
            "hc.example.com",
            &[
                Ipv4Addr::new(163, 220, 236, 51),
                Ipv4Addr::new(163, 220, 236, 52),
                Ipv4Addr::new(163, 220, 236, 53),
            ],
            60,
        );
        let flow_records = records
            .into_iter()
            .filter(|record| record.name == "flow.hc.example.com")
            .collect::<Vec<_>>();
        let failures = verify_with(&flow_records, |_| {
            Ok(vec![
                IpAddr::V4(Ipv4Addr::new(163, 220, 236, 53)),
                IpAddr::V4(Ipv4Addr::new(163, 220, 236, 51)),
                IpAddr::V4(Ipv4Addr::new(163, 220, 236, 52)),
            ])
        });
        assert!(failures.is_empty());
    }

    #[test]
    fn zone_output_is_ready_for_bulk_import() -> Result<(), Box<dyn std::error::Error>> {
        let addresses = vec![Ipv4Addr::new(163, 220, 236, 51)];
        let records = build_records("hc.example.com", &addresses, 60);
        let output = render_records(&records, OutputFormat::Zone, "hc.example.com", &addresses)?;
        assert!(output.contains("cloud-a.hc.example.com.\t60\tIN\tA\t163.220.236.51"));
        assert!(output.contains("flow.hc.example.com.\t60\tIN\tA\t163.220.236.51"));
        assert!(output.contains(
            "; heterocloud dns verify --domain hc.example.com --public-ip 163.220.236.51"
        ));
        Ok(())
    }

    #[test]
    fn verification_rejects_extra_or_wrong_addresses() {
        let records = build_records("hc.example.com", &[Ipv4Addr::new(163, 220, 236, 51)], 60);
        let failures = verify_with(&records[..1], |_| {
            Ok(vec![
                IpAddr::V4(Ipv4Addr::new(163, 220, 236, 51)),
                IpAddr::V4(Ipv4Addr::new(163, 220, 236, 52)),
            ])
        });
        assert_eq!(failures.len(), 1);
    }

    #[test]
    fn rejects_non_public_addresses_by_default() {
        assert!(!is_public_ipv4(Ipv4Addr::new(10, 250, 0, 4)));
        assert!(!is_public_ipv4(Ipv4Addr::new(100, 89, 33, 61)));
        assert!(!is_public_ipv4(Ipv4Addr::new(203, 0, 113, 1)));
        assert!(is_public_ipv4(Ipv4Addr::new(163, 220, 236, 51)));
    }

    #[test]
    fn node_labels_continue_after_z() {
        assert_eq!(node_label(0), "a");
        assert_eq!(node_label(25), "z");
        assert_eq!(node_label(26), "aa");
        assert_eq!(node_label(27), "ab");
    }
}
