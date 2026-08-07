-- Keep the latest 100 history rows per Flow service. Active contexts remain
-- until they expire so operators can still revoke every valid credential.
WITH ranked AS MATERIALIZED (
    SELECT context_id,
           expires_at,
           revoked_at,
           row_number() OVER (
               PARTITION BY service_instance_id
               ORDER BY issued_at DESC, context_id DESC
           ) AS retention_rank
    FROM flow_access_contexts
)
DELETE FROM flow_access_contexts AS contexts
USING ranked
WHERE contexts.context_id = ranked.context_id
  AND ranked.retention_rank > 100
  AND (ranked.revoked_at IS NOT NULL OR ranked.expires_at <= now());
