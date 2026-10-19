-- Backups outlive their source service. Never cascade them from a service/PVC.
CREATE TABLE flash_backups (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    project_id uuid NOT NULL,
    service_instance_id uuid NOT NULL,
    principal_id uuid NOT NULL,
    request_id uuid NOT NULL,
    name text NOT NULL,
    source_name text NOT NULL,
    source_generation bigint NOT NULL,
    source_pvc_uid text NOT NULL,
    source_spec jsonb NOT NULL,
    volume_size_bytes bigint NOT NULL CHECK (volume_size_bytes > 0),
    phase text NOT NULL DEFAULT 'pending',
    status jsonb NOT NULL DEFAULT '{}',
    delete_requested boolean NOT NULL DEFAULT false,
    deleted_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (organization_id, request_id)
);
CREATE INDEX flash_backups_owner ON flash_backups(organization_id, created_at DESC);
CREATE INDEX flash_backups_reconcile ON flash_backups(updated_at);

CREATE TABLE flash_backup_restore_requests (
    organization_id uuid NOT NULL REFERENCES organizations(id),
    principal_id uuid NOT NULL,
    request_id uuid NOT NULL,
    backup_id uuid NOT NULL,
    project_id uuid NOT NULL,
    name text NOT NULL,
    service_instance_id uuid NOT NULL UNIQUE,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (organization_id, principal_id, request_id)
);
