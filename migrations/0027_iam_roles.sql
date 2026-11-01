-- Roles are service principals with an explicit trust policy. Sessions are
-- short-lived bearer credentials; raw credentials are never stored.
ALTER TABLE cloud_security_activation ADD COLUMN roles_enabled boolean NOT NULL DEFAULT false;
CREATE TABLE iam_roles (
    principal_id uuid PRIMARY KEY,
    organization_id uuid NOT NULL,
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 64),
    trust_policy jsonb NOT NULL,
    max_session_seconds integer NOT NULL DEFAULT 3600 CHECK (max_session_seconds BETWEEN 3600 AND 43200),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (organization_id, name),
    UNIQUE (organization_id, principal_id),
    FOREIGN KEY (organization_id, principal_id) REFERENCES principals(organization_id, id) ON DELETE CASCADE
);

CREATE TABLE iam_role_sessions (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL,
    role_principal_id uuid NOT NULL,
    source_organization_id uuid NOT NULL,
    source_principal_id uuid NOT NULL,
    source_arn text NOT NULL,
    session_name text NOT NULL CHECK (char_length(session_name) BETWEEN 2 AND 64),
    session_policy jsonb,
    token_hash bytea NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
    expires_at timestamptz NOT NULL,
    revoked_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (organization_id, role_principal_id) REFERENCES iam_roles(organization_id, principal_id) ON DELETE CASCADE
);
CREATE INDEX iam_role_sessions_expiry ON iam_role_sessions (expires_at);
