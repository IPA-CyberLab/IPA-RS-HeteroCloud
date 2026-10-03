use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{DomainError, FlashProtocol};

pub const MAX_VPC_SECURITY_GROUPS: usize = 32;
pub const MAX_VPC_RULES: usize = 128;
pub const MAX_FLASH_SECURITY_GROUPS: usize = 8;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VpcSpec {
    pub region: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub nat: VpcNat,
    #[serde(default = "default_security_groups")]
    pub security_groups: Vec<String>,
    #[serde(default)]
    pub rules: Vec<VpcRule>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VpcNat {
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VpcRule {
    #[serde(default)]
    pub description: String,
    pub source: VpcPeer,
    pub destination: VpcPeer,
    pub protocol: FlashProtocol,
    pub port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_port: Option<u16>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum VpcPeer {
    SecurityGroup {
        name: String,
    },
    Service {
        service_id: Uuid,
    },
    /// An explicit grant to every attached service in this VPC, never another VPC.
    All,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FlashVpcAttachment {
    pub vpc_id: Uuid,
    #[serde(default = "default_security_groups")]
    pub security_groups: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_name: Option<String>,
}

pub fn default_security_groups() -> Vec<String> {
    vec!["default".into()]
}

pub fn valid_private_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 63
        && value.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && value
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

impl VpcSpec {
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.region.is_empty() || self.region.len() > 128 {
            return Err(invalid("region is required and must be at most 128 bytes"));
        }
        if self.description.len() > 2_048 {
            return Err(invalid("description must be at most 2048 bytes"));
        }
        validate_groups(&self.security_groups, MAX_VPC_SECURITY_GROUPS)?;
        if self.rules.len() > MAX_VPC_RULES {
            return Err(invalid("a VPC may have at most 128 connection rules"));
        }
        for rule in &self.rules {
            if rule.description.len() > 256 {
                return Err(invalid("rule description must be at most 256 bytes"));
            }
            if rule.port == 0 || rule.end_port.is_some_and(|end| end < rule.port) {
                return Err(invalid(
                    "rule ports must be within 1..65535 in ascending order",
                ));
            }
            for peer in [&rule.source, &rule.destination] {
                match peer {
                    VpcPeer::SecurityGroup { name } if !self.security_groups.contains(name) => {
                        return Err(invalid("rule references an unknown security group"));
                    }
                    VpcPeer::Service { service_id } if service_id.is_nil() => {
                        return Err(invalid("rule service_id must be a non-nil UUID"));
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
}

impl FlashVpcAttachment {
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.vpc_id.is_nil() {
            return Err(invalid("vpc_id must be a non-nil UUID"));
        }
        validate_groups(&self.security_groups, MAX_FLASH_SECURITY_GROUPS)?;
        if self
            .private_name
            .as_deref()
            .is_some_and(|name| !valid_private_name(name))
        {
            return Err(invalid(
                "private_name must be a lowercase DNS label starting with a letter",
            ));
        }
        Ok(())
    }
}

fn validate_groups(groups: &[String], maximum: usize) -> Result<(), DomainError> {
    if groups.is_empty() || groups.len() > maximum {
        return Err(invalid(format!(
            "security_groups must contain 1..{maximum} names"
        )));
    }
    let mut seen = BTreeSet::new();
    if groups
        .iter()
        .any(|name| !valid_private_name(name) || !seen.insert(name))
    {
        return Err(invalid(
            "security group names must be unique lowercase DNS labels",
        ));
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> DomainError {
    DomainError::InvalidVpcSpec(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn vpc_defaults_are_isolated_and_have_no_nat() -> Result<(), Box<dyn std::error::Error>> {
        let spec: VpcSpec = serde_json::from_value(json!({"region": "example-region"}))?;
        spec.validate()?;
        assert!(!spec.nat.enabled);
        assert!(spec.rules.is_empty());
        assert_eq!(spec.security_groups, ["default"]);
        Ok(())
    }

    #[test]
    fn rules_reject_unknown_groups_and_invalid_port_ranges()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut spec: VpcSpec = serde_json::from_value(json!({
            "region": "example-region", "security_groups": ["coder", "workspaces"],
            "rules": [{"source": {"type": "security_group", "name": "coder"},
                "destination": {"type": "security_group", "name": "workspaces"},
                "protocol": "tcp", "port": 22}]
        }))?;
        spec.validate()?;
        spec.rules[0].end_port = Some(21);
        assert!(spec.validate().is_err());
        spec.rules[0].end_port = Some(22);
        spec.security_groups.pop();
        assert!(spec.validate().is_err());
        Ok(())
    }

    #[test]
    fn attachment_rejects_ambiguous_names_and_unknown_fields()
    -> Result<(), Box<dyn std::error::Error>> {
        for name in [
            "-workspace",
            "workspace.",
            "a/b",
            "Workspace",
            "a..b",
            "",
            "xn--",
        ] {
            let attachment = FlashVpcAttachment {
                vpc_id: Uuid::from_u128(1),
                security_groups: vec!["default".into()],
                private_name: Some(name.into()),
            };
            assert!(attachment.validate().is_err(), "accepted {name}");
        }
        assert!(
            serde_json::from_value::<FlashVpcAttachment>(json!({
                "vpc_id": Uuid::from_u128(1), "host_network": true
            }))
            .is_err()
        );
        Ok(())
    }
}
