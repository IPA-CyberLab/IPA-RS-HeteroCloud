CREATE TABLE cli_device_authorizations (
    id uuid PRIMARY KEY,
    device_code_hash bytea NOT NULL UNIQUE CHECK (octet_length(device_code_hash) = 32),
    user_code_hash bytea NOT NULL UNIQUE CHECK (octet_length(user_code_hash) = 32),
    organization_id uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    approved_user_id uuid REFERENCES users(id) ON DELETE CASCADE,
    status text NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'approved', 'denied', 'consumed')),
    interval_seconds integer NOT NULL CHECK (interval_seconds BETWEEN 1 AND 60),
    expires_at timestamptz NOT NULL,
    last_polled_at timestamptz,
    approved_at timestamptz,
    consumed_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    CHECK (
        (status = 'pending' AND approved_user_id IS NULL AND approved_at IS NULL AND consumed_at IS NULL)
        OR (status = 'approved' AND approved_user_id IS NOT NULL AND approved_at IS NOT NULL AND consumed_at IS NULL)
        OR (status = 'denied' AND consumed_at IS NULL)
        OR (status = 'consumed' AND approved_user_id IS NOT NULL AND approved_at IS NOT NULL AND consumed_at IS NOT NULL)
    )
);

CREATE INDEX cli_device_authorizations_expires_at_idx
    ON cli_device_authorizations (expires_at);

CREATE TABLE cli_access_tokens (
    id uuid PRIMARY KEY,
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    organization_id uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    prefix text NOT NULL UNIQUE CHECK (char_length(prefix) = 16),
    token_hash bytea NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
    expires_at timestamptz NOT NULL,
    last_used_at timestamptz,
    revoked_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX cli_access_tokens_user_id_idx ON cli_access_tokens (user_id);
CREATE INDEX cli_access_tokens_expires_at_idx ON cli_access_tokens (expires_at);
