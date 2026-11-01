use heterocloud_domain::PolicyEffect;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::action_pattern_matches;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
enum OneOrMany<T> {
    One(T),
    Many(Vec<T>),
}
impl<T> OneOrMany<T> {
    fn items(&self) -> &[T] {
        match self {
            Self::One(item) => std::slice::from_ref(item),
            Self::Many(items) => items,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
enum TrustPrincipal {
    Wildcard(String),
    Accounts(TrustAccounts),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TrustAccounts {
    #[serde(rename = "AWS")]
    aws: OneOrMany<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TrustStatement {
    #[serde(rename = "Sid", skip_serializing_if = "Option::is_none")]
    sid: Option<String>,
    #[serde(rename = "Effect")]
    effect: PolicyEffect,
    #[serde(rename = "Action")]
    action: OneOrMany<String>,
    #[serde(rename = "Principal")]
    principal: TrustPrincipal,
}

/// A deliberately restricted AWS role trust-policy grammar. Unsupported
/// fields are rejected, never ignored. The authenticated caller must also have
/// an identity-based sts:AssumeRole grant in the source account.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RoleTrustDocument {
    #[serde(rename = "Version", skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(rename = "Statement")]
    statements: OneOrMany<TrustStatement>,
}

#[derive(Debug, Error)]
pub enum RoleTrustError {
    #[error("invalid role trust policy: {0}")]
    Invalid(&'static str),
}

impl RoleTrustDocument {
    pub fn validate(&self) -> Result<(), RoleTrustError> {
        if self
            .version
            .as_deref()
            .is_some_and(|v| v != "2012-10-17" && v != "2008-10-17")
        {
            return Err(RoleTrustError::Invalid("unsupported Version"));
        }
        if self.statements.items().is_empty() || self.statements.items().len() > 128 {
            return Err(RoleTrustError::Invalid("expected 1..128 statements"));
        }
        for statement in self.statements.items() {
            if statement.action.items().is_empty()
                || statement.action.items().len() > 128
                || statement.action.items().iter().any(|action| {
                    action.is_empty()
                        || action.len() > 128
                        || action.chars().any(char::is_whitespace)
                })
            {
                return Err(RoleTrustError::Invalid("invalid Action"));
            }
            let principals = match &statement.principal {
                TrustPrincipal::Wildcard(value) if value == "*" => continue,
                TrustPrincipal::Wildcard(_) => {
                    return Err(RoleTrustError::Invalid("Principal string must be *"));
                }
                TrustPrincipal::Accounts(accounts) => accounts.aws.items(),
            };
            if principals.is_empty()
                || principals.len() > 128
                || principals.iter().any(|p| !valid_principal(p))
            {
                return Err(RoleTrustError::Invalid("unsupported AWS Principal"));
            }
        }
        Ok(())
    }

    /// Match a caller's account ID and canonical IAM principal ARN. Explicit
    /// deny takes precedence, including account-wide root delegation.
    pub fn permits(&self, caller_account: &str, caller_arn: &str) -> Result<bool, RoleTrustError> {
        self.validate()?;
        let mut allowed = false;
        for statement in self.statements.items() {
            if !statement
                .action
                .items()
                .iter()
                .any(|action| action_pattern_matches(action, "sts:AssumeRole"))
            {
                continue;
            }
            let matches = match &statement.principal {
                TrustPrincipal::Wildcard(value) => value == "*",
                TrustPrincipal::Accounts(accounts) => accounts.aws.items().iter().any(|value| {
                    value == "*"
                        || value == caller_arn
                        || value == caller_account
                        || value == &format!("arn:aws:iam::{caller_account}:root")
                }),
            };
            if matches {
                match statement.effect {
                    PolicyEffect::Deny => return Ok(false),
                    PolicyEffect::Allow => allowed = true,
                }
            }
        }
        Ok(allowed)
    }
}

fn valid_principal(value: &str) -> bool {
    if value == "*" {
        return true;
    }
    if value.len() == 12 && value.bytes().all(|b| b.is_ascii_digit()) {
        return true;
    }
    let Some(rest) = value.strip_prefix("arn:aws:iam::") else {
        return false;
    };
    let Some((account, path)) = rest.split_once(':') else {
        return false;
    };
    if account.len() != 12 || !account.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    if path == "root" {
        return true;
    }
    let Some(name) = path
        .strip_prefix("user/")
        .or_else(|| path.strip_prefix("role/"))
    else {
        return false;
    };
    !name.is_empty()
        && name.len() <= 256
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"+=,.@_-/".contains(&b))
}

#[cfg(test)]
mod tests {
    use super::RoleTrustDocument;
    use serde_json::json;

    #[test]
    fn trust_requires_match_and_explicit_deny_wins() -> Result<(), Box<dyn std::error::Error>> {
        let trust: RoleTrustDocument = serde_json::from_value(
            json!({"Version":"2012-10-17","Statement":[
                {"Effect":"Allow","Action":"sts:AssumeRole","Principal":{"AWS":"arn:aws:iam::111111111111:root"}},
                {"Effect":"Deny","Action":"STS:ASSUMEROLE","Principal":{"AWS":"arn:aws:iam::111111111111:user/blocked"}}
            ]}),
        )?;
        assert!(trust.permits("111111111111", "arn:aws:iam::111111111111:user/allowed")?);
        assert!(!trust.permits("111111111111", "arn:aws:iam::111111111111:user/blocked")?);
        assert!(!trust.permits("222222222222", "arn:aws:iam::222222222222:user/allowed")?);
        Ok(())
    }

    #[test]
    fn unsupported_trust_conditions_and_partial_principal_wildcards_fail_closed()
    -> Result<(), Box<dyn std::error::Error>> {
        assert!(serde_json::from_value::<RoleTrustDocument>(json!({"Statement":{"Effect":"Allow","Action":"sts:AssumeRole","Principal":"*","Condition":{"Bool":{"aws:MultiFactorAuthPresent":"true"}}}})).is_err());
        let trust: RoleTrustDocument = serde_json::from_value(
            json!({"Statement":{"Effect":"Allow","Action":"sts:AssumeRole","Principal":{"AWS":"arn:aws:iam::111111111111:user/*"}}}),
        )?;
        assert!(trust.validate().is_err());
        Ok(())
    }
}
