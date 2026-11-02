-- Aggregate service allocations can only be activated after all API replicas
-- use the shared cross-account lock and enforce these limits.
ALTER TABLE cloud_security_activation
    ADD COLUMN organization_service_quotas_enabled boolean NOT NULL DEFAULT false;
