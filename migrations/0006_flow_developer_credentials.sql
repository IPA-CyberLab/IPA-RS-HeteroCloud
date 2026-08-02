CREATE TABLE flow_developer_credentials (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    service_instance_id uuid NOT NULL REFERENCES service_instances(id) ON DELETE CASCADE,
    created_by uuid NOT NULL REFERENCES principals(id),
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 120),
    prefix text NOT NULL UNIQUE CHECK (prefix ~ '^hcf_[0-9a-f]{16}$'),
    secret_hash bytea NOT NULL CHECK (octet_length(secret_hash) = 32),
    permissions text[] NOT NULL CHECK (cardinality(permissions) BETWEEN 1 AND 8),
    expires_at timestamptz NOT NULL,
    last_used_at timestamptz,
    revoked_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    CHECK (expires_at >= created_at + interval '1 day'),
    CHECK (expires_at <= created_at + interval '365 days'),
    CHECK (array_position(permissions, NULL) IS NULL),
    CHECK (
        permissions <@ ARRAY[
            'flow.queue.read',
            'flow.queue.write',
            'flow.room.create',
            'flow.room.read',
            'flow.room.join',
            'flow.turn.issue',
            'flow.signal.connect',
            'flow.metrics.read'
        ]::text[]
    )
);

CREATE INDEX flow_developer_credentials_service_idx
    ON flow_developer_credentials (service_instance_id, created_at DESC, id DESC);

CREATE INDEX flow_developer_credentials_active_idx
    ON flow_developer_credentials (service_instance_id, expires_at)
    WHERE revoked_at IS NULL;

CREATE TABLE flow_access_contexts (
    context_id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    service_instance_id uuid NOT NULL REFERENCES service_instances(id) ON DELETE CASCADE,
    credential_id uuid REFERENCES flow_developer_credentials(id) ON DELETE SET NULL,
    principal_id uuid NOT NULL,
    permissions text[] NOT NULL CHECK (cardinality(permissions) BETWEEN 1 AND 8),
    issued_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL,
    revoked_at timestamptz,
    CHECK (expires_at >= issued_at + interval '30 seconds'),
    CHECK (expires_at <= issued_at + interval '300 seconds'),
    CHECK (array_position(permissions, NULL) IS NULL),
    CHECK (
        permissions <@ ARRAY[
            'flow.queue.read',
            'flow.queue.write',
            'flow.room.create',
            'flow.room.read',
            'flow.room.join',
            'flow.turn.issue',
            'flow.signal.connect',
            'flow.metrics.read'
        ]::text[]
    )
);

CREATE INDEX flow_access_contexts_service_issued_idx
    ON flow_access_contexts (service_instance_id, issued_at DESC, context_id DESC);

CREATE INDEX flow_access_contexts_active_idx
    ON flow_access_contexts (service_instance_id, expires_at)
    WHERE revoked_at IS NULL;
