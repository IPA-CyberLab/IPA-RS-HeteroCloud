use heterocloud_domain::{OrganizationId, PolicyDocument, PolicyEffect};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

mod role_trust;
pub use role_trust::{RoleTrustDocument, RoleTrustError};

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
            let action_matches = if statement.not_actions.is_empty() {
                statement
                    .actions
                    .iter()
                    .any(|pattern| action_pattern_matches(pattern, request.action))
            } else {
                !statement
                    .not_actions
                    .iter()
                    .any(|pattern| action_pattern_matches(pattern, request.action))
            };
            let resource_matches = if statement.not_resources.is_empty() {
                statement
                    .resources
                    .iter()
                    .any(|pattern| pattern_matches(pattern, request.resource))
            } else {
                !statement
                    .not_resources
                    .iter()
                    .any(|pattern| pattern_matches(pattern, request.resource))
            };
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

/// An account administrator has an implicit administrator policy, but an
/// explicit deny in an attached policy must still take precedence. This is
/// different from an account root identity, which this service does not model.
pub fn authorize_with_administrator(
    request: &AuthorizationRequest<'_>,
    policies: &[PolicyDocument],
    administrator: bool,
) -> Result<Evaluation, IamError> {
    let mut evaluation = authorize(request, policies)?;
    if administrator && evaluation.reason == "default_deny" {
        evaluation.decision = Decision::Allow;
        evaluation.applicable_allow = true;
        evaluation.reason = "administrator_allow";
    }
    Ok(evaluation)
}

/// SCPs do not grant permissions. Every level must allow the request and an
/// explicit deny at any level takes precedence over identity permissions.
pub fn enforce_service_control_policies(
    request: &AuthorizationRequest<'_>,
    identity: Evaluation,
    levels: &[Vec<PolicyDocument>],
) -> Result<Evaluation, IamError> {
    if identity.decision == Decision::Deny {
        return Ok(identity);
    }
    let mut no_allow = false;
    for policies in levels {
        let evaluation = authorize(request, policies)?;
        if evaluation.applicable_deny {
            return Ok(Evaluation {
                decision: Decision::Deny,
                applicable_allow: identity.applicable_allow,
                applicable_deny: true,
                reason: "scp_explicit_deny",
            });
        }
        no_allow |= evaluation.decision == Decision::Deny;
    }
    if no_allow {
        return Ok(Evaluation {
            decision: Decision::Deny,
            applicable_allow: identity.applicable_allow,
            applicable_deny: false,
            reason: "scp_no_allow",
        });
    }
    Ok(identity)
}

/// A permissions boundary limits identity-based grants; it is not a grant by itself.
pub fn enforce_permissions_boundary(
    request: &AuthorizationRequest<'_>,
    identity: Evaluation,
    boundary: Option<&PolicyDocument>,
) -> Result<Evaluation, IamError> {
    let Some(boundary) = boundary else {
        return Ok(identity);
    };
    if identity.decision == Decision::Deny {
        return Ok(identity);
    }
    let evaluation = authorize(request, std::slice::from_ref(boundary))?;
    if evaluation.decision == Decision::Deny {
        return Ok(Evaluation {
            decision: Decision::Deny,
            applicable_allow: identity.applicable_allow,
            applicable_deny: evaluation.applicable_deny,
            reason: if evaluation.applicable_deny {
                "permissions_boundary_explicit_deny"
            } else {
                "permissions_boundary_no_allow"
            },
        });
    }
    Ok(identity)
}

pub fn enforce_session_policy(
    request: &AuthorizationRequest<'_>,
    identity: Evaluation,
    session_policy: Option<&PolicyDocument>,
) -> Result<Evaluation, IamError> {
    if identity.decision == Decision::Deny {
        return Ok(identity);
    }
    let mut evaluation = enforce_permissions_boundary(request, identity, session_policy)?;
    evaluation.reason = match evaluation.reason {
        "permissions_boundary_explicit_deny" => "session_policy_explicit_deny",
        "permissions_boundary_no_allow" => "session_policy_no_allow",
        reason => reason,
    };
    Ok(evaluation)
}

