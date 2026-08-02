use chrono::{Duration, Utc};
use heterocloud_domain::{OrganizationId, PrincipalId, ProjectId, ServiceInstanceId};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

pub const PROVIDER_TOKEN_TTL_SECONDS: i64 = 60;
pub const PRINCIPAL_CONTEXT_REVOKE_ACTION: &str = "principal-context.revoke";
pub const PRINCIPAL_CONTEXT_REVOCATION_GRACE_SECONDS: i64 = 15;
pub type PrincipalContextId = Uuid;
const _: () = assert!(PROVIDER_TOKEN_TTL_SECONDS <= 60);

pub struct ProviderSigner {
    issuer: String,
    audience: String,
    key_id: String,
    key: EncodingKey,
}

impl ProviderSigner {
    pub fn from_ed25519_pem(
        issuer: impl Into<String>,
        audience: impl Into<String>,
        key_id: impl Into<String>,
        pem: &[u8],
    ) -> Result<Self, ProviderError> {
        Ok(Self {
            issuer: issuer.into(),
            audience: audience.into(),
            key_id: key_id.into(),
            key: EncodingKey::from_ed_pem(pem)?,
        })
    }

    pub fn sign(&self, context: ProviderContext) -> Result<SignedProviderContext, ProviderError> {
        let now = Utc::now();
        let claims = ProviderClaims {
            issuer: self.issuer.clone(),
            audience: self.audience.clone(),
            subject: context.principal_id.to_string(),
            organization_id: context.organization_id,
            project_id: context.project_id,
            service_instance_id: context.service_instance_id,
            action: context.action,
            generation: context.generation,
            jwt_id: Uuid::now_v7(),
            issued_at: now.timestamp(),
            not_before: now.timestamp() - 5,
            expires_at: (now + Duration::seconds(PROVIDER_TOKEN_TTL_SECONDS)).timestamp(),
        };
        let mut header = Header::new(Algorithm::EdDSA);
        header.kid = Some(self.key_id.clone());
        let token = jsonwebtoken::encode(&header, &claims, &self.key)?;
        Ok(SignedProviderContext { token, claims })
    }
}

#[derive(Clone, Debug)]
pub struct ProviderContext {
    pub principal_id: PrincipalId,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub service_instance_id: ServiceInstanceId,
    pub action: String,
    pub generation: i64,
}

#[derive(Clone, Debug)]
pub struct SignedProviderContext {
    pub token: String,
    pub claims: ProviderClaims,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProviderClaims {
    #[serde(rename = "iss")]
    pub issuer: String,
    #[serde(rename = "aud")]
    pub audience: String,
    #[serde(rename = "sub")]
    pub subject: String,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub service_instance_id: ServiceInstanceId,
    pub action: String,
    pub generation: i64,
    #[serde(rename = "jti")]
    pub jwt_id: Uuid,
    #[serde(rename = "iat")]
    pub issued_at: i64,
    #[serde(rename = "nbf")]
    pub not_before: i64,
    #[serde(rename = "exp")]
    pub expires_at: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReconcileRequest {
    pub generation: i64,
    pub name: String,
    pub spec: Value,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PrincipalContextRevocationRequest {
    pub expires_at: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AcceptedOperation {
    pub operation_id: Uuid,
    pub status: Value,
}

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("provider signing failed: {0}")]
    Jwt(#[from] jsonwebtoken::errors::Error),
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use uuid::Uuid;

    use super::{
        AcceptedOperation, PRINCIPAL_CONTEXT_REVOCATION_GRACE_SECONDS,
        PRINCIPAL_CONTEXT_REVOKE_ACTION, PrincipalContextRevocationRequest,
    };

    #[test]
    fn accepted_operation_requires_provider_status() -> Result<(), Box<dyn std::error::Error>> {
        let operation_id = Uuid::from_u128(1);
        let operation: AcceptedOperation = serde_json::from_value(json!({
            "operation_id": operation_id,
            "status": {
                "phase": "ready",
                "observed_generation": 1
            }
        }))?;
        assert_eq!(operation.operation_id, operation_id);
        assert_eq!(operation.status["phase"], json!("ready"));
        assert!(
            serde_json::from_value::<AcceptedOperation>(json!({
                "operation_id": operation_id
            }))
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn principal_context_revocation_contract_is_stable() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(PRINCIPAL_CONTEXT_REVOKE_ACTION, "principal-context.revoke");
        assert_eq!(PRINCIPAL_CONTEXT_REVOCATION_GRACE_SECONDS, 15);
        let payload = PrincipalContextRevocationRequest {
            expires_at: 1_785_480_300,
        };
        assert_eq!(
            serde_json::to_value(payload)?,
            json!({"expires_at": 1_785_480_300_i64})
        );
        assert!(
            serde_json::from_value::<PrincipalContextRevocationRequest>(json!({
                "expires_at": 1_785_480_300_i64,
                "context_id": Uuid::nil()
            }))
            .is_err()
        );
        Ok(())
    }
}
