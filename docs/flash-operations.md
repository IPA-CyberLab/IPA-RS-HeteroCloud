# Flash operations and recovery

The service console includes measured persistent-disk use, actionable diagnostics,
deployment history, manual rollback and scheduled backups. Organization and owner
consoles have a Flash notifications page. Measurements that cannot be obtained are
shown as unavailable; they are never represented as zero use.

## Placement preflight

Use **配置を確認** in the Flash create or edit form, or run:

```sh
heterocloud flash preflight --file create.json
heterocloud flash preflight SERVICE_ID --file update.json
heterocloud flash preflight SERVICE_ID
```

The first two commands accept the same manifests as `create` and `update`.
Omitting the manifest checks the stored configuration as if started. The check
uses the current allocation limits, then estimates CPU, memory, image and host
disk space, persistent-disk replication, visible GPU availability and required
replica spread. It checks the requested initial replica count and any first
rolling-update surge. Autoscaling ceilings are checked against allocation quotas.

`POST .../flash/services/preflight` requires `flash:CreateInstance`.
`POST .../flash/services/{id}/preflight` requires `flash:UpdateInstance` and
accepts an optional update manifest. The JSON status is `feasible`, `blocked` or
`unavailable`. The latter two include a reason code and produce a nonzero CLI
exit status. Invalid manifests and allocation limits use ordinary API errors.
The preview uses live observations and creates no reservations or resources.

Create and replace-update requests can opt in with `?check_placement=true`.
A definite scheduling blocker returns HTTP 409 `flash_placement_blocked`
before creating or updating the service. When observations are unavailable,
the normal reconciliation and admission checks run. The Coder template uses
this option for workspace writes and continues to wait for real workload
readiness after a successful write.

## Wait for capacity

Choose **空きを待って起動** for a stopped service or enable capacity waiting in
the create form. Coder's **Flash の空き容量を待つ** workspace parameter also enables
this behavior for a new or stopped workspace. The console and Coder wait up to
five minutes for capacity. The CLI accepts a period from 30 to 900 seconds.

```sh
heterocloud flash start SERVICE_ID --wait-for-capacity 300
heterocloud flash get SERVICE_ID
heterocloud flash cancel-start SERVICE_ID REQUEST_ID
```

The first command returns the request ID immediately. Service detail shows its
reason, deadline and state. Once placement is feasible, Flash checks current
allocations again and starts the same service. It allows a further seven minutes
for provider readiness. Cancelling or expiring the queue keeps the service and
home disk. If execution has started for that request, the operation stops only
its generation. A later service edit supersedes the pending request.

`wait_for_capacity_seconds` is an optional query parameter on Flash POST-create,
PUT-replace of a stopped service and POST-start. Read `status.capacity_wait`
from the service. PUT the document `{"cancel_capacity_wait_id":"REQUEST_ID"}`
to the same service to cancel exactly that request using `flash:UpdateInstance`.
The read-only preflight remains available for checking a draft independently.

## Updates

`spec.deployment` is optional. Defaults are `strategy: "auto"`,
`stateless: false`, `deadline_seconds: 600`, `auto_rollback: true`.

Persistent homes and GPU assignments use a stop/update/start deployment. Stateless
services can set `stateless: true` and use rolling updates with zero unavailable
replicas and one additional replica. This requires capacity for both versions.
Existing home disks are retained when switching to stateless operation, but are
not mounted. Setting `stateless` does not make additional administrator-provided
mounts safe for concurrent writers; such mounts use stop/update/start.

The first TCP port supplies a TCP readiness/startup probe. An explicit health
check can instead specify `{ "port": 8080, "path": "/healthz" }`. A service
without a TCP port must provide a health check to verify application readiness;
otherwise process readiness is used. Probes apply on the next user-requested
update, so installing a new provider does not restart unchanged workloads.

```json
{
  "strategy": "rolling",
  "stateless": true,
  "deadline_seconds": 300,
  "auto_rollback": true,
  "health_check": {"port": 8080, "path": "/healthz"}
}
```

If a new revision never becomes healthy before its deadline (60–3600 seconds),
the scheduler re-applies execution settings from the preceding healthy revision.
It checks the original actor's current IAM permissions and fences concurrent
edits by generation. Existing security settings, secrets, network restrictions,
stop state and persistent data remain current. Disks never shrink. The service
must have a previous healthy revision; an initial failed deployment cannot be
automatically rolled back. A revision that became healthy and subsequently fails
is recovered by the runtime, rather than rolling back an unrelated older update.
Zero ready replicas do not qualify a revision as healthy. For scale-to-zero
services the deadline starts on first activation, so time spent idle does not
consume the cold-start health-check budget. Exhausted quotas do not trigger a
runtime rollback.

```sh
heterocloud flash revisions SERVICE_ID
heterocloud flash events SERVICE_ID
heterocloud flash rollback SERVICE_ID --generation 7 --expected-generation 9
```

History summaries do not expose historical environment values. Rollback requires
`flash:UpdateInstance`; current quotas still apply. Running previous revisions
remain metered while a replacement is pending or fails its preflight.

## Scheduled backups

```json
{"enabled":true,"interval_seconds":86400,"retention_count":2,"verify_restore":true}
```

```sh
heterocloud flash backup-policy set SERVICE_ID --file backup-policy.json
heterocloud flash backup-policy get SERVICE_ID
heterocloud flash backup-policy run SERVICE_ID
heterocloud flash backups verify BACKUP_ID
```

The first run is due after enabling; subsequent runs use the configured interval
(1–168 hours). Replicas claim due runs using database leases. Idempotency keys
prevent duplicate backups after retries. Current create/delete permissions and,
when enabled, restore permission are required at execution time.

Retention is 1–8 *scheduled* generations. Manual backups are never pruned. A new
ready and, when requested, verified generation must exist before an older one is
removed. Upload errors, missing permissions or exhausted backup quota retain the
last good generation and are visible in the policy. Backup quotas cover both
manual and scheduled generations; leave capacity for the replacement generation.

Verification restores a separate disk, waits for the physical copy, mounts it
read-only in an isolated verifier, samples file readability and removes the
temporary Job, PVC, PV and Longhorn volume before reporting success. User code is
never started. This checks storage restore and sampled readability, not database
or application consistency. Running backups are crash-consistent; quiesce or stop
an application when it requires application-consistent backups.

Existing services are not automatically opted into daily backups. Configure the
policy per service after checking backup-store space.

## API

Under `/api/v1/organizations/{organization_id}/flash`:

| Method | Path | Permission |
| --- | --- | --- |
| GET | `/services/{id}/revisions`, `/services/{id}/events` | `flash:GetInstance` |
| POST | `/services/{id}/rollback` | `flash:UpdateInstance` |
| GET | `/services/{id}/backup-policy` | `flash:GetInstance` |
| PUT | `/services/{id}/backup-policy` | Backup create/delete and optional restore |
| POST | `/services/{id}/backup-policy/run` | `flash:CreateBackup` |
| POST | `/backups/{id}/verify` | `flash:RestoreBackup` |
| GET | `/events` | `flash:ListEvents` |

Rollback's body is `{ "generation": 7, "expected_generation": 9 }`. An outdated
generation receives 409. A missing backup policy returns JSON `null`.
Cookie-authenticated mutations require the existing Origin and CSRF protection.
