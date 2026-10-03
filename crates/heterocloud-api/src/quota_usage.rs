use std::collections::{BTreeSet, HashMap};

use futures_util::{StreamExt, stream};
use heterocloud_domain::{OrganizationId, PrincipalId, ServiceInstance, ServiceState, SyouyuSpec};
use heterocloud_store::ResourceQuotaTenant;

use crate::syouyu_provider::{
    SyouyuCredentialLimits, SyouyuProviderContext, SyouyuProviderError, SyouyuProviderProxy,
};

/// Called only after the owner session and network boundary have been checked.
pub(crate) async fn populate_syouyu_usage(
    tenants: &mut [ResourceQuotaTenant],
    targets: Vec<ServiceInstance>,
    provider: Option<&SyouyuProviderProxy>,
    owner_subject: PrincipalId,
) {
    let limits = tenants
        .iter()
        .map(|tenant| {
            let quota = &tenant.effective_limits.syouyu;
            (
                tenant.organization.id,
                SyouyuCredentialLimits {
                    max_credentials_per_bucket: quota.max_credentials_per_bucket,
                    max_total_credentials: quota.max_total_credentials,
                },
            )
        })
        .collect::<HashMap<_, _>>();
    let mut totals = tenants
        .iter()
        .map(|tenant| (tenant.organization.id, Some(0_u64)))
        .collect::<HashMap<_, _>>();
    let mut results = stream::iter(targets)
        .map(|target| {
            let limits = &limits;
            async move {
                let result = async {
                    if target.state != ServiceState::Ready {
                        return Err(SyouyuProviderError::InvalidResponse);
                    }
                    let provider = provider.ok_or(SyouyuProviderError::InvalidResponse)?;
                    let credential_limits = *limits
                        .get(&target.organization_id)
                        .ok_or(SyouyuProviderError::InvalidResponse)?;
                    let spec: SyouyuSpec = serde_json::from_value(target.spec)
                        .map_err(|_| SyouyuProviderError::InvalidResponse)?;
                    let usage = provider
                        .usage(&SyouyuProviderContext {
                            // This is the authenticated global owner's subject,
                            // never a tenant-selected principal or permission.
                            principal_id: owner_subject,
                            organization_id: target.organization_id,
                            project_id: target.project_id,
                            service_instance_id: target.id,
                            permissions: BTreeSet::from(["syouyu.usage.read".to_owned()]),
                            credential_limits,
                        })
                        .await?;
                    if usage.service_instance_id != target.id.0
                        || usage.quota_bytes != spec.quota_bytes
                        || usage.quota_objects != spec.quota_objects
                    {
                        return Err(SyouyuProviderError::InvalidResponse);
                    }
                    Ok(usage.bytes_used)
                }
                .await;
                if let Err(error) = &result {
                    tracing::warn!(
                        organization_id = %target.organization_id,
                        service_instance_id = %target.id,
                        %error,
                        "owner Syouyu usage lookup failed"
                    );
                }
                (target.organization_id, result.ok())
            }
        })
        .buffer_unordered(8);
    while let Some((organization, bytes)) = results.next().await {
        add_usage(&mut totals, organization, bytes);
    }
    for tenant in tenants {
        tenant.usage.syouyu_storage_bytes = totals.remove(&tenant.organization.id).flatten();
    }
}

