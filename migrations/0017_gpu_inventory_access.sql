CREATE TABLE gpu_devices (
    id uuid PRIMARY KEY,
    management_id text NOT NULL UNIQUE
        CHECK (char_length(management_id) BETWEEN 1 AND 512),
    gpu_type text NOT NULL
        CHECK (gpu_type ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'),
    display_name text NOT NULL
        CHECK (char_length(display_name) BETWEEN 1 AND 120),
    visibility text NOT NULL DEFAULT 'open'
        CHECK (visibility IN ('open', 'private')),
    available boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX gpu_devices_type_visibility_idx
    ON gpu_devices (gpu_type, visibility, id);

CREATE TABLE gpu_device_user_assignments (
    gpu_device_id uuid NOT NULL REFERENCES gpu_devices(id) ON DELETE CASCADE,
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (gpu_device_id, user_id)
);

CREATE INDEX gpu_device_user_assignments_user_idx
    ON gpu_device_user_assignments (user_id, gpu_device_id);

-- `gpu_count` was accepted only as 0 or 1. Persisted requests are upgraded to
-- the initial cluster GPU model; new API requests accept `gpu_type` only.
UPDATE service_instances
SET spec = CASE
    WHEN COALESCE((spec->>'gpu_count')::integer, 0) = 1
        THEN CASE
            WHEN jsonb_typeof(spec->'autoscaling') = 'object' THEN
                jsonb_set(
                    jsonb_set(
                        jsonb_set(
                            (spec - 'gpu_count') || jsonb_build_object(
                                'gpu_type', 'nvidia-geforce-gtx-1080-ti'
                            ),
                            '{replicas}', '1'::jsonb, true
                        ),
                        '{autoscaling,max_replicas}', '1'::jsonb, true
                    ),
                    '{autoscaling,min_replicas}',
                    to_jsonb(least(
                        COALESCE((spec->'autoscaling'->>'min_replicas')::integer, 1),
                        1
                    )),
                    true
                )
            ELSE jsonb_set(
                (spec - 'gpu_count') || jsonb_build_object(
                    'gpu_type', 'nvidia-geforce-gtx-1080-ti'
                ),
                '{replicas}', '1'::jsonb, true
            )
        END
    ELSE spec - 'gpu_count'
END,
updated_at = now()
WHERE provider = 'flash' AND spec ? 'gpu_count';

CREATE TABLE flash_gpu_service_requests (
    service_instance_id uuid PRIMARY KEY
        REFERENCES service_instances(id) ON DELETE CASCADE,
    gpu_type text NOT NULL
        CHECK (gpu_type ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'),
    requested_by_principal_id uuid REFERENCES principals(id) ON DELETE SET NULL,
    requested_by_user_id uuid REFERENCES users(id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX flash_gpu_service_requests_type_idx
    ON flash_gpu_service_requests (gpu_type, service_instance_id);

-- Old rows predate requester tracking, but must remain represented for access
-- reconciliation. Recover the latest actor from the retained outbox when one
-- is available.
INSERT INTO flash_gpu_service_requests (
    service_instance_id,
    gpu_type,
    requested_by_principal_id,
    requested_by_user_id
)
SELECT
    s.id,
    s.spec->>'gpu_type',
    p.id,
    p.user_id
FROM service_instances s
LEFT JOIN LATERAL (
    SELECT (e.payload->>'principal_id')::uuid AS principal_id
    FROM outbox_events e
    WHERE e.aggregate_id = s.id
      AND e.topic = 'service-instance.reconcile'
      AND e.payload->>'provider' = 'flash'
      AND e.payload->>'principal_id' IS NOT NULL
    ORDER BY
        (e.payload->>'generation')::bigint DESC NULLS LAST,
        e.created_at DESC,
        e.id DESC
    LIMIT 1
) requester ON true
LEFT JOIN principals p
    ON p.id = requester.principal_id
   AND p.organization_id = s.organization_id
WHERE s.provider = 'flash'
  AND s.state <> 'deleting'
  AND s.spec ? 'gpu_type';
