UPDATE service_instances
SET spec = spec - 'traffic_mode'
WHERE provider = 'flow'
  AND jsonb_typeof(spec) = 'object'
  AND spec ? 'traffic_mode';
