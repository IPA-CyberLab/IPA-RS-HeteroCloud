UPDATE service_instances
SET spec = jsonb_set(spec, '{max_rooms}', '100'::jsonb, true)
WHERE provider = 'flow'
  AND jsonb_typeof(spec) = 'object'
  AND NOT spec ? 'max_rooms';

CREATE TABLE realtime_metric_samples (
    service_instance_id uuid NOT NULL
        REFERENCES service_instances(id) ON DELETE CASCADE,
    sampled_at timestamptz NOT NULL,
    measured_at timestamptz NOT NULL,
    active_rooms bigint NOT NULL CHECK (active_rooms >= 0),
    concurrent_connections bigint NOT NULL CHECK (concurrent_connections >= 0),
    sfu_participants bigint NOT NULL CHECK (sfu_participants >= 0),
    p2p_connections bigint NOT NULL CHECK (p2p_connections >= 0),
    ingress_bytes bigint NOT NULL CHECK (ingress_bytes >= 0),
    egress_bytes bigint NOT NULL CHECK (egress_bytes >= 0),
    transferred_bytes bigint NOT NULL CHECK (transferred_bytes >= 0),
    turn_allocations bigint CHECK (turn_allocations >= 0),
    room_limit bigint CHECK (room_limit BETWEEN 1 AND 1000000),
    PRIMARY KEY (service_instance_id, sampled_at)
);
