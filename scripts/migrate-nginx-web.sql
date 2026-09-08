-- One-off DBA maintenance, not an authenticated API request or a schema migration.
-- Requires deployed Flash web support and PodTemplate-preserving reconciliation.
-- No Kubernetes objects, credentials, sessions, IAM policies, or image fields are changed.
-- Review the target's full spec privately before supplying its fingerprint.
-- Preflight (read-only):
-- SELECT id, organization_id, generation, state, name,
--        encode(sha256(convert_to(spec::text, 'UTF8')), 'hex') AS spec_sha256,
--        spec->'exposure' AS exposure, spec->'ports' AS ports
-- FROM service_instances WHERE id = '01a02a0c-c65f-7802-b5be-a4efe04d0f69';
-- Select an existing authorized owner principal, not a newly invented identity:
-- SELECT m.principal_id, m.organization_id, p.name FROM organization_memberships m
-- JOIN principals p ON p.id = m.principal_id
-- WHERE m.role = 'owner' AND p.enabled;
-- Run with psql -X using existing DBA connectivity, no passwords on the command line:
-- psql -X -v expected_org=019fbda8-eff3-7610-bdd5-8fe89e746c14 \
--   -v actor_principal=UUID -v expected_generation=2 \
--   -v expected_spec_sha256=HEX -v operator_ref='operator/change-reference' \
--   -f scripts/migrate-nginx-web.sql
-- Default is ROLLBACK. After reviewing the dry run, repeat with -v apply=true.
-- Never auto-retry a commit with an uncertain outcome: inspect the row/outbox first.
-- This mirrors the bounded store mutation and reservation checks, not the full Rust
-- validator/IAM evaluator. All unchanged fields rely on the reviewed stored spec.
\set ON_ERROR_STOP on
\if :{?apply}
\else
\set apply false
\endif

BEGIN;
SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '15s';
SET LOCAL idle_in_transaction_session_timeout = '30s';
CREATE TEMP TABLE flash_web_migration_input ON COMMIT DROP AS
SELECT :'expected_org'::uuid AS organization_id,
       :'actor_principal'::uuid AS principal_id,
       :'expected_generation'::bigint AS generation,
       :'expected_spec_sha256'::text AS fingerprint,
       :'operator_ref'::text AS operator_ref;

DO $migration$
DECLARE
    input record;
    service service_instances%ROWTYPE;
    quota jsonb;
    totals record;
    reserved numeric;
    next_spec jsonb;
    event_id uuid := gen_random_uuid();
    before_hash text;
    after_hash text;
