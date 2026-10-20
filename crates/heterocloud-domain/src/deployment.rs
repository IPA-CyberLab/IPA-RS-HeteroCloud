//! Deployment policy shared by the API, CLI and provider protocol.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeploymentPolicy {
    #[serde(default = "default_strategy")]
    pub strategy: String,
    /// Do not mount a persistent home. Existing disks remain retained.
    #[serde(default)]
    pub stateless: bool,
    #[serde(default = "default_deadline")]
    pub deadline_seconds: u32,
    #[serde(default = "default_rollback")]
    pub auto_rollback: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health_check: Option<HealthCheck>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HealthCheck {
    pub port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

fn default_strategy() -> String {
    "auto".into()
}
fn default_deadline() -> u32 {
    600
}
fn default_rollback() -> bool {
    true
}

impl Default for DeploymentPolicy {
    fn default() -> Self {
        Self {
            strategy: default_strategy(),
            stateless: false,
            deadline_seconds: default_deadline(),
            auto_rollback: true,
            health_check: None,
        }
    }
}

impl DeploymentPolicy {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !matches!(self.strategy.as_str(), "auto" | "rolling" | "recreate") {
            return Err("deployment strategy must be auto, rolling, or recreate");
        }
        if self.strategy == "rolling" && !self.stateless {
            return Err(
                "rolling deployments require stateless=true; persistent homes are retained without being mounted",
            );
        }
        if !(60..=3600).contains(&self.deadline_seconds) {
            return Err("deployment deadline must be between 60 and 3600 seconds");
        }
        if let Some(check) = &self.health_check
            && (check.port == 0
                || check.path.as_ref().is_some_and(|p| {
                    !p.starts_with('/') || p.len() > 1024 || p.chars().any(char::is_control)
                }))
        {
            return Err("health check requires a nonzero port and an absolute HTTP path");
        }
        Ok(())
    }
}
