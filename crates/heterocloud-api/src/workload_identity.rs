//! Verify a rotating, Pod-bound Kubernetes identity, never a caller-selected role.
use reqwest::{Certificate, Client};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::HashMap, time::Duration};
use tokio::sync::Semaphore;
use uuid::Uuid;

pub const AUDIENCE: &str = "heterocloud-workload";
const TOKEN_FILE: &str = "/var/run/secrets/heterocloud-kubernetes/token";

pub struct WorkloadIdentity {
    http: Client,
    origin: url::Url,
    namespace: String,
    gate: Semaphore,
}

#[derive(Debug, thiserror::Error)]
#[error("Workload identity verification failed")]
pub struct IdentityError;

pub struct VerifiedWorkload {
    pub service_id: Uuid,
    pub pod_uid: Uuid,
    pub task_role: Uuid,
}

#[derive(Deserialize)]
struct Review {
    status: Option<ReviewStatus>,
}
#[derive(Deserialize)]
struct ReviewStatus {
    #[serde(default)]
    authenticated: bool,
    #[serde(default)]
    audiences: Vec<String>,
    user: Option<ReviewUser>,
}
#[derive(Deserialize)]
struct ReviewUser {
    username: String,
    #[serde(default)]
    extra: HashMap<String, Vec<String>>,
}

impl WorkloadIdentity {
    pub async fn from_environment() -> Result<Option<Self>, Box<dyn std::error::Error>> {
        if std::env::var("HETEROCLOUD_WORKLOAD_IDENTITY_ENABLED").as_deref() != Ok("true") {
            return Ok(None);
        }
        let host = std::env::var("KUBERNETES_SERVICE_HOST")?;
        let port = std::env::var("KUBERNETES_SERVICE_PORT_HTTPS").unwrap_or_else(|_| "443".into());
        let ca = tokio::fs::read("/var/run/secrets/heterocloud-kubernetes/ca.crt").await?;
        let http = Client::builder()
            .add_root_certificate(Certificate::from_pem(&ca)?)
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(5))
            .build()?;
        let host = if host.contains(':') {
            format!("[{host}]")
        } else {
            host
        };
        let origin = url::Url::parse(&format!("https://{host}:{port}/"))?;
        let namespace = std::env::var("HETEROCLOUD_WORKLOAD_NAMESPACE")?;
        Ok(Some(Self {
            http,
            origin,
            namespace,
            gate: Semaphore::new(8),
        }))
    }

    pub async fn verify(&self, subject_token: &str) -> Result<VerifiedWorkload, IdentityError> {
        if subject_token.is_empty() || subject_token.len() > 16384 {
            return Err(IdentityError);
        }
        let _permit = self.gate.try_acquire().map_err(|_| IdentityError)?;
        // Read on every exchange: Kubernetes rotates this API identity as well.
        let api_token = tokio::fs::read_to_string(TOKEN_FILE)
            .await
            .map_err(|_| IdentityError)?;
        let response = self
            .http
            .post(
                self.origin
                    .join("apis/authentication.k8s.io/v1/tokenreviews")
                    .map_err(|_| IdentityError)?,
            )
            .bearer_auth(api_token.trim())
            .json(
                &json!({"apiVersion":"authentication.k8s.io/v1","kind":"TokenReview",
                "spec":{"token":subject_token,"audiences":[AUDIENCE]}}),
            )
            .send()
            .await
            .map_err(|_| IdentityError)?;
        if !response.status().is_success() {
            return Err(IdentityError);
        }
        let review: Review = response.json().await.map_err(|_| IdentityError)?;
        let status = review
            .status
            .filter(|s| s.authenticated && s.audiences.iter().any(|a| a == AUDIENCE))
            .ok_or(IdentityError)?;
        let user = status.user.ok_or(IdentityError)?;
        let pod_name = extra_one(&user.extra, "authentication.kubernetes.io/pod-name")
            .map_err(|_| IdentityError)?;
        let pod_uid = extra_one(&user.extra, "authentication.kubernetes.io/pod-uid")
            .map_err(|_| IdentityError)?;
        if !valid_label(pod_name) {
            return Err(IdentityError);
        }
        let response = self
            .http
            .get(
                self.origin
                    .join(&format!(
                        "api/v1/namespaces/{}/pods/{pod_name}",
                        self.namespace
                    ))
                    .map_err(|_| IdentityError)?,
            )
            .bearer_auth(api_token.trim())
            .send()
            .await
            .map_err(|_| IdentityError)?;
        if !response.status().is_success() {
            return Err(IdentityError);
        }
        let pod: Value = response.json().await.map_err(|_| IdentityError)?;
        verify_pod(&pod, &user.username, &self.namespace, pod_uid).map_err(|_| IdentityError)
    }
}

