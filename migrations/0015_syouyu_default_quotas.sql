UPDATE resource_quota_defaults
SET limits = jsonb_set(
    jsonb_set(limits, '{syouyu,max_buckets}', '10'::jsonb, false),
    '{syouyu,max_total_bytes}', '32212254720'::jsonb, false
), updated_at = now()
WHERE limits #>> '{syouyu,max_buckets}' = '100'
  AND limits #>> '{syouyu,max_total_bytes}' = '107374182400';

UPDATE organization_resource_quotas
SET limits = jsonb_set(
    jsonb_set(limits, '{syouyu,max_buckets}', '10'::jsonb, false),
    '{syouyu,max_total_bytes}', '32212254720'::jsonb, false
), updated_at = now()
WHERE limits #>> '{syouyu,max_buckets}' = '100'
  AND limits #>> '{syouyu,max_total_bytes}' = '107374182400';
