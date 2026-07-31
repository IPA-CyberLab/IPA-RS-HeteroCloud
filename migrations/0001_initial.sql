CREATE TABLE users (
    id uuid PRIMARY KEY,
    email text NOT NULL,
    display_name text NOT NULL CHECK (char_length(display_name) BETWEEN 1 AND 120),
    password_hash text NOT NULL,
    status text NOT NULL CHECK (status IN ('active', 'suspended')),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX users_normalized_email_key ON users (lower(email));

CREATE TABLE organizations (
    id uuid PRIMARY KEY,
    slug text NOT NULL UNIQUE CHECK (slug ~ '^[a-z][a-z0-9-]{1,61}[a-z0-9]$'),
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 120),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE principals (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    kind text NOT NULL CHECK (kind IN ('user', 'service_account')),
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 120),
    user_id uuid REFERENCES users(id) ON DELETE CASCADE,
    enabled boolean NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CHECK (
        (kind = 'user' AND user_id IS NOT NULL)
        OR (kind = 'service_account' AND user_id IS NULL)
    ),
    UNIQUE (organization_id, name),
    UNIQUE (organization_id, user_id)
);

CREATE TABLE organization_memberships (
    organization_id uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    principal_id uuid NOT NULL REFERENCES principals(id) ON DELETE CASCADE,
    role text NOT NULL CHECK (role IN ('owner', 'member')),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (organization_id, user_id),
    UNIQUE (organization_id, principal_id)
);

CREATE TABLE projects (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    slug text NOT NULL CHECK (slug ~ '^[a-z][a-z0-9-]{1,61}[a-z0-9]$'),
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 120),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (organization_id, slug)
);

CREATE TABLE iam_policies (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 120),
    document jsonb NOT NULL,
    semantics_digest text NOT NULL CHECK (char_length(semantics_digest) = 64),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (organization_id, name)
);

CREATE TABLE iam_bindings (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    principal_id uuid NOT NULL REFERENCES principals(id) ON DELETE CASCADE,
    policy_id uuid NOT NULL REFERENCES iam_policies(id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (organization_id, principal_id, policy_id)
);

CREATE TABLE api_keys (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    principal_id uuid NOT NULL REFERENCES principals(id) ON DELETE CASCADE,
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 120),
    prefix text NOT NULL UNIQUE,
    secret_hash bytea NOT NULL CHECK (octet_length(secret_hash) = 32),
    expires_at timestamptz,
    last_used_at timestamptz,
    revoked_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (organization_id, principal_id, name)
);

CREATE TABLE sessions (
    id uuid PRIMARY KEY,
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash bytea NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
    expires_at timestamptz NOT NULL,
    last_seen_at timestamptz NOT NULL DEFAULT now(),
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX sessions_user_id_idx ON sessions (user_id);
CREATE INDEX sessions_expires_at_idx ON sessions (expires_at);

CREATE TABLE invitations (
    id uuid PRIMARY KEY,
    code_hash bytea NOT NULL UNIQUE CHECK (octet_length(code_hash) = 32),
    created_by uuid NOT NULL REFERENCES users(id),
    organization_id uuid REFERENCES organizations(id) ON DELETE CASCADE,
    max_uses integer NOT NULL CHECK (max_uses BETWEEN 1 AND 1000),
    used_count integer NOT NULL DEFAULT 0 CHECK (used_count >= 0),
    expires_at timestamptz NOT NULL,
    revoked_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    CHECK (used_count <= max_uses)
);

CREATE TABLE service_instances (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    provider text NOT NULL CHECK (provider ~ '^[a-z][a-z0-9-]{1,62}$'),
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 120),
    generation bigint NOT NULL DEFAULT 1 CHECK (generation > 0),
    state text NOT NULL CHECK (
        state IN ('provisioning', 'ready', 'updating', 'deleting', 'error')
    ),
    spec jsonb NOT NULL,
    status jsonb NOT NULL DEFAULT '{}'::jsonb,
    external_id text,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (project_id, provider, name)
);

CREATE INDEX service_instances_provider_state_idx
    ON service_instances (provider, state);

CREATE TABLE operations (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    project_id uuid REFERENCES projects(id) ON DELETE CASCADE,
    resource_id uuid,
    provider text NOT NULL,
    kind text NOT NULL,
    state text NOT NULL CHECK (state IN ('pending', 'running', 'succeeded', 'failed')),
    idempotency_key text NOT NULL,
    error jsonb,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (organization_id, idempotency_key)
);

CREATE TABLE audit_events (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    occurred_at timestamptz NOT NULL DEFAULT now(),
    organization_id uuid REFERENCES organizations(id) ON DELETE SET NULL,
    principal_id uuid REFERENCES principals(id) ON DELETE SET NULL,
    user_id uuid REFERENCES users(id) ON DELETE SET NULL,
    request_id text NOT NULL,
    source_ip inet,
    action text NOT NULL,
    resource text NOT NULL,
    decision text NOT NULL CHECK (decision IN ('allow', 'deny', 'error')),
    reason text NOT NULL,
    metadata jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE INDEX audit_events_org_time_idx
    ON audit_events (organization_id, occurred_at DESC, id DESC);

CREATE TABLE outbox_events (
    id uuid PRIMARY KEY,
    topic text NOT NULL,
    aggregate_id uuid NOT NULL,
    payload jsonb NOT NULL,
    attempts integer NOT NULL DEFAULT 0,
    available_at timestamptz NOT NULL DEFAULT now(),
    locked_at timestamptz,
    delivered_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX outbox_events_pending_idx
    ON outbox_events (available_at, id)
    WHERE delivered_at IS NULL;