fn pattern_matches(pattern: &str, value: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let value: Vec<char> = value.chars().collect();
    let (mut p, mut v, mut star, mut retry) = (0, 0, None, 0);
    while v < value.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == value[v]) {
            p += 1;
            v += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some(p);
            p += 1;
            retry = v;
        } else if let Some(index) = star {
            retry += 1;
            v = retry;
            p = index + 1;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == '*' {
        p += 1;
    }
    p == pattern.len()
}

fn action_pattern_matches(pattern: &str, value: &str) -> bool {
    pattern_matches(&pattern.to_ascii_lowercase(), &value.to_ascii_lowercase())
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

    use super::{
        AuthorizationRequest, Decision, authorize, authorize_with_administrator, decide,
        enforce_permissions_boundary, enforce_service_control_policies,
    };

    fn policy(effect: PolicyEffect) -> PolicyDocument {
        PolicyDocument {
            version: POLICY_VERSION.into(),
            statements: vec![PolicyStatement {
                effect,
                sid: None,
                actions: vec!["project:*".into()],
                not_actions: vec![],
                resources: vec!["hc:org:".into()],
                not_resources: vec![],
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
    fn explicit_deny_overrides_administrator_policy() -> Result<(), Box<dyn std::error::Error>> {
        let account = OrganizationId::new();
        let request = AuthorizationRequest {
            principal_organization_id: account,
            resource_organization_id: account,
            action: "project:read",
            resource: "hc:org:example:project/demo",
        };
        let mut deny = policy(PolicyEffect::Deny);
        deny.statements[0].resources = vec!["hc:org:*".into()];
        let allowed = authorize_with_administrator(&request, &[], true)?;
        assert_eq!(allowed.decision, Decision::Allow);
        assert_eq!(allowed.reason, "administrator_allow");
        let denied = authorize_with_administrator(&request, &[deny], true)?;
        assert_eq!(denied.decision, Decision::Deny);
        assert_eq!(denied.reason, "explicit_deny");
        let other = AuthorizationRequest {
            resource_organization_id: OrganizationId::new(),
            ..request
        };
        assert_eq!(
            authorize_with_administrator(&other, &[], true)?.decision,
            Decision::Deny
        );
        Ok(())
    }

    #[test]
    fn scp_is_an_intersection_at_every_level_and_never_grants()
    -> Result<(), Box<dyn std::error::Error>> {
        let account = OrganizationId::new();
        let request = AuthorizationRequest {
            principal_organization_id: account,
            resource_organization_id: account,
            action: "project:read",
            resource: "hc:org:example:project/demo",
        };
        let full_access = PolicyDocument::from_aws_json(
            serde_json::json!({"Statement":{"Effect":"Allow","Action":"*","Resource":"*"}}),
        )?;
        let deny = PolicyDocument::from_aws_json(
            serde_json::json!({"Statement":{"Effect":"Deny","Action":"project:*","Resource":"*"}}),
        )?;
        let admin = authorize_with_administrator(&request, &[], true)?;
        let levels = vec![
            vec![full_access.clone()],
            vec![full_access.clone(), deny],
            vec![full_access.clone()],
        ];
        assert_eq!(
            enforce_service_control_policies(&request, admin.clone(), &levels)?.reason,
            "scp_explicit_deny"
        );
        assert_eq!(
            enforce_service_control_policies(
                &request,
                admin.clone(),
                &[vec![full_access.clone()], vec![]]
            )?
            .reason,
            "scp_no_allow"
        );
        assert_eq!(
            enforce_service_control_policies(
                &request,
                admin.clone(),
                &[vec![], levels[1].clone()]
            )?
            .reason,
            "scp_explicit_deny"
        );
        assert_eq!(
            enforce_service_control_policies(&request, admin, &[])?.decision,
            Decision::Allow
        );
        let no_identity_allow = authorize(&request, &[])?;
        assert_eq!(
            enforce_service_control_policies(&request, no_identity_allow, &[vec![full_access]])?
                .decision,
            Decision::Deny
        );
        Ok(())
    }

    #[test]
    fn permissions_boundary_caps_administrators_without_granting_access()
    -> Result<(), Box<dyn std::error::Error>> {
        let account = OrganizationId::new();
        let request = AuthorizationRequest {
            principal_organization_id: account,
            resource_organization_id: account,
            action: "project:read",
            resource: "hc:org:example:project/demo",
        };
        let allow = PolicyDocument::from_aws_json(
            serde_json::json!({"Statement":{"Effect":"Allow","Action":"project:read","Resource":"*"}}),
        )?;
        let other = PolicyDocument::from_aws_json(
            serde_json::json!({"Statement":{"Effect":"Allow","Action":"project:write","Resource":"*"}}),
        )?;
        let deny = PolicyDocument::from_aws_json(
            serde_json::json!({"Statement":{"Effect":"Deny","Action":"project:read","Resource":"*"}}),
        )?;
        let admin = authorize_with_administrator(&request, &[], true)?;
        assert_eq!(
            enforce_permissions_boundary(&request, admin.clone(), Some(&allow))?.decision,
            Decision::Allow
        );
        assert_eq!(
            enforce_permissions_boundary(&request, admin.clone(), Some(&other))?.reason,
            "permissions_boundary_no_allow"
        );
        assert_eq!(
            enforce_permissions_boundary(&request, admin, Some(&deny))?.reason,
            "permissions_boundary_explicit_deny"
        );
        assert_eq!(
            enforce_permissions_boundary(&request, authorize(&request, &[])?, Some(&allow))?
                .decision,
            Decision::Deny
        );
        Ok(())
    }

    #[test]
    fn aws_identity_policy_supports_not_and_wildcard_semantics()
    -> Result<(), Box<dyn std::error::Error>> {
        let policy = PolicyDocument::from_aws_json(serde_json::json!({
            "Version": "2012-10-17",
            "Statement": [
                {"Effect": "Allow", "Action": "PROJECT:GET*", "Resource": "hc:org:*:project/te?t"},
                {"Effect": "Deny", "NotAction": "project:GetSafe", "NotResource": "hc:org:*:project/test"}
            ]
        }))?;
        let account = OrganizationId::new();
        let request = AuthorizationRequest {
            principal_organization_id: account,
            resource_organization_id: account,
            action: "project:GetThing",
            resource: "hc:org:one:project/test",
        };
        assert_eq!(
            authorize(&request, std::slice::from_ref(&policy))?.decision,
            Decision::Allow
        );
        let denied = AuthorizationRequest {
            resource: "hc:org:one:project/text",
            ..request.clone()
        };
        assert_eq!(authorize(&denied, &[policy])?.reason, "explicit_deny");
        Ok(())
    }

    #[test]
    fn aws_unsupported_conditions_and_principals_are_rejected()
    -> Result<(), Box<dyn std::error::Error>> {
        for extra in [
            serde_json::json!({"Condition":{"StringEquals":{"aws:SourceIp":"127.0.0.1"}}}),
            serde_json::json!({"Principal":"*"}),
        ] {
            let mut statement = serde_json::json!({"Effect":"Allow", "Action":"*", "Resource":"*"});
            statement
                .as_object_mut()
                .ok_or("statement is not an object")?
                .extend(extra.as_object().ok_or("extra is not an object")?.clone());
            assert!(
                PolicyDocument::from_aws_json(
                    serde_json::json!({"Version":"2012-10-17", "Statement":statement})
                )
                .is_err()
            );
        }
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
                    sid: None,
                    actions: vec!["*".into()],
                    not_actions: vec![],
                    resources: vec!["*".into()],
                    not_resources: vec![],
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
