ALTER TABLE organization_memberships
    DROP CONSTRAINT organization_memberships_role_check;
ALTER TABLE organization_memberships
    ADD CONSTRAINT organization_memberships_role_check
    CHECK (role IN ('owner', 'admin', 'member'));

-- A missing override inherits the organization ceiling. Overrides are scoped
-- to a membership so that leaving an organization removes its allocation.
CREATE TABLE organization_member_resource_quotas (
    organization_id uuid NOT NULL,
    user_id uuid NOT NULL,
    limits jsonb NOT NULL CHECK (jsonb_typeof(limits) = 'object'),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (organization_id, user_id),
    FOREIGN KEY (organization_id, user_id)
      REFERENCES organization_memberships(organization_id, user_id) ON DELETE CASCADE
);

-- This is the person whose allocation is charged, not the last person to edit
-- a resource. Historical resources without reliable provenance remain NULL.
ALTER TABLE service_instances
    ADD COLUMN quota_user_id uuid REFERENCES users(id) ON DELETE SET NULL;
ALTER TABLE principals
    ADD COLUMN quota_user_id uuid REFERENCES users(id) ON DELETE SET NULL;
UPDATE principals SET quota_user_id = user_id WHERE kind = 'user';

UPDATE service_instances s
SET quota_user_id = (
    SELECT p.user_id
    FROM outbox_events e
    JOIN principals p ON p.id::text = e.payload->>'principal_id'
                     AND p.organization_id = s.organization_id
                     AND p.kind = 'user'
    WHERE e.aggregate_id = s.id
      AND e.topic = 'service-instance.reconcile'
      AND e.payload->>'generation' = '1'
    ORDER BY e.created_at, e.id
    LIMIT 1
)
WHERE EXISTS (
    SELECT 1 FROM outbox_events e
    JOIN principals p ON p.id::text = e.payload->>'principal_id'
                     AND p.organization_id = s.organization_id
                     AND p.kind = 'user'
    WHERE e.aggregate_id = s.id
      AND e.topic = 'service-instance.reconcile'
      AND e.payload->>'generation' = '1'
);
CREATE INDEX service_instances_quota_user_idx
    ON service_instances (organization_id, quota_user_id, provider)
    WHERE quota_user_id IS NOT NULL;

-- Usage records survive deletion of a service; its allocation owner must too.
CREATE TABLE service_quota_owners (
    service_instance_id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id uuid REFERENCES users(id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
INSERT INTO service_quota_owners (service_instance_id, organization_id, user_id, created_at)
SELECT id, organization_id, quota_user_id, created_at FROM service_instances;
CREATE INDEX service_quota_owners_user_idx ON service_quota_owners (organization_id, user_id);

-- Existing Flash resources must receive their new quota owner even without a
-- later edit. Old workers do not recognize this topic and retry it, allowing
-- a new worker to deliver the update safely during a rolling upgrade.
INSERT INTO outbox_events (id, topic, aggregate_id, payload)
SELECT gen_random_uuid(), 'service-instance.flash-quota', s.id,
       jsonb_build_object('service_instance_id', s.id, 'organization_id', s.organization_id,
           'project_id', s.project_id, 'principal_id', p.id, 'provider', 'flash',
           'generation', s.generation)
FROM service_instances s
JOIN LATERAL (
    SELECT principal.id FROM principals principal
    WHERE principal.organization_id = s.organization_id AND principal.enabled
    ORDER BY (principal.id::text = (
        SELECT e.payload->>'principal_id' FROM outbox_events e
        WHERE e.aggregate_id = s.id AND e.topic = 'service-instance.reconcile'
          AND e.payload->>'generation' = s.generation::text
        ORDER BY e.created_at DESC, e.id DESC LIMIT 1
    )) DESC NULLS LAST, principal.id LIMIT 1
) p ON true
WHERE s.provider = 'flash' AND s.state <> 'deleting';
