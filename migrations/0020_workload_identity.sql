-- Workload credentials are short lived and contain no personal CLI identity.
CREATE TABLE workload_access_tokens (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    service_instance_id uuid NOT NULL REFERENCES service_instances(id) ON DELETE CASCADE,
    principal_id uuid NOT NULL REFERENCES principals(id) ON DELETE CASCADE,
    pod_uid uuid NOT NULL,
    token_hash bytea NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
    expires_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX workload_access_tokens_expiry_idx ON workload_access_tokens(expires_at);
CREATE INDEX workload_access_tokens_service_idx ON workload_access_tokens(service_instance_id);
