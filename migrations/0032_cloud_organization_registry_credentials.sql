-- Every API replica must understand cross-account reservations before this
-- gate is enabled. Existing credentials are retained when the limit is lower.
ALTER TABLE cloud_security_activation
    ADD COLUMN organization_registry_credentials_enabled boolean NOT NULL DEFAULT false;
