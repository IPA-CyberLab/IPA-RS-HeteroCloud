//! Desired domain bindings are independent of workload generations and secrets.
use crate::{flash_provider::FlashProviderContext, routes::AppState};
use heterocloud_domain::{PrincipalId, ServiceInstanceId};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

pub fn validate_hostname(value: &str) -> Result<(), &'static str> {
    if value.len() > 253
        || value.len() < 3
        || !value.contains('.')
        || value.parse::<std::net::IpAddr>().is_ok()
        || value.ends_with(".internal")
        || value.ends_with(".localhost")
        || value.ends_with(".local")
        || value.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || !label.as_bytes()[0].is_ascii_alphanumeric()
                || !label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
                || label
                    .bytes()
                    .any(|b| !b.is_ascii_lowercase() && !b.is_ascii_digit() && b != b'-')
        })
    {
        return Err(
            "Use a lowercase public DNS hostname without scheme, path, port, wildcard or IP address.",
        );
    }
    Ok(())
}

pub fn reserved_hostname(hostname: &str, names: &str, suffixes: &str) -> bool {
    names
        .split(',')
        .filter(|x| !x.is_empty())
        .any(|x| hostname == x)
        || suffixes
            .split(',')
            .filter(|x| !x.is_empty())
            .any(|x| hostname == x || hostname.ends_with(&format!(".{x}")))
}

pub fn pending_status(binding: &heterocloud_store::FlashDomainBinding) -> Value {
    json!({"id":binding.id,"hostname":binding.hostname,"phase":if binding.delete_requested {"deleting"} else {"queued"},
        "cname_target":null,"verification":{"type":"TXT","name":format!("_heterocloud.{}",binding.hostname),"value":binding.verification_value},
        "oidc_callback_url":format!("https://{}/_heterocloud/oidc/callback",binding.hostname)})
}

pub fn spawn_reconciler(state: Arc<AppState>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(5));
        loop {
            ticker.tick().await;
            let Ok(bindings) = state.store.pending_flash_domains().await else {
                continue;
            };
            let Some(provider) = &state.flash_provider else {
                continue;
            };
            for binding in bindings {
                let _ = state.store.mark_flash_domain_attempted(binding.id).await;
                let Ok(Some(service)) = state
                    .store
                    .service_instance(ServiceInstanceId(binding.service_instance_id))
                    .await
                else {
                    continue;
                };
                if service.organization_id.0 != binding.organization_id
                    || service.provider != "flash"
                {
                    continue;
                }
                let context = FlashProviderContext {
                    principal_id: PrincipalId(binding.principal_id),
                    user_id: None,
                    organization_id: service.organization_id,
                    project_id: service.project_id,
                    service_instance_id: service.id,
                    generation: service.generation,
                };
                if binding.delete_requested {
                    if let Ok(done) = provider
                        .delete_custom_domain(context, &binding.hostname)
                        .await
                        && done
                    {
                        let _ = state.store.release_flash_domain(binding.id).await;
                    }
                } else if provider
                    .put_custom_domain(
                        context,
                        &binding.hostname,
                        binding.id,
                        &binding.verification_value,
                    )
                    .await
                    .is_ok()
                {
                    let _ = state.store.mark_flash_domain_synced(binding.id).await;
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_hostnames_reject_caddy_injection_and_private_targets() {
        for value in [
            "App.example.org",
            "https://app.example.org",
            "a.example.org:443",
            "*.example.org",
            "a..example.org",
            "a.example.org\n{respond 200}",
            "127.0.0.1",
            "secrets.heteronetwork.internal",
            "host.local",
            "localhost",
            "a.example.org.",
            "a.-bad.org",
        ] {
            assert!(validate_hostname(value).is_err(), "{value}");
        }
        for value in [
            "app.example.org",
            "example.org",
            "xn--eckwd4c7c.example.org",
        ] {
            assert!(validate_hostname(value).is_ok());
        }
    }
    #[test]
    fn reserved_hostnames_use_label_boundaries_and_configured_site() {
        assert!(reserved_hostname(
            "owner.example.org",
            "owner.example.org",
            "flash.example.org"
        ));
        assert!(reserved_hostname(
            "tenant.flash.example.org",
            "",
            "flash.example.org"
        ));
        assert!(!reserved_hostname(
            "myflash.example.org",
            "",
            "flash.example.org"
        ));
        assert!(!reserved_hostname(
            "owner.example.net",
            "owner.example.org",
            "flash.example.org"
        ));
    }
}
