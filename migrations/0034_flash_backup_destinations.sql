-- Destination credentials are stored in Secret Manager and the private storage
-- provider, never in this table. Existing backups continue using Syouyu.
CREATE TABLE flash_backup_destinations (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    principal_id uuid NOT NULL,
    request_id uuid NOT NULL,
    name text NOT NULL,
    endpoint text NOT NULL,
    bucket text NOT NULL,
    region text NOT NULL,
    prefix text NOT NULL DEFAULT '',
    virtual_hosted boolean NOT NULL DEFAULT false,
    state text NOT NULL DEFAULT 'pending' CHECK (state IN ('pending', 'ready', 'error')),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (organization_id, id),
    UNIQUE (organization_id, request_id),
    UNIQUE (organization_id, name),
    FOREIGN KEY (organization_id, principal_id) REFERENCES principals(organization_id, id)
);

ALTER TABLE flash_backups ADD COLUMN destination_id uuid;
ALTER TABLE flash_backups ADD CONSTRAINT flash_backups_destination_scope
    FOREIGN KEY (organization_id, destination_id) REFERENCES flash_backup_destinations(organization_id, id);
ALTER TABLE flash_backup_policies ADD COLUMN destination_id uuid;
ALTER TABLE flash_backup_policies ADD CONSTRAINT flash_backup_policies_destination_scope
    FOREIGN KEY (organization_id, destination_id) REFERENCES flash_backup_destinations(organization_id, id);
