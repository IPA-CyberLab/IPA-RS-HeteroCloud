-- SSH keys belong to a user in an account. A key grants access only when
-- explicitly attached to a Flash service and the user retains ExecInstance.
CREATE TABLE flash_ssh_keys (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL,
    user_id uuid NOT NULL,
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 120),
    public_key text NOT NULL CHECK (octet_length(public_key) BETWEEN 1 AND 16384),
    fingerprint text NOT NULL CHECK (char_length(fingerprint) BETWEEN 1 AND 128),
    github_username text,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (organization_id, id),
    UNIQUE (organization_id, fingerprint),
    FOREIGN KEY (organization_id, user_id)
        REFERENCES organization_memberships (organization_id, user_id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX service_instances_organization_id_id_key
    ON service_instances (organization_id, id);

CREATE TABLE flash_ssh_key_bindings (
    organization_id uuid NOT NULL,
    service_instance_id uuid NOT NULL,
    key_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (organization_id, service_instance_id, key_id),
    FOREIGN KEY (organization_id, service_instance_id)
        REFERENCES service_instances (organization_id, id) ON DELETE CASCADE,
    FOREIGN KEY (organization_id, key_id)
        REFERENCES flash_ssh_keys (organization_id, id) ON DELETE CASCADE
);
