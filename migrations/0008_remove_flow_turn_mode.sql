UPDATE service_instances
SET spec = spec - 'turn_enabled'
WHERE provider = 'flow'
  AND jsonb_typeof(spec) = 'object'
  AND spec ? 'turn_enabled';