fn valid_label(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
}
fn extra_one<'a>(extra: &'a HashMap<String, Vec<String>>, name: &str) -> Result<&'a str, ()> {
    let values = extra.get(name).ok_or(())?;
    if values.len() != 1 {
        return Err(());
    }
    Ok(values[0].as_str())
}
fn verify_pod(
    pod: &Value,
    username: &str,
    namespace: &str,
    uid: &str,
) -> Result<VerifiedWorkload, ()> {
    if pod["metadata"]["uid"] != uid
        || pod["metadata"]["namespace"] != namespace
        || pod["metadata"]
            .get("deletionTimestamp")
            .is_some_and(|v| !v.is_null())
        || pod["status"]["phase"] != "Running"
    {
        return Err(());
    }
    let service_id = Uuid::parse_str(
        pod["metadata"]["labels"]["flash.heterocloud.io/instance"]
            .as_str()
            .ok_or(())?,
    )
    .map_err(|_| ())?;
    let account = format!("flash-{}", service_id.simple());
    if username != format!("system:serviceaccount:{namespace}:{account}")
        || pod["spec"]["serviceAccountName"] != account
    {
        return Err(());
    }
    let task_role = Uuid::parse_str(
        pod["metadata"]["annotations"]["iam.heterocloud.io/task-role"]
            .as_str()
            .ok_or(())?,
    )
    .map_err(|_| ())?;
    Ok(VerifiedWorkload {
        service_id,
        pod_uid: Uuid::parse_str(uid).map_err(|_| ())?,
        task_role,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_is_bound_to_live_pod_service_account_and_attached_role()
    -> Result<(), Box<dyn std::error::Error>> {
        let id = Uuid::from_u128(1);
        let uid = Uuid::from_u128(2);
        let role = Uuid::from_u128(3);
        let user = format!("system:serviceaccount:workloads:flash-{}", id.simple());
        let pod = json!({"metadata":{"uid":uid,"namespace":"workloads","labels":{"flash.heterocloud.io/instance":id},"annotations":{"iam.heterocloud.io/task-role":role}},"spec":{"serviceAccountName":format!("flash-{}",id.simple())},"status":{"phase":"Running"}});
        assert_eq!(
            verify_pod(&pod, &user, "workloads", &uid.to_string())
                .map_err(|_| "identity")?
                .task_role,
            role
        );
        assert!(verify_pod(&pod, &user, "other", &uid.to_string()).is_err());
        assert!(
            verify_pod(
                &pod,
                "system:serviceaccount:workloads:default",
                "workloads",
                &uid.to_string()
            )
            .is_err()
        );
        for patch in [
            json!({"metadata":{"uid":Uuid::from_u128(9)}}),
            json!({"metadata":{"deletionTimestamp":"2026-10-05T00:00:00Z"}}),
            json!({"status":{"phase":"Failed"}}),
        ] {
            let mut changed = pod.clone();
            for (section, fields) in patch.as_object().ok_or("object")? {
                for (field, value) in fields.as_object().ok_or("object")? {
                    changed[section][field] = value.clone();
                }
            }
            assert!(verify_pod(&changed, &user, "workloads", &uid.to_string()).is_err());
        }
        Ok(())
    }
}