fn add_usage(
    totals: &mut HashMap<OrganizationId, Option<u64>>,
    organization: OrganizationId,
    bytes: Option<u64>,
) {
    let total = totals.entry(organization).or_insert(Some(0));
    *total = total.and_then(|total| bytes.and_then(|bytes| total.checked_add(bytes)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn totals_are_scoped_and_never_hide_failed_or_overflowing_buckets() {
        let organization = OrganizationId::new();
        let other = OrganizationId::new();
        let empty = OrganizationId::new();
        let mut totals = HashMap::from([(empty, Some(0))]);
        add_usage(&mut totals, organization, Some(42));
        add_usage(&mut totals, organization, Some(21));
        add_usage(&mut totals, other, Some(10));
        assert_eq!(totals[&organization], Some(63));
        assert_eq!(totals[&empty], Some(0));
        add_usage(&mut totals, organization, None);
        add_usage(&mut totals, organization, Some(10));
        assert_eq!(totals[&organization], None);
        assert_eq!(totals[&other], Some(10));
        add_usage(&mut totals, other, Some(u64::MAX));
        assert_eq!(totals[&other], None);
    }

    fn tenant(id: OrganizationId) -> ResourceQuotaTenant {
        ResourceQuotaTenant {
            organization: heterocloud_domain::Organization {
                id,
                slug: id.to_string(),
                name: "quota-test".into(),
                created_at: chrono::Utc::now(),
            },
            override_limits: None,
            effective_limits: heterocloud_domain::ResourceQuotaLimits::default(),
            usage: heterocloud_store::ResourceQuotaUsage::default(),
        }
    }

    fn bucket(organization_id: OrganizationId) -> ServiceInstance {
        ServiceInstance {
            id: heterocloud_domain::ServiceInstanceId::new(),
            organization_id,
            project_id: heterocloud_domain::ProjectId::new(),
            provider: "syouyu".into(),
            name: "usage-test".into(),
            generation: 1,
            state: ServiceState::Ready,
            spec: serde_json::json!({
                "region": "heteronet-global", "bucket_name": "usage-test", "quota_bytes": 1073741824_u64,
                "quota_objects": 1000, "metadata": {},
            }),
            status: serde_json::json!({}),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    async fn no_buckets_is_zero_but_missing_provider_for_a_bucket_is_unavailable() {
        let empty = OrganizationId::new();
        let occupied = OrganizationId::new();
        let mut tenants = [tenant(empty), tenant(occupied)];
        populate_syouyu_usage(
            &mut tenants,
            vec![bucket(occupied)],
            None,
            PrincipalId::new(),
        )
        .await;
        assert_eq!(tenants[0].usage.syouyu_storage_bytes, Some(0));
        assert_eq!(tenants[1].usage.syouyu_storage_bytes, None);
    }

    #[tokio::test]
    async fn provider_usage_is_read_with_service_scope_and_wrong_scope_is_rejected()
    -> Result<(), Box<dyn std::error::Error>> {
        use axum::{
            Json, Router,
            http::{HeaderMap, StatusCode},
            routing::get,
        };
        use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
        use serde_json::{Value, json};
        let _ = rustls::crypto::ring::default_provider().install_default();
        let organization = OrganizationId::new();
        let other = OrganizationId::new();
        let first = bucket(organization);
        let second = bucket(organization);
        let bad = bucket(other);
        let bad_id = bad.id.0;
        let owner_subject = PrincipalId::new();
        let app = Router::new().route("/v1/usage", get(move |headers: HeaderMap| async move {
            let raw = URL_SAFE_NO_PAD.decode(headers["x-syouyu-principal"].as_bytes()).map_err(|_| StatusCode::BAD_REQUEST)?;
            let context: Value = serde_json::from_slice(&raw).map_err(|_| StatusCode::BAD_REQUEST)?;
            assert_eq!(context["permissions"], json!(["syouyu.usage.read"]));
            assert_eq!(context["principal_id"], owner_subject.0.to_string());
            let scope = context["service_instance_id"].as_str().ok_or(StatusCode::BAD_REQUEST)?;
            let returned_scope = if scope == bad_id.to_string() { uuid::Uuid::new_v4().to_string() } else { scope.to_owned() };
            Ok::<_, StatusCode>(Json(json!({
                "service_instance_id": returned_scope, "bytes_used": 42, "objects_used": 1,
                "unfinished_upload_bytes": 0, "unfinished_uploads": 0,
                "quota_bytes": 1073741824_u64, "quota_objects": 1000, "measured_at": chrono::Utc::now(),
            })))
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let endpoint = format!("http://{}", listener.local_addr()?).parse()?;
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        let provider = SyouyuProviderProxy::new(
            endpoint,
            "heterocloud",
            "heterocloud-syouyu-data",
            secrecy::SecretString::new("usage-test-secret-with-at-least-32-bytes".into()),
            reqwest::Client::new(),
        )?;
        let mut tenants = [tenant(organization), tenant(other)];
        populate_syouyu_usage(
            &mut tenants,
            vec![first, second, bad],
            Some(&provider),
            owner_subject,
        )
        .await;
        assert_eq!(tenants[0].usage.syouyu_storage_bytes, Some(84));
        assert_eq!(tenants[1].usage.syouyu_storage_bytes, None);
        server.abort();
        Ok(())
    }
}
