CREATE TABLE flash_capacity_waits (
    service_instance_id uuid PRIMARY KEY REFERENCES service_instances(id) ON DELETE CASCADE,
    id uuid NOT NULL UNIQUE,
    organization_id uuid NOT NULL,
    principal_id uuid NOT NULL,
    role_session_id uuid,
    created_service boolean NOT NULL,
    generation bigint NOT NULL,
    state text NOT NULL CHECK (state IN ('waiting', 'starting', 'ready', 'cancelled', 'expired', 'rejected', 'superseded')),
    reason text,
    requested_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    expires_at timestamptz NOT NULL,
    next_check_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    attempt bigint NOT NULL DEFAULT 0
);

CREATE INDEX flash_capacity_waits_due ON flash_capacity_waits (next_check_at, requested_at, id)
    WHERE state IN ('waiting', 'starting');
