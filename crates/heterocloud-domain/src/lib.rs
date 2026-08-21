use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

pub const POLICY_VERSION: &str = "2026-07-31";

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct UserId(pub Uuid);

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct OrganizationId(pub Uuid);

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ProjectId(pub Uuid);

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct PrincipalId(pub Uuid);

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct PolicyId(pub Uuid);

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ServiceInstanceId(pub Uuid);

macro_rules! impl_id {
    ($name:ident) => {
        impl $name {
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

impl_id!(UserId);
impl_id!(OrganizationId);
impl_id!(ProjectId);
impl_id!(PrincipalId);
impl_id!(PolicyId);
impl_id!(ServiceInstanceId);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UserStatus {
    Active,
    Suspended,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct User {
    pub id: UserId,
    pub email: String,
    pub display_name: String,
    pub status: UserStatus,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Organization {
    pub id: OrganizationId,
    pub slug: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Project {
    pub id: ProjectId,
    pub organization_id: OrganizationId,
    pub slug: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalKind {
    User,
    ServiceAccount,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Principal {
    pub id: PrincipalId,
    pub organization_id: OrganizationId,
    pub kind: PrincipalKind,
    pub name: String,
    pub user_id: Option<UserId>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum PolicyEffect {
    Allow,
    Deny,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyStatement {
    pub effect: PolicyEffect,
    pub actions: Vec<String>,
    pub resources: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyDocument {
    pub version: String,
    pub statements: Vec<PolicyStatement>,
}

impl PolicyDocument {
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.version != POLICY_VERSION {
            return Err(DomainError::UnsupportedPolicyVersion(self.version.clone()));
        }
        if self.statements.is_empty() || self.statements.len() > 128 {
            return Err(DomainError::InvalidPolicy(
                "a policy requires between 1 and 128 statements".into(),
            ));
        }
        for statement in &self.statements {
            if statement.actions.is_empty()
                || statement.actions.len() > 128
                || statement.resources.is_empty()
                || statement.resources.len() > 128
            {
                return Err(DomainError::InvalidPolicy(
                    "each statement requires 1..128 actions and resources".into(),
                ));
            }
            for pattern in statement.actions.iter().chain(&statement.resources) {
                validate_pattern(pattern)?;
            }
        }
        Ok(())
    }
}

fn validate_pattern(pattern: &str) -> Result<(), DomainError> {
    if pattern.is_empty() || pattern.len() > 512 || pattern.chars().any(char::is_whitespace) {
        return Err(DomainError::InvalidPolicy(
            "policy patterns must be 1..512 non-whitespace characters".into(),
        ));
    }
    if let Some(index) = pattern.find('*')
        && index + 1 != pattern.len()
    {
        return Err(DomainError::InvalidPolicy(
            "wildcards are supported only as a terminal suffix".into(),
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IamPolicy {
    pub id: PolicyId,
    pub organization_id: OrganizationId,
    pub name: String,
    pub document: PolicyDocument,
    pub semantics_digest: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FlowSpec {
    pub region: String,
    pub max_participants: u32,
    pub max_rooms: u32,
    #[serde(default)]
    pub rate_limit: FlowRateLimit,
    pub metadata: Value,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FlowRateLimit {
    pub requests_per_second: u32,
    pub burst: u32,
}

impl Default for FlowRateLimit {
    fn default() -> Self {
        Self {
            requests_per_second: DEFAULT_FLOW_RATE_LIMIT_REQUESTS_PER_SECOND,
            burst: DEFAULT_FLOW_RATE_LIMIT_BURST,
        }
    }
}

pub const DEFAULT_FLOW_MAX_ROOMS: u32 = 100;
pub const MAX_FLOW_ROOMS: u32 = 1_000_000;
pub const DEFAULT_FLOW_RATE_LIMIT_REQUESTS_PER_SECOND: u32 = 20;
pub const DEFAULT_FLOW_RATE_LIMIT_BURST: u32 = 40;
pub const MAX_FLOW_RATE_LIMIT_REQUESTS_PER_SECOND: u32 = 1_000;
pub const MAX_FLOW_RATE_LIMIT_BURST: u32 = 5_000;
pub const MAX_FLOW_PARTICIPANTS: u32 = 100_000;
pub const MAX_TENANT_SERVICES: u32 = 10_000;
pub const MAX_TENANT_FLOW_ROOMS: u64 = 100_000_000;

pub const MIN_FLASH_REPLICAS: u32 = 1;
pub const MAX_FLASH_REPLICAS: u32 = 100;
pub const MIN_FLASH_CPU_MILLIS: u32 = 10;
pub const MAX_FLASH_CPU_MILLIS: u32 = 4_000;
pub const MIN_FLASH_MEMORY_MIB: u32 = 16;
pub const MAX_FLASH_MEMORY_MIB: u32 = 8_128;
pub const MIN_FLASH_EPHEMERAL_STORAGE_GIB: u32 = 1;
pub const MAX_FLASH_EPHEMERAL_STORAGE_GIB: u32 = 10;
pub const DEFAULT_FLASH_EPHEMERAL_STORAGE_GIB: u32 = 10;
pub const MAX_FLASH_ORGANIZATION_CPU_MILLIS: u64 = 20_000;
pub const MAX_FLASH_ORGANIZATION_MEMORY_MIB: u64 = 32_768;
pub const MAX_FLASH_ORGANIZATION_EPHEMERAL_STORAGE_GIB: u64 = 100;
pub const MAX_FLASH_ORGANIZATION_REPLICAS: u64 = 100;
pub const MIN_FLASH_SERVICE_PORT: u16 = 30_000;
pub const MAX_FLASH_SERVICE_PORT: u16 = 32_767;
pub const MAX_FLASH_PORTS: usize = 16;
pub const MAX_FLASH_REGION_LENGTH: usize = 63;
pub const MAX_FLASH_IMAGE_LENGTH: usize = 512;
pub const MAX_FLASH_PORT_NAME_LENGTH: usize = 63;
pub const MAX_FLASH_ENV_VARS: usize = 128;
pub const MAX_FLASH_ENV_KEY_LENGTH: usize = 253;
pub const MAX_FLASH_ENV_VALUE_LENGTH: usize = 16 * 1024;
pub const MAX_FLASH_COMMAND_PARTS: usize = 128;
pub const MAX_FLASH_ARGS: usize = 256;
pub const MAX_FLASH_PROCESS_VALUE_LENGTH: usize = 4 * 1024;
pub const MAX_FLASH_METADATA_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceQuotaLimits {
    pub flow: FlowQuotaLimits,
    pub flash: FlashQuotaLimits,
    pub registry: RegistryQuotaLimits,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FlowQuotaLimits {
    pub max_services: u32,
    pub max_rooms_per_service: u32,
    pub max_total_rooms: u64,
    pub max_participants_per_service: u32,
    pub max_rate_limit_requests_per_second: u32,
    pub max_rate_limit_burst: u32,
    pub max_developer_credentials_per_service: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FlashQuotaLimits {
    pub max_services: u32,
    pub max_replicas_per_service: u32,
    pub max_cpu_millis_per_vm: u32,
    pub max_memory_mib_per_vm: u32,
    pub max_disk_gib_per_vm: u32,
    pub max_total_replicas: u64,
    pub max_total_cpu_millis: u64,
    pub max_total_memory_mib: u64,
    pub max_total_disk_gib: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryQuotaLimits {
    pub storage_gib: u32,
    pub max_credentials: u32,
}

impl Default for ResourceQuotaLimits {
    fn default() -> Self {
        Self {
            flow: FlowQuotaLimits {
                max_services: 100,
                max_rooms_per_service: MAX_FLOW_ROOMS,
                max_total_rooms: u64::from(MAX_FLOW_ROOMS),
                max_participants_per_service: MAX_FLOW_PARTICIPANTS,
                max_rate_limit_requests_per_second: MAX_FLOW_RATE_LIMIT_REQUESTS_PER_SECOND,
                max_rate_limit_burst: MAX_FLOW_RATE_LIMIT_BURST,
                max_developer_credentials_per_service: 100,
            },
            flash: FlashQuotaLimits {
                max_services: 100,
                max_replicas_per_service: MAX_FLASH_REPLICAS,
                max_cpu_millis_per_vm: MAX_FLASH_CPU_MILLIS,
                max_memory_mib_per_vm: MAX_FLASH_MEMORY_MIB,
                max_disk_gib_per_vm: MAX_FLASH_EPHEMERAL_STORAGE_GIB,
                max_total_replicas: MAX_FLASH_ORGANIZATION_REPLICAS,
                max_total_cpu_millis: MAX_FLASH_ORGANIZATION_CPU_MILLIS,
                max_total_memory_mib: MAX_FLASH_ORGANIZATION_MEMORY_MIB,
                max_total_disk_gib: MAX_FLASH_ORGANIZATION_EPHEMERAL_STORAGE_GIB,
            },
            registry: RegistryQuotaLimits {
                storage_gib: 10,
                max_credentials: 10,
            },
        }
    }
}

impl ResourceQuotaLimits {
    pub fn validate(&self) -> Result<(), DomainError> {
        let flow = &self.flow;
        if !(1..=MAX_TENANT_SERVICES).contains(&flow.max_services) {
            return Err(invalid_quota(
                "flow.max_services is outside the safety range",
            ));
        }
        if !(1..=MAX_FLOW_ROOMS).contains(&flow.max_rooms_per_service)
            || flow.max_total_rooms < u64::from(flow.max_rooms_per_service)
            || flow.max_total_rooms > MAX_TENANT_FLOW_ROOMS
        {
            return Err(invalid_quota(
                "Flow room limits are inconsistent or outside the safety range",
            ));
        }
        if !(1..=MAX_FLOW_PARTICIPANTS).contains(&flow.max_participants_per_service) {
            return Err(invalid_quota(
                "flow.max_participants_per_service is outside the safety range",
            ));
        }
        if !(1..=MAX_FLOW_RATE_LIMIT_REQUESTS_PER_SECOND)
            .contains(&flow.max_rate_limit_requests_per_second)
            || !(1..=MAX_FLOW_RATE_LIMIT_BURST).contains(&flow.max_rate_limit_burst)
            || flow.max_rate_limit_burst < flow.max_rate_limit_requests_per_second
        {
            return Err(invalid_quota(
                "Flow API rate limits are inconsistent or outside the safety range",
            ));
        }
        if !(1..=10_000).contains(&flow.max_developer_credentials_per_service) {
            return Err(invalid_quota(
                "flow.max_developer_credentials_per_service is outside the safety range",
            ));
        }

        let flash = &self.flash;
        if !(1..=MAX_TENANT_SERVICES).contains(&flash.max_services)
            || !(MIN_FLASH_REPLICAS..=MAX_FLASH_REPLICAS).contains(&flash.max_replicas_per_service)
            || !(MIN_FLASH_CPU_MILLIS..=MAX_FLASH_CPU_MILLIS).contains(&flash.max_cpu_millis_per_vm)
            || !(MIN_FLASH_MEMORY_MIB..=MAX_FLASH_MEMORY_MIB).contains(&flash.max_memory_mib_per_vm)
            || !(MIN_FLASH_EPHEMERAL_STORAGE_GIB..=MAX_FLASH_EPHEMERAL_STORAGE_GIB)
                .contains(&flash.max_disk_gib_per_vm)
        {
            return Err(invalid_quota(
                "Flash per-service limits are outside the safety range",
            ));
        }
        if flash.max_total_replicas < u64::from(flash.max_replicas_per_service)
            || flash.max_total_replicas > 100_000
            || flash.max_total_cpu_millis < u64::from(flash.max_cpu_millis_per_vm)
            || flash.max_total_cpu_millis > 100_000_000
            || flash.max_total_memory_mib < u64::from(flash.max_memory_mib_per_vm)
            || flash.max_total_memory_mib > 1_048_576
            || flash.max_total_disk_gib < u64::from(flash.max_disk_gib_per_vm)
            || flash.max_total_disk_gib > 1_000_000
        {
            return Err(invalid_quota(
                "Flash tenant totals are inconsistent or outside the safety range",
            ));
        }
        if !(1..=10_240).contains(&self.registry.storage_gib) {
            return Err(invalid_quota(
                "registry.storage_gib must be between 1 and 10240",
            ));
        }
        if !(1..=1_000).contains(&self.registry.max_credentials) {
            return Err(invalid_quota(
                "registry.max_credentials must be between 1 and 1000",
            ));
        }
        Ok(())
    }
}

fn invalid_quota(message: impl Into<String>) -> DomainError {
    DomainError::InvalidResourceQuota(message.into())
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FlashProtocol {
    Tcp,
    Udp,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FlashPort {
    pub name: String,
    pub protocol: FlashProtocol,
    pub container_port: u16,
    #[serde(default)]
    pub service_port: u16,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FlashExposureType {
    Internal,
    Public,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FlashTrafficMode {
    Forwarded,
    Direct,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FlashExposure {
    #[serde(rename = "type")]
    pub exposure_type: FlashExposureType,
    pub traffic_mode: FlashTrafficMode,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FlashSpec {
    pub region: String,
    pub image: String,
    pub replicas: u32,
    pub cpu_millis: u32,
    pub memory_mib: u32,
    #[serde(default = "default_flash_ephemeral_storage_gib")]
    pub ephemeral_storage_gib: u32,
    pub ports: Vec<FlashPort>,
    pub exposure: FlashExposure,
    pub env: BTreeMap<String, String>,
    pub command: Vec<String>,
    pub args: Vec<String>,
    pub metadata: BTreeMap<String, Value>,
}

impl FlashSpec {
    pub fn validate(&self) -> Result<(), DomainError> {
        self.validate_inner(true)
    }

    pub fn validate_request(&self) -> Result<(), DomainError> {
        self.validate_inner(false)
    }

    fn validate_inner(&self, require_assigned_service_ports: bool) -> Result<(), DomainError> {
        if self.region.is_empty()
            || self.region.len() > MAX_FLASH_REGION_LENGTH
            || self
                .region
                .bytes()
                .any(|byte| !byte.is_ascii_lowercase() && !byte.is_ascii_digit() && byte != b'-')
            || self.region.starts_with('-')
            || self.region.ends_with('-')
        {
            return Err(invalid_flash_spec(format!(
                "region must be a 1..{MAX_FLASH_REGION_LENGTH} character lowercase DNS label"
            )));
        }
        if self.image.is_empty()
            || self.image.len() > MAX_FLASH_IMAGE_LENGTH
            || self.image.chars().any(char::is_whitespace)
            || self.image.chars().any(char::is_control)
        {
            return Err(invalid_flash_spec(
                "image must be a 1..512 character container image reference without whitespace",
            ));
        }
        if !(MIN_FLASH_REPLICAS..=MAX_FLASH_REPLICAS).contains(&self.replicas) {
            return Err(invalid_flash_spec(format!(
                "replicas must be between {MIN_FLASH_REPLICAS} and {MAX_FLASH_REPLICAS}"
            )));
        }
        if !(MIN_FLASH_CPU_MILLIS..=MAX_FLASH_CPU_MILLIS).contains(&self.cpu_millis) {
            return Err(invalid_flash_spec(format!(
                "cpu_millis must be between {MIN_FLASH_CPU_MILLIS} and {MAX_FLASH_CPU_MILLIS}"
            )));
        }
        if !(MIN_FLASH_MEMORY_MIB..=MAX_FLASH_MEMORY_MIB).contains(&self.memory_mib) {
            return Err(invalid_flash_spec(format!(
                "memory_mib must be between {MIN_FLASH_MEMORY_MIB} and {MAX_FLASH_MEMORY_MIB}"
            )));
        }
        if !(MIN_FLASH_EPHEMERAL_STORAGE_GIB..=MAX_FLASH_EPHEMERAL_STORAGE_GIB)
            .contains(&self.ephemeral_storage_gib)
        {
            return Err(invalid_flash_spec(format!(
                "ephemeral_storage_gib must be between {MIN_FLASH_EPHEMERAL_STORAGE_GIB} and {MAX_FLASH_EPHEMERAL_STORAGE_GIB}"
            )));
        }
        if self.ports.is_empty() || self.ports.len() > MAX_FLASH_PORTS {
            return Err(invalid_flash_spec(format!(
                "ports must contain between 1 and {MAX_FLASH_PORTS} entries"
            )));
        }
        let mut port_names = BTreeSet::new();
        let mut service_ports = BTreeSet::new();
        for port in &self.ports {
            if !valid_flash_port_name(&port.name) {
                return Err(invalid_flash_spec(
                    "port names must be unique lowercase DNS labels of at most 63 characters",
                ));
            }
            if !port_names.insert(port.name.as_str()) {
                return Err(invalid_flash_spec("port names must be unique"));
            }
            if port.container_port == 0 {
                return Err(invalid_flash_spec(
                    "container_port must be between 1 and 65535",
                ));
            }
            if require_assigned_service_ports
                && !(MIN_FLASH_SERVICE_PORT..=MAX_FLASH_SERVICE_PORT).contains(&port.service_port)
            {
                return Err(invalid_flash_spec(format!(
                    "service_port must be assigned between {MIN_FLASH_SERVICE_PORT} and {MAX_FLASH_SERVICE_PORT}"
                )));
            }
            if require_assigned_service_ports
                && !service_ports.insert((port.protocol, port.service_port))
            {
                return Err(invalid_flash_spec(
                    "service_port must be unique within each protocol",
                ));
            }
        }
        if self.exposure.exposure_type == FlashExposureType::Internal
            && self.exposure.traffic_mode != FlashTrafficMode::Forwarded
        {
            return Err(invalid_flash_spec(
                "internal exposure requires forwarded traffic_mode",
            ));
        }
        if self.env.len() > MAX_FLASH_ENV_VARS {
            return Err(invalid_flash_spec(format!(
                "env must contain at most {MAX_FLASH_ENV_VARS} entries"
            )));
        }
        for (name, value) in &self.env {
            if !valid_flash_env_name(name) {
                return Err(invalid_flash_spec(
                    "environment variable names must be valid C identifiers",
                ));
            }
            if value.len() > MAX_FLASH_ENV_VALUE_LENGTH || value.contains('\0') {
                return Err(invalid_flash_spec(format!(
                    "environment variable values must be at most {MAX_FLASH_ENV_VALUE_LENGTH} bytes and cannot contain NUL"
                )));
            }
        }
        validate_process_values("command", &self.command, MAX_FLASH_COMMAND_PARTS)?;
        validate_process_values("args", &self.args, MAX_FLASH_ARGS)?;
        let metadata_bytes = serde_json::to_vec(&self.metadata)
            .map_err(|_| invalid_flash_spec("metadata must be valid JSON"))?;
        if metadata_bytes.len() > MAX_FLASH_METADATA_BYTES {
            return Err(invalid_flash_spec(format!(
                "metadata must not exceed {MAX_FLASH_METADATA_BYTES} serialized bytes"
            )));
        }
        Ok(())
    }
}

const fn default_flash_ephemeral_storage_gib() -> u32 {
    DEFAULT_FLASH_EPHEMERAL_STORAGE_GIB
}

fn valid_flash_port_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_FLASH_PORT_NAME_LENGTH
        && name.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && name
            .as_bytes()
            .last()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn valid_flash_env_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_FLASH_ENV_KEY_LENGTH
        && name
            .as_bytes()
            .first()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn validate_process_values(
    field: &str,
    values: &[String],
    maximum_parts: usize,
) -> Result<(), DomainError> {
    if values.len() > maximum_parts {
        return Err(invalid_flash_spec(format!(
            "{field} must contain at most {maximum_parts} entries"
        )));
    }
    if values.iter().any(|value| {
        value.is_empty() || value.len() > MAX_FLASH_PROCESS_VALUE_LENGTH || value.contains('\0')
    }) {
        return Err(invalid_flash_spec(format!(
            "{field} entries must be non-empty, at most {MAX_FLASH_PROCESS_VALUE_LENGTH} bytes, and cannot contain NUL"
        )));
    }
    Ok(())
}

fn invalid_flash_spec(message: impl Into<String>) -> DomainError {
    DomainError::InvalidFlashSpec(message.into())
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceState {
    Provisioning,
    Ready,
    Updating,
    Deleting,
    Error,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ServiceInstance {
    pub id: ServiceInstanceId,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub provider: String,
    pub name: String,
    pub generation: i64,
    pub state: ServiceState,
    pub spec: Value,
    pub status: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Error)]
pub enum DomainError {
    #[error("invalid resource quota: {0}")]
    InvalidResourceQuota(String),

    #[error("invalid Flash spec: {0}")]
    InvalidFlashSpec(String),
    #[error("invalid policy: {0}")]
    InvalidPolicy(String),
    #[error("unsupported policy version: {0}")]
    UnsupportedPolicyVersion(String),
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        DEFAULT_FLOW_MAX_ROOMS, DEFAULT_FLOW_RATE_LIMIT_BURST,
        DEFAULT_FLOW_RATE_LIMIT_REQUESTS_PER_SECOND, FlashExposure, FlashExposureType, FlashPort,
        FlashProtocol, FlashSpec, FlashTrafficMode, FlowRateLimit, FlowSpec,
        MIN_FLASH_SERVICE_PORT, POLICY_VERSION, PolicyDocument, PolicyEffect, PolicyStatement,
    };

    #[test]
    fn flow_spec_requires_an_explicit_room_limit() {
        let missing = serde_json::from_value::<FlowSpec>(json!({
            "region": "heteronet-global",
            "max_participants": 100,
            "rate_limit": {
                "requests_per_second": DEFAULT_FLOW_RATE_LIMIT_REQUESTS_PER_SECOND,
                "burst": DEFAULT_FLOW_RATE_LIMIT_BURST
            },
            "metadata": {}
        }));
        assert!(missing.is_err());

        let spec = serde_json::from_value::<FlowSpec>(json!({
            "region": "heteronet-global",
            "max_participants": 100,
            "max_rooms": DEFAULT_FLOW_MAX_ROOMS,
            "rate_limit": {
                "requests_per_second": DEFAULT_FLOW_RATE_LIMIT_REQUESTS_PER_SECOND,
                "burst": DEFAULT_FLOW_RATE_LIMIT_BURST
            },
            "metadata": {}
        }));
        assert_eq!(spec.ok().map(|spec| spec.max_rooms), Some(100));
    }

    #[test]
    fn flow_rate_limit_is_explicit_and_structured() {
        let spec = serde_json::from_value::<FlowSpec>(json!({
            "region": "heteronet-global",
            "max_participants": 100,
            "max_rooms": DEFAULT_FLOW_MAX_ROOMS,
            "rate_limit": {
                "requests_per_second": 75,
                "burst": 150
            },
            "metadata": {}
        }));
        assert_eq!(
            spec.ok().map(|spec| spec.rate_limit),
            Some(FlowRateLimit {
                requests_per_second: 75,
                burst: 150,
            })
        );
    }

    #[test]
    fn flow_spec_rejects_removed_traffic_mode() {
        let spec = serde_json::from_value::<FlowSpec>(json!({
            "region": "heteronet-global",
            "traffic_mode": "forwarded",
            "max_participants": 100,
            "max_rooms": DEFAULT_FLOW_MAX_ROOMS,
            "rate_limit": {
                "requests_per_second": DEFAULT_FLOW_RATE_LIMIT_REQUESTS_PER_SECOND,
                "burst": DEFAULT_FLOW_RATE_LIMIT_BURST
            },
            "metadata": {}
        }));
        assert!(spec.is_err());
    }

    #[test]
    fn flow_spec_rejects_removed_turn_mode() {
        let spec = serde_json::from_value::<FlowSpec>(json!({
            "region": "heteronet-global",
            "max_participants": 100,
            "max_rooms": DEFAULT_FLOW_MAX_ROOMS,
            "rate_limit": {
                "requests_per_second": DEFAULT_FLOW_RATE_LIMIT_REQUESTS_PER_SECOND,
                "burst": DEFAULT_FLOW_RATE_LIMIT_BURST
            },
            "turn_enabled": true,
            "metadata": {}
        }));
        assert!(spec.is_err());
    }

    fn flash_spec() -> FlashSpec {
        FlashSpec {
            region: "heteronet-global".into(),
            image: "ghcr.io/example/game-server:v1".into(),
            replicas: 3,
            cpu_millis: 500,
            memory_mib: 512,
            ephemeral_storage_gib: 10,
            ports: vec![FlashPort {
                name: "game-udp".into(),
                protocol: FlashProtocol::Udp,
                container_port: 7777,
                service_port: MIN_FLASH_SERVICE_PORT,
            }],
            exposure: FlashExposure {
                exposure_type: FlashExposureType::Public,
                traffic_mode: FlashTrafficMode::Direct,
            },
            env: [("LOG_LEVEL".into(), "info".into())].into_iter().collect(),
            command: vec!["/app/server".into()],
            args: vec!["--port=7777".into()],
            metadata: [("team".into(), json!("simulation"))].into_iter().collect(),
        }
    }

    #[test]
    fn flash_spec_json_contract_is_exact_and_strict() -> Result<(), Box<dyn std::error::Error>> {
        let spec = flash_spec();
        spec.validate()?;
        let value = serde_json::to_value(&spec)?;
        assert_eq!(value["ports"][0]["protocol"], json!("udp"));
        assert_eq!(value["exposure"]["type"], json!("public"));
        assert_eq!(value["exposure"]["traffic_mode"], json!("direct"));
        assert_eq!(value["ephemeral_storage_gib"], json!(10));

        let mut unknown = value;
        unknown["runtime_class"] = json!("runc");
        assert!(serde_json::from_value::<FlashSpec>(unknown).is_err());
        Ok(())
    }

    #[test]
    fn flash_spec_defaults_and_bounds_ephemeral_storage() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut value = serde_json::to_value(flash_spec())?;
        value
            .as_object_mut()
            .ok_or("Flash spec must be an object")?
            .remove("ephemeral_storage_gib");
        let defaulted = serde_json::from_value::<FlashSpec>(value)?;
        assert_eq!(defaulted.ephemeral_storage_gib, 10);

        let mut oversized = flash_spec();
        oversized.ephemeral_storage_gib = 11;
        assert!(oversized.validate().is_err());
        Ok(())
    }

    #[test]
    fn flash_request_allows_server_assigned_service_port() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut value = serde_json::to_value(flash_spec())?;
        value["ports"][0]
            .as_object_mut()
            .ok_or("port must be an object")?
            .remove("service_port");
        let spec = serde_json::from_value::<FlashSpec>(value)?;
        spec.validate_request()?;
        assert!(spec.validate().is_err());
        Ok(())
    }

    #[test]
    fn flash_spec_rejects_unsafe_or_ambiguous_workloads() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut spec = flash_spec();
        spec.ports.push(spec.ports[0].clone());
        assert!(spec.validate().is_err());

        let mut spec = flash_spec();
        spec.image = "invalid image".into();
        assert!(spec.validate().is_err());

        let mut spec = flash_spec();
        spec.env.insert("INVALID-NAME".into(), "value".into());
        assert!(spec.validate().is_err());

        let mut spec = flash_spec();
        spec.exposure.exposure_type = FlashExposureType::Internal;
        spec.exposure.traffic_mode = FlashTrafficMode::Direct;
        assert!(spec.validate().is_err());

        let mut value = serde_json::to_value(flash_spec())?;
        value["metadata"] = json!([]);
        assert!(serde_json::from_value::<FlashSpec>(value).is_err());
        Ok(())
    }

    #[test]
    fn rejects_middle_wildcards() {
        let document = PolicyDocument {
            version: POLICY_VERSION.into(),
            statements: vec![PolicyStatement {
                effect: PolicyEffect::Allow,
                actions: vec!["project:*:read".into()],
                resources: vec!["hc:org:*".into()],
            }],
        };

        assert!(document.validate().is_err());
    }
}
