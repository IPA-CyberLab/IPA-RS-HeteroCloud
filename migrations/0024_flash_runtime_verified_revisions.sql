-- A cold/explicitly stopped service can be operationally ready with zero Pods.
-- Only a revision whose application has actually passed readiness is a rollback
-- target. Cold-start deadlines begin at first activation, not while idle.
ALTER TABLE flash_service_revisions ADD COLUMN activation_started_at timestamptz;

UPDATE flash_service_revisions
SET ready_at=NULL,phase=CASE WHEN phase='ready' THEN 'pending' ELSE phase END
WHERE coalesce((spec#>>'{autoscaling,min_replicas}')::integer=0,false)
   OR coalesce((spec->>'stopped')::boolean,false);

UPDATE flash_service_revisions SET activation_started_at=created_at
WHERE NOT coalesce((spec#>>'{autoscaling,min_replicas}')::integer=0,false)
  AND NOT coalesce((spec->>'stopped')::boolean,false);

CREATE OR REPLACE FUNCTION capture_flash_revision() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    runtime jsonb;
    healthy boolean;
BEGIN
    IF NEW.provider = 'flash' THEN
        runtime := coalesce(NEW.status->'status',NEW.status);
        healthy := NEW.state='ready'
            AND coalesce((runtime->>'observed_generation')::bigint=NEW.generation,false)
            AND coalesce((runtime->>'ready_replicas')::integer,0)>0
            AND NOT coalesce((NEW.spec->>'stopped')::boolean,false);
        INSERT INTO flash_service_revisions(service_instance_id,generation,organization_id,project_id,name,spec,phase,ready_at)
        VALUES(NEW.id,NEW.generation,NEW.organization_id,NEW.project_id,NEW.name,NEW.spec,
               CASE WHEN healthy THEN 'ready' WHEN NEW.state='error' THEN 'failed' ELSE 'pending' END,
               CASE WHEN healthy THEN now() ELSE NULL END)
        ON CONFLICT(service_instance_id,generation) DO UPDATE SET
            phase=CASE WHEN flash_service_revisions.ready_at IS NOT NULL THEN 'ready' ELSE EXCLUDED.phase END,
            ready_at=coalesce(flash_service_revisions.ready_at,EXCLUDED.ready_at);
    END IF;
    RETURN NEW;
END $$;

-- Preserve observable proof for currently running zero-minimum services.
UPDATE flash_service_revisions r SET ready_at=coalesce(r.ready_at,now()),phase='ready',
    activation_started_at=coalesce(r.activation_started_at,now())
FROM service_instances s
WHERE r.service_instance_id=s.id AND r.generation=s.generation
  AND s.state='ready'
  AND coalesce((coalesce(s.status->'status',s.status)->>'observed_generation')::bigint=s.generation,false)
  AND coalesce((coalesce(s.status->'status',s.status)->>'ready_replicas')::integer,0)>0
  AND NOT coalesce((s.spec->>'stopped')::boolean,false);
