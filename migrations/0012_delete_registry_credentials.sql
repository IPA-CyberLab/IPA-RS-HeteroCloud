DELETE FROM registry_credentials WHERE status = 'revoked';

ALTER TABLE registry_credentials
    DROP CONSTRAINT registry_credentials_status_check;

ALTER TABLE registry_credentials
    ADD CONSTRAINT registry_credentials_status_check
    CHECK (status IN ('provisioning', 'active'));

ALTER TABLE registry_credentials
    DROP COLUMN revoked_at;
