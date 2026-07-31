use std::collections::BTreeSet;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use heterocloud_domain::{OrganizationId, PrincipalId, ProjectId, ServiceInstanceId};
use hmac::{Hmac, Mac};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use thiserror::Error;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
pub struct FlowAccessSigner {
    issuer: String,
    audience: String,
    secret: SecretString,
}

impl FlowAccessSigner {
    pub fn new(
        issuer: impl Into<String>,
        audience: impl Into<String>,
        secret: SecretString,
    ) -> Result<Self, FlowAccessError> {
        let issuer = issuer.into();
        let audience = audience.into();
        if issuer.is_empty() || issuer.chars().any(char::is_whitespace) {
            return Err(FlowAccessError::InvalidIssuer);
        }
        if audience.is_empty() || audience.chars().any(char::is_whitespace) {
            return Err(FlowAccessError::InvalidAudience);
        }
        if secret.expose_secret().len() < 32 {
            return Err(FlowAccessError::WeakSecret);
        }
        Ok(Self {
            issuer,
            audience,
            secret,
        })
    }

    pub fn sign(
        &self,
        input: FlowAccessInput,
        issued_at: u64,
        expires_at: u64,
        context_id: Uuid,
    ) -> Result<SignedFlowAccessContext, FlowAccessError> {
        if expires_at <= issued_at {
            return Err(FlowAccessError::InvalidLifetime);
        }
        let context = FlowPrincipalContext {
            issuer: self.issuer.clone(),
            audience: self.audience.clone(),
            organization_id: input.organization_id,
            project_id: input.project_id,
            service_instance_id: input.service_instance_id,
            principal_id: input.principal_id,
            permissions: input.permissions,
            issued_at,
            expires_at,
            context_id,
        };
        let encoded = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&context)?);
        let timestamp = issued_at.to_string();
        let mut mac = HmacSha256::new_from_slice(self.secret.expose_secret().as_bytes())
            .map_err(|_| FlowAccessError::InvalidSecret)?;
        mac.update(timestamp.as_bytes());
        mac.update(b".");
        mac.update(encoded.as_bytes());
        let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
        Ok(SignedFlowAccessContext {
            context,
            encoded,
            timestamp,
            signature,
        })
    }
}

#[derive(Clone, Debug)]
pub struct FlowAccessInput {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub service_instance_id: ServiceInstanceId,
    pub principal_id: PrincipalId,
    pub permissions: BTreeSet<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FlowPrincipalContext {
    pub issuer: String,
    pub audience: String,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub service_instance_id: ServiceInstanceId,
    pub principal_id: PrincipalId,
    pub permissions: BTreeSet<String>,
    pub issued_at: u64,
    pub expires_at: u64,
    pub context_id: Uuid,
}

pub struct SignedFlowAccessContext {
    pub context: FlowPrincipalContext,
    pub encoded: String,
    pub timestamp: String,
    pub signature: String,
}

#[derive(Debug, Error)]
pub enum FlowAccessError {
    #[error("Flow access audience must be a nonempty token")]
    InvalidAudience,
    #[error("Flow access issuer must be a nonempty token")]
    InvalidIssuer,
    #[error("Flow access context expiry must be after issuance")]
    InvalidLifetime,
    #[error("Flow access HMAC secret is invalid")]
    InvalidSecret,
    #[error("Flow access context serialization failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Flow access HMAC secret must contain at least 32 bytes")]
    WeakSecret,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use heterocloud_domain::{OrganizationId, PrincipalId, ProjectId, ServiceInstanceId};
    use hmac::{Hmac, Mac};
    use secrecy::SecretString;
    use sha2::Sha256;
    use uuid::Uuid;

    use super::{FlowAccessInput, FlowAccessSigner, FlowPrincipalContext};

    #[test]
    fn signature_and_canonical_permission_order_are_deterministic()
    -> Result<(), Box<dyn std::error::Error>> {
        let signer = FlowAccessSigner::new(
            "heterocloud",
            "heterocloud-flow-data",
            SecretString::from("0123456789abcdef0123456789abcdef".to_owned()),
        )?;
        let permissions = BTreeSet::from([
            "flow.turn.issue".to_owned(),
            "flow.room.join".to_owned(),
            "flow.room.read".to_owned(),
        ]);
        let signed = signer.sign(
            FlowAccessInput {
                organization_id: OrganizationId(Uuid::parse_str(
                    "00000000-0000-0000-0000-000000000001",
                )?),
                project_id: ProjectId(Uuid::parse_str("00000000-0000-0000-0000-000000000002")?),
                service_instance_id: ServiceInstanceId(Uuid::parse_str(
                    "00000000-0000-0000-0000-000000000003",
                )?),
                principal_id: PrincipalId(Uuid::parse_str("00000000-0000-0000-0000-000000000004")?),
                permissions,
            },
            1_785_480_000,
            1_785_480_300,
            Uuid::parse_str("00000000-0000-0000-0000-000000000005")?,
        )?;

        assert_eq!(signed.timestamp, "1785480000");
        assert_eq!(
            signed.signature,
            "iwjXlHs367L10EDfAVRPXuSY5X4gj85Fd4I7q0k5Nxo"
        );
        let mut verifier = Hmac::<Sha256>::new_from_slice(b"0123456789abcdef0123456789abcdef")?;
        verifier.update(signed.timestamp.as_bytes());
        verifier.update(b".");
        verifier.update(signed.encoded.as_bytes());
        verifier.verify_slice(&URL_SAFE_NO_PAD.decode(&signed.signature)?)?;
        let decoded: FlowPrincipalContext =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(&signed.encoded)?)?;
        assert_eq!(decoded, signed.context);
        assert_eq!(
            decoded.permissions.into_iter().collect::<Vec<_>>(),
            ["flow.room.join", "flow.room.read", "flow.turn.issue"]
        );
        Ok(())
    }
}
