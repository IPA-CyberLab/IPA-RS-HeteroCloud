CREATE TABLE flash_domain_bindings (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    service_instance_id uuid NOT NULL REFERENCES service_instances(id) ON DELETE CASCADE,
    principal_id uuid NOT NULL,
    hostname text NOT NULL UNIQUE CHECK (length(hostname) BETWEEN 3 AND 253 AND hostname = lower(hostname)),
    verification_value text NOT NULL,
    delete_requested boolean NOT NULL DEFAULT false,
    reconcile_pending boolean NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX flash_domain_bindings_service_idx ON flash_domain_bindings(service_instance_id);
CREATE INDEX flash_domain_bindings_pending_idx ON flash_domain_bindings(updated_at) WHERE reconcile_pending;
