use heterocloud_domain::{OrganizationId, PolicyDocument, PolicyEffect};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const LEAN_SEMANTICS_SOURCE: &str = include_str!("../../../lean/HeteroCloud/IAM.lean");

#[must_use]
pub fn semantics_digest() -> String {
    let digest = Sha256::digest(LEAN_SEMANTICS_SOURCE.as_bytes());
    format!("{digest:x}")
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Allow,
    Deny,
}

#[derive(Clone, Debug)]
pub struct AuthorizationRequest<'a> {
    pub principal_organization_id: OrganizationId,
    pub resource_organization_id: OrganizationId,
    pub action: &'a str,
    pub resource: &'a str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Evaluation {
    pub decision: Decision,
    pub applicable_allow: bool,
    pub applicable_deny: bool,
    pub reason: &'static str,
}

#[must_use]
pub const fn decide(
    same_organization: bool,
    applicable_allow: bool,
    applicable_deny: bool,
) -> Decision {
    if !same_organization || applicable_deny {
        Decision::Deny
    } else if applicable_allow {
        Decision::Allow
    } else {
        Decision::Deny
    }
}

pub fn authorize(
    request: &AuthorizationRequest<'_>,
    policies: &[PolicyDocument],
) -> Result<Evaluation, IamError> {
    let same_organization = request.principal_organization_id == request.resource_organization_id;
    if !same_organization {
        return Ok(Evaluation {
            decision: Decision::Deny,
            applicable_allow: false,
            applicable_deny: false,
            reason: "cross_organization",
        });
    }

    let mut applicable_allow = false;
    let mut applicable_deny = false;
    for policy in policies {
        policy.validate()?;
        for statement in &policy.statements {
            let action_matches = statement
                .actions
                .iter()
                .any(|pattern| pattern_matches(pattern, request.action));
            let resource_matches = statement
                .resources
                .iter()
                .any(|pattern| pattern_matches(pattern, request.resource));
            if !action_matches || !resource_matches {
                continue;
            }
            match statement.effect {
                PolicyEffect::Allow => applicable_allow = true,
                PolicyEffect::Deny => applicable_deny = true,
            }
        }
    }

    let decision = decide(same_organization, applicable_allow, applicable_deny);
    if applicable_deny {
        return Ok(Evaluation {
            decision,
            applicable_allow,
            applicable_deny,
            reason: "explicit_deny",
        });
    }
    if applicable_allow {
        return Ok(Evaluation {
            decision,
            applicable_allow,
            applicable_deny,
            reason: "explicit_allow",
        });
    }
    Ok(Evaluation {
        decision,
        applicable_allow,
        applicable_deny,
        reason: "default_deny",
    })
}

fn pattern_matches(pattern: &str, value: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => value.starts_with(prefix),
        None => pattern == value,
    }
}

#[derive(Debug, Error)]
pub enum IamError {
    #[error(transparent)]
    InvalidPolicy(#[from] heterocloud_domain::DomainError),
}

#[cfg(test)]
mod tests {
    use heterocloud_domain::{
        OrganizationId, POLICY_VERSION, PolicyDocument, PolicyEffect, PolicyStatement,
    };

    use super::{AuthorizationRequest, Decision, authorize, decide};

    fn policy(effect: PolicyEffect) -> PolicyDocument {
        PolicyDocument {
            version: POLICY_VERSION.into(),
            statements: vec![PolicyStatement {
                effect,
                actions: vec!["project:*".into()],
                resources: vec!["hc:org:".into()],
            }],
        }
    }

    #[test]
    fn default_is_deny() -> Result<(), Box<dyn std::error::Error>> {
        let organization_id = OrganizationId::new();
        let evaluation = authorize(
            &AuthorizationRequest {
                principal_organization_id: organization_id,
                resource_organization_id: organization_id,
                action: "project:read",
                resource: "hc:org:example:project/demo",
            },
            &[],
        )?;

        assert_eq!(evaluation.decision, Decision::Deny);
        assert_eq!(evaluation.reason, "default_deny");
        Ok(())
    }

    #[test]
    fn deny_overrides_allow() -> Result<(), Box<dyn std::error::Error>> {
        let organization_id = OrganizationId::new();
        let mut allow = policy(PolicyEffect::Allow);
        allow.statements[0].resources = vec!["hc:org:*".into()];
        let mut deny = policy(PolicyEffect::Deny);
        deny.statements[0].resources = vec!["hc:org:*".into()];
        let evaluation = authorize(
            &AuthorizationRequest {
                principal_organization_id: organization_id,
                resource_organization_id: organization_id,
                action: "project:read",
                resource: "hc:org:example:project/demo",
            },
            &[allow, deny],
        )?;

        assert_eq!(evaluation.decision, Decision::Deny);
        assert_eq!(evaluation.reason, "explicit_deny");
        Ok(())
    }

    #[test]
    fn tenant_boundary_precedes_policy() -> Result<(), Box<dyn std::error::Error>> {
        let evaluation = authorize(
            &AuthorizationRequest {
                principal_organization_id: OrganizationId::new(),
                resource_organization_id: OrganizationId::new(),
                action: "project:read",
                resource: "hc:org:other:project/demo",
            },
            &[PolicyDocument {
                version: POLICY_VERSION.into(),
                statements: vec![PolicyStatement {
                    effect: PolicyEffect::Allow,
                    actions: vec!["*".into()],
                    resources: vec!["*".into()],
                }],
            }],
        )?;

        assert_eq!(evaluation.decision, Decision::Deny);
        assert_eq!(evaluation.reason, "cross_organization");
        Ok(())
    }

    #[test]
    fn rust_truth_table_matches_the_lean_kernel() {
        let cases = [
            (false, false, false, Decision::Deny),
            (false, false, true, Decision::Deny),
            (false, true, false, Decision::Deny),
            (false, true, true, Decision::Deny),
            (true, false, false, Decision::Deny),
            (true, false, true, Decision::Deny),
            (true, true, false, Decision::Allow),
            (true, true, true, Decision::Deny),
        ];
        for (same_organization, allow, deny, expected) in cases {
            assert_eq!(decide(same_organization, allow, deny), expected);
        }
    }
}
