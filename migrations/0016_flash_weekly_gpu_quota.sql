UPDATE resource_quota_defaults
SET limits = jsonb_set(
    limits,
    '{flash,max_weekly_gpu_seconds}',
    '40320'::jsonb,
    true
), updated_at = now()
WHERE NOT (limits->'flash' ? 'max_weekly_gpu_seconds');

UPDATE organization_resource_quotas
SET limits = jsonb_set(
    limits,
    '{flash,max_weekly_gpu_seconds}',
    '40320'::jsonb,
    true
), updated_at = now()
WHERE NOT (limits->'flash' ? 'max_weekly_gpu_seconds');
