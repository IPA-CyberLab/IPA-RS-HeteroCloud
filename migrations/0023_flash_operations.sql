CREATE TABLE flash_service_revisions (
    service_instance_id uuid NOT NULL,
    generation bigint NOT NULL,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    project_id uuid NOT NULL,
    principal_id uuid,
    name text NOT NULL,
    spec jsonb NOT NULL,
    phase text NOT NULL DEFAULT 'pending',
    created_at timestamptz NOT NULL DEFAULT now(),
    ready_at timestamptz,
    PRIMARY KEY (service_instance_id, generation)
);
CREATE INDEX flash_revision_owner ON flash_service_revisions(organization_id, service_instance_id, generation DESC);

CREATE FUNCTION capture_flash_revision_actor() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.topic='service-instance.reconcile' AND NEW.payload->>'provider'='flash' THEN
        UPDATE flash_service_revisions SET principal_id=(NEW.payload->>'principal_id')::uuid
        WHERE service_instance_id=NEW.aggregate_id AND generation=(NEW.payload->>'generation')::bigint;
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER flash_revision_actor AFTER INSERT ON outbox_events FOR EACH ROW EXECUTE FUNCTION capture_flash_revision_actor();

CREATE FUNCTION capture_flash_revision() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.provider = 'flash' THEN
        INSERT INTO flash_service_revisions(service_instance_id,generation,organization_id,project_id,name,spec,phase,ready_at)
        VALUES(NEW.id,NEW.generation,NEW.organization_id,NEW.project_id,NEW.name,NEW.spec,
               CASE WHEN NEW.state='ready' THEN 'ready' WHEN NEW.state='error' THEN 'failed' ELSE 'pending' END,
               CASE WHEN NEW.state='ready' THEN now() ELSE NULL END)
        ON CONFLICT(service_instance_id,generation) DO UPDATE SET
            phase=CASE WHEN flash_service_revisions.ready_at IS NOT NULL THEN 'ready' ELSE EXCLUDED.phase END,
            ready_at=coalesce(flash_service_revisions.ready_at,EXCLUDED.ready_at);
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER flash_revision_changed AFTER INSERT OR UPDATE OF generation,state ON service_instances
    FOR EACH ROW EXECUTE FUNCTION capture_flash_revision();
INSERT INTO flash_service_revisions(service_instance_id,generation,organization_id,project_id,name,spec,phase,created_at,ready_at)
SELECT id,generation,organization_id,project_id,name,spec,
       CASE WHEN state='ready' THEN 'ready' ELSE 'pending' END,updated_at,
       CASE WHEN state='ready' THEN updated_at ELSE NULL END
FROM service_instances WHERE provider='flash';

CREATE TABLE flash_service_events (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    service_instance_id uuid NOT NULL,
    generation bigint NOT NULL,
    code text NOT NULL CHECK(length(code)<=80),
    severity text NOT NULL CHECK(severity IN ('info','warning','error')),
    message text NOT NULL CHECK(length(message)<=1024),
    created_at timestamptz NOT NULL DEFAULT now(),
    last_observed_at timestamptz NOT NULL DEFAULT now(),
    resolved_at timestamptz,
    acknowledged_at timestamptz,
    UNIQUE(service_instance_id,generation,code)
);
CREATE INDEX flash_events_owner ON flash_service_events(organization_id,created_at DESC);

CREATE TABLE flash_backup_policies (
    service_instance_id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    principal_id uuid NOT NULL,
    enabled boolean NOT NULL DEFAULT false,
    interval_seconds integer NOT NULL DEFAULT 86400 CHECK(interval_seconds BETWEEN 3600 AND 604800),
    retention_count integer NOT NULL DEFAULT 2 CHECK(retention_count BETWEEN 1 AND 8),
    verify_restore boolean NOT NULL DEFAULT true,
    next_run_at timestamptz NOT NULL DEFAULT now(),
    last_run_at timestamptz,
    last_backup_id uuid,
    last_error text,
    lease_until timestamptz,
    updated_at timestamptz NOT NULL DEFAULT now()
);
ALTER TABLE flash_backups ADD COLUMN schedule_service_id uuid;
CREATE INDEX flash_scheduled_backups ON flash_backups(schedule_service_id,created_at DESC) WHERE deleted_at IS NULL;