BEGIN
    SELECT * INTO STRICT input FROM flash_web_migration_input;
    IF input.fingerprint !~ '^[0-9a-f]{64}$'
       OR input.generation <> 2
       OR input.organization_id <> '019fbda8-eff3-7610-bdd5-8fe89e746c14'::uuid
       OR length(trim(input.operator_ref)) NOT BETWEEN 1 AND 200 THEN
        RAISE EXCEPTION 'Invalid expected fingerprint, generation, or operator reference';
    END IF;

    -- Same lock order and keys as Store::update_service_instance for Flash.
    PERFORM pg_advisory_xact_lock(hashtextextended(input.organization_id::text, 1));
    PERFORM pg_advisory_xact_lock(hashtextextended('heterocloud-flash-allocation', 0));
    SELECT * INTO STRICT service FROM service_instances
    WHERE id = '01a02a0c-c65f-7802-b5be-a4efe04d0f69'
      AND organization_id = input.organization_id AND provider = 'flash'
      AND project_id = '019fbdaa-c85e-7950-b0f1-356e78da4b6e'
    FOR UPDATE;
    before_hash := encode(sha256(convert_to(service.spec::text, 'UTF8')), 'hex');
    IF service.generation <> input.generation OR before_hash <> input.fingerprint
       OR service.state <> 'ready' THEN
        RAISE EXCEPTION 'Target generation/spec/state changed; re-inspect, do not retry blindly';
    END IF;
    IF EXISTS (SELECT 1 FROM outbox_events WHERE aggregate_id = service.id
               AND delivered_at IS NULL) THEN
        RAISE EXCEPTION 'Target has pending outbox work; wait for reconciliation';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM principals p
        JOIN organization_memberships m ON m.principal_id = p.id
          AND m.organization_id = p.organization_id AND m.user_id = p.user_id
        JOIN users u ON u.id = p.user_id
        WHERE p.id = input.principal_id AND p.organization_id = input.organization_id
          AND p.enabled AND p.kind = 'user' AND m.role = 'owner' AND u.status = 'active'
    ) THEN
        RAISE EXCEPTION 'An existing enabled active owner principal in the target tenant is required';
    END IF;
    IF service.spec #>> '{exposure,endpoint_mode}' IS DISTINCT FROM 'load_balancer'
       OR service.spec #>> '{exposure,type}' IS DISTINCT FROM 'public'
       OR service.spec #>> '{exposure,traffic_mode}' IS DISTINCT FROM 'forwarded'
       OR coalesce(service.spec #> '{exposure,allowed_source_cidrs}', '[]'::jsonb) <> '[]'::jsonb
       OR coalesce(service.spec #> '{exposure,denied_source_cidrs}', '[]'::jsonb) <> '[]'::jsonb
       OR jsonb_array_length(service.spec->'ports') IS DISTINCT FROM 1
       OR service.spec #>> '{ports,0,protocol}' IS DISTINCT FROM 'tcp'
       OR service.spec #>> '{ports,0,container_port}' IS DISTINCT FROM '80'
       OR service.spec #>> '{ports,0,service_port}' IS DISTINCT FROM '30000' THEN
        RAISE EXCEPTION 'Target is not the reviewed public/forwarded TCP80:30000 service with empty ACLs';
    END IF;
    IF EXISTS (
        SELECT 1 FROM service_instances s, jsonb_array_elements(s.spec->'ports') port
        WHERE s.provider = 'flash' AND s.state <> 'deleting' AND s.id <> service.id
          AND port->>'protocol' = 'tcp' AND port->>'service_port' = '30000'
    ) THEN
        RAISE EXCEPTION 'TCP30000 conflicts; this migration will not reassign ports';
    END IF;

    SELECT coalesce(q.limits, d.limits)->'flash' INTO STRICT quota
    FROM organizations o CROSS JOIN resource_quota_defaults d
    LEFT JOIN organization_resource_quotas q ON q.organization_id = o.id
    WHERE o.id = input.organization_id AND d.singleton = true;
    IF quota IS NULL OR NOT quota ?& ARRAY[
        'max_services', 'max_replicas_per_service', 'max_cpu_millis_per_vm',
        'max_memory_mib_per_vm', 'max_disk_gib_per_vm', 'max_total_replicas',
        'max_total_cpu_millis', 'max_total_memory_mib', 'max_total_disk_gib'
    ] OR EXISTS (SELECT 1 FROM jsonb_each(quota) item WHERE item.value = 'null'::jsonb) THEN
        RAISE EXCEPTION 'Incomplete Flash quota configuration';
    END IF;
    reserved := coalesce(service.spec #>> '{autoscaling,max_replicas}', service.spec->>'replicas')::numeric;
    IF reserved > (quota->>'max_replicas_per_service')::numeric
       OR (service.spec->>'cpu_millis')::numeric > (quota->>'max_cpu_millis_per_vm')::numeric
       OR (service.spec->>'memory_mib')::numeric > (quota->>'max_memory_mib_per_vm')::numeric
       OR coalesce(service.spec->>'ephemeral_storage_gib', '10')::numeric > (quota->>'max_disk_gib_per_vm')::numeric THEN
        RAISE EXCEPTION 'Current service exceeds a per-service/VM quota';
    END IF;
    -- Only endpoint mode changes, so replacement totals equal current active totals.
    SELECT count(*) AS services, sum(replicas) AS replicas,
           sum(replicas * cpu) AS cpu, sum(replicas * memory) AS memory,
           sum(replicas * disk) AS disk INTO totals
    FROM (
        SELECT coalesce(spec #>> '{autoscaling,max_replicas}', spec->>'replicas')::numeric AS replicas,
               (spec->>'cpu_millis')::numeric AS cpu, (spec->>'memory_mib')::numeric AS memory,
               coalesce(spec->>'ephemeral_storage_gib', '10')::numeric AS disk
        FROM service_instances WHERE organization_id = input.organization_id
          AND provider = 'flash' AND state <> 'deleting'
    ) allocations;
    IF totals.services > (quota->>'max_services')::numeric
       OR totals.replicas > (quota->>'max_total_replicas')::numeric
       OR totals.cpu > (quota->>'max_total_cpu_millis')::numeric
       OR totals.memory > (quota->>'max_total_memory_mib')::numeric
       OR totals.disk > (quota->>'max_total_disk_gib')::numeric THEN
        RAISE EXCEPTION 'Current tenant reservations exceed quota';
    END IF;

    next_spec := jsonb_set(service.spec, '{exposure,endpoint_mode}', '"web"'::jsonb, false);
    IF (next_spec #- '{exposure,endpoint_mode}') IS DISTINCT FROM
       (service.spec #- '{exposure,endpoint_mode}') THEN
        RAISE EXCEPTION 'Unexpected spec changes';
    END IF;
    after_hash := encode(sha256(convert_to(next_spec::text, 'UTF8')), 'hex');
    UPDATE service_instances SET spec = next_spec, generation = service.generation + 1,
        state = 'updating', status = '{}'::jsonb, updated_at = now()
    WHERE id = service.id;
    INSERT INTO outbox_events (id, topic, aggregate_id, payload)
    VALUES (event_id, 'service-instance.reconcile', service.id, jsonb_build_object(
        'service_instance_id', service.id, 'organization_id', service.organization_id,
        'project_id', service.project_id, 'principal_id', input.principal_id,
        'provider', 'flash', 'generation', service.generation + 1
    ));
    INSERT INTO audit_events (organization_id, principal_id, user_id, request_id,
        source_ip, action, resource, decision, reason, metadata)
    VALUES (service.organization_id, input.principal_id, NULL, event_id::text,
        inet_client_addr(), 'flash:UpdateInstance',
        format('hc:org:%s:flash/instance/%s', service.organization_id, service.id),
        'allow', 'explicit_dba_maintenance', jsonb_build_object(
            'authentication', jsonb_build_object('actor', 'database_operator',
                'session_user', session_user, 'current_user', current_user),
            'operator_ref', input.operator_ref, 'outbox_event_id', event_id,
            'previous_generation', service.generation, 'generation', service.generation + 1,
            'previous_spec_sha256', before_hash, 'spec_sha256', after_hash,
            'change', jsonb_build_object('exposure.endpoint_mode', jsonb_build_array('load_balancer', 'web'))
        ));
    RAISE NOTICE 'Prepared service %, generation % -> %, outbox %, spec SHA256 % -> %',
        service.id, service.generation, service.generation + 1, event_id, before_hash, after_hash;
END
$migration$;

\if :apply
COMMIT;
\echo 'Migration committed; normal worker/provider reconciliation must now complete.'
\else
ROLLBACK;
\echo 'DRY RUN rolled back service/outbox/audit changes. Repeat reviewed inputs with -v apply=true to commit.'
\endif
