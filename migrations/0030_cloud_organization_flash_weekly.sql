-- This gate is separate from allocation limits: the Flash provider and its
-- CRD must understand cloud organization usage before workers send it.
ALTER TABLE cloud_security_activation
    ADD COLUMN organization_flash_weekly_quotas_enabled boolean NOT NULL DEFAULT false;
