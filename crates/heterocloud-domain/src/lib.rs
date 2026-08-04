use std::fmt;

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
        DEFAULT_FLOW_RATE_LIMIT_REQUESTS_PER_SECOND, FlowRateLimit, FlowSpec, POLICY_VERSION,
        PolicyDocument, PolicyEffect, PolicyStatement,
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
