UPDATE resource_quota_defaults
SET limits = jsonb_set(
    limits,
    '{syouyu}',
    '{
      "max_buckets": 100,
      "max_bytes_per_bucket": 10737418240,
      "max_objects_per_bucket": 1000000,
      "max_total_bytes": 107374182400,
      "max_credentials_per_bucket": 10,
      "max_total_credentials": 1000
    }'::jsonb,
    true
), updated_at = now()
WHERE NOT (limits ? 'syouyu');

UPDATE organization_resource_quotas
SET limits = jsonb_set(
    limits,
    '{syouyu}',
    '{
      "max_buckets": 100,
      "max_bytes_per_bucket": 10737418240,
      "max_objects_per_bucket": 1000000,
      "max_total_bytes": 107374182400,
      "max_credentials_per_bucket": 10,
      "max_total_credentials": 1000
    }'::jsonb,
    true
), updated_at = now()
WHERE NOT (limits ? 'syouyu');

CREATE UNIQUE INDEX service_instances_syouyu_bucket_name_idx
    ON service_instances ((spec->>'bucket_name'))
    WHERE provider = 'syouyu';
