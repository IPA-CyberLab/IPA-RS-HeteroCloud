UPDATE service_instances
SET spec = jsonb_set(
    spec,
    '{rate_limit}',
    '{"requests_per_second":20,"burst":40}'::jsonb,
    true
)
WHERE provider = 'flow'
  AND jsonb_typeof(spec) = 'object'
  AND NOT spec ? 'rate_limit';
