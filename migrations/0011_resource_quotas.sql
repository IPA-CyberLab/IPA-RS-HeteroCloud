CREATE TABLE resource_quota_defaults (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    limits jsonb NOT NULL CHECK (jsonb_typeof(limits) = 'object'),
    updated_at timestamptz NOT NULL DEFAULT now()
);

UPDATE service_instances
SET spec = jsonb_set(spec, '{max_rooms}', '100'::jsonb, true)
WHERE provider = 'flow' AND NOT (spec ? 'max_rooms');

INSERT INTO resource_quota_defaults (singleton, limits)
VALUES (
    true,
    '{
      "flow": {
        "max_services": 100,
        "max_rooms_per_service": 1000000,
        "max_total_rooms": 1000000,
        "max_participants_per_service": 100000,
        "max_rate_limit_requests_per_second": 1000,
        "max_rate_limit_burst": 5000,
        "max_developer_credentials_per_service": 100
      },
      "flash": {
        "max_services": 100,
        "max_replicas_per_service": 100,
        "max_cpu_millis_per_vm": 4000,
        "max_memory_mib_per_vm": 8128,
        "max_disk_gib_per_vm": 10,
        "max_total_replicas": 100,
        "max_total_cpu_millis": 20000,
        "max_total_memory_mib": 32768,
        "max_total_disk_gib": 100
      },
      "registry": {
        "storage_gib": 10,
        "max_credentials": 10
      }
    }'::jsonb
);

CREATE TABLE organization_resource_quotas (
    organization_id uuid PRIMARY KEY REFERENCES organizations(id) ON DELETE CASCADE,
    limits jsonb NOT NULL CHECK (jsonb_typeof(limits) = 'object'),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE registry_credentials (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    created_by uuid NOT NULL REFERENCES principals(id) ON DELETE RESTRICT,
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 120),
    username text,
    harbor_robot_id bigint UNIQUE,
    status text NOT NULL CHECK (status IN ('provisioning', 'active', 'revoked')),
    created_at timestamptz NOT NULL DEFAULT now(),
    revoked_at timestamptz,
    UNIQUE (organization_id, name)
);

CREATE INDEX registry_credentials_organization_created_idx
    ON registry_credentials (organization_id, created_at DESC);
