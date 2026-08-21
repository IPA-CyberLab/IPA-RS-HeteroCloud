UPDATE service_instances
SET spec = jsonb_set(spec, '{ephemeral_storage_gib}', '20'::jsonb, true)
WHERE provider = 'flash'
  AND jsonb_typeof(spec) = 'object'
  AND NOT spec ? 'ephemeral_storage_gib';
