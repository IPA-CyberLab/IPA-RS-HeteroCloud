# Flow, Flash, and Syouyu Live CLI Audit

Date: 2026-09-07 UTC. Final public reads: 05:49:40-05:49:42Z.
Target: https://heterocloud.mizuame.app

## Result

All 15 service command leaves were exercised against the live authenticated API.
This is **not an all-pass result**:

- Flow: complete create/list/get/update/delete lifecycle passed.
- Flash: initial create was accepted but reconciliation reached `error`.
  A name/metadata-only update subsequently reached `ready`; deletion passed.
- Syouyu: create and update were accepted and readable, but readiness never
  completed during observation. Delete timed out after 120 seconds.
  **One audit resource remains pending deletion**, not successfully cleaned up.
- Temporary Keycloak user and credential files were removed. All local audit
  subprocesses were reaped, and the SSH admin session was closed.

Local/mock CLI checks and service data-plane/UDP tests are outside this report.

## Release and Authentication

Used the official Linux x64 binary from the
[v0.1.61 release](https://github.com/IPA-CyberLab/IPA-RS-HeteroCloud/releases/tag/v0.1.61),
not a build containing local fixes. Version output: `heterocloud 0.1.61`.
Published archive checksum verification passed:
`41bc6c308581093e9ca1021c930332bec02391e54657ffaeb21bf1e4a0e0f491`.
The preinstalled 0.1.3 binary was not replaced.

SSH admin access used the user-authorized node and sudo credential.
No existing test CLI key was identified in targeted operational configuration
checks. The existing Keycloak bootstrap-admin credential worked after trimming
its file newline. One expired admin access token caused a 401 on user creation;
refreshing it allowed the same create to succeed with 201.

A uniquely named temporary user was created through the normal Keycloak admin
API, then authenticated through the public OIDC browser flow. Initial local
browser dependency failures were resolved by installing Chromium runtime
libraries/fontconfig/fonts on the coding machine only. One earlier browser
attempt ended with session HTTP 401; its exact cause was not captured. A fresh
normal login succeeded: authentication POST 302, callback 303, session GET 200.

Normal HeteroCloud APIs returned 201 for project, principal, policy, binding, and
API-key creation; logout returned 204. The key was issued with
`expires_in_days: 1`, stored mode 0600 outside git, never printed, and used by
the released CLI through `HETEROCLOUD_API_KEY_FILE`.

The service-account policy has three Allow statements, each restricted to this
new organization, with exactly these actions:

| Resource scope within the audit organization | Actions |
| --- | --- |
| `realtime/*` | `realtime:ListServices`, `GetService`, `CreateService`, `UpdateService`, `DeleteService` |
| `flash/*` | `flash:ListInstances`, `GetInstance`, `CreateInstance`, `UpdateInstance`, `DeleteInstance` |
| `syouyu/bucket/*` | `syouyu:ListBuckets`, `GetBucket`, `CreateBucket`, `UpdateBucket`, `DeleteBucket` |

Each abbreviated action retains its row's prefix. Full resource patterns begin
`hc:org:01a07a62-55b0-7cb3-a503-36f313f2d3a2:`.
The key has no IAM, registry, shell-exec, or other-tenant permissions.

## Fifteen Command Outcomes

All commands used the public HTTPS endpoint, the dedicated organization, JSON
output, and a 120-second convergence limit. Creates used `--no-wait` followed
by CLI reads; Flow/Flash updates used normal waiting, Syouyu update used
`--no-wait` after the audit coordinator bounded further readiness waiting. Deletes used
`--yes` and normal waiting. Lists were filtered to the dedicated project.

| CLI leaf | UTC time | Exit | Actual outcome |
| --- | --- | --- | --- |
| `flow list` | 05:41:10 | 0 | Initially empty; later contained only the created Flow resource |
| `flow create` | 05:41:10 | 0 | Accepted, provisioning generation 1; ready observed 05:41:14 |
| `flow get` | 05:41:14 | 0 | Ready, generation 1; repeated read unchanged |
| `flow update` | 05:41:18 | 0 | Ready generation 2 in 3.159s; renamed resource and metadata persisted |
| `flow delete` | 05:41:21 | 0 | Converged in 3.054s; subsequent GET 404 and empty list |
| `flash list` | 05:41:22 | 0 | Initially empty; later contained only the created Flash resource |
| `flash create` | 05:41:22 | 0 | Accepted, provisioning generation 1; **error** observed 05:41:29 |
| `flash get` | 05:41:30 | 0 | Successfully read the resource in error; repeated read unchanged |
| `flash update` | 05:42:37 | 0 | Ready generation 2 in 66.327s; name and metadata persisted |
| `flash delete` | 05:42:42 | 0 | Converged in 4.851s; subsequent GET 404 and empty list |
| `syouyu list` | 05:42:43 | 0 | Initially empty; later contained only the created Syouyu resource |
| `syouyu create` | 05:42:43 | 0 | Accepted, provisioning generation 1; still provisioning at 05:45:58 |
| `syouyu get` | 05:45:58 | 0 | Provisioning with status {}; repeated read unchanged |
| `syouyu update` | 05:45:58 | 0 | Accepted, updating generation 2; name/metadata persisted; readiness not established |
| `syouyu delete` | 05:47:59 | 1 | Timed out after 120 seconds; resource remained deleting generation 3 |

A zero exit on `create --no-wait` proves acceptance, not successful provisioning.
Successful mutation HTTP codes are not separately asserted: the released CLI
prints resource JSON, not success status codes. The independently observed HTTP
statuses below come from authenticated read-only requests.

Repeated GETs preserved generation and specification for all three services.
Every update changed only the display name (adding `-updated`) and metadata
(adding `phase: updated`). Resource sizes, image, command, ports, bucket name,
and quotas were not changed. An audit-coordinator pause stopped the driver during
Syouyu polling; that process was subsequently terminated/reaped and a bounded
resume driver performed the remaining commands.

## Flash Failure Evidence

Configuration: one replica, 100 millicores, 64 MiB memory, 1 GiB ephemeral storage,
`docker.io/library/busybox:1.37.0`, command `/bin/sleep 600`, no ports, internal
exposure, and disabled workload egress.

At 05:41:29.870907Z the worker logged, for this exact service ID:
`provider reported a permanent service reconciliation failure`.

The original pod's Kubernetes events at 05:41:27 and 05:41:33 reported
`FailedScheduling` with the exact message:

> 0/6 nodes are available: pod has unbound immediate PersistentVolumeClaims. not found

Its PVC was provisioning from 05:41:26 and became provisioned at 05:41:33.
Both original and replacement pods subsequently pulled the same resolved
BusyBox image successfully and started. No image-pull failure was observed.
The event timing and controller's handling of Unschedulable conditions support
a transient PVC/scheduling failure being surfaced as a permanent service error.
This is an evidence-backed inference: the original service status detail was
overwritten by update before it was captured, so its exact status message cannot
be recovered from this audit. Do not describe initial Flash create as passing.

The update did not fix image, CPU, memory, storage, or readiness configuration;
it only changed name/metadata. Ready was observed at generation 2.
After CLI deletion, a read-only Kubernetes listing found no deployment, pod,
PVC, or FlashService matching the audit Flash ID.

## Syouyu Failure and Remaining Cleanup

Configuration: empty bucket, 1 MiB quota, one-object quota; no object uploads or
S3 credentials were created.

Worker logs filtered to the audit service ID show repeated provider delivery
failures for create, update, and delete. The exact request error was:

`error sending request for url (http://heterocloud-syouyu-api.heterocloud-syouyu.svc.cluster.local:8080/internal/v1/service-instances/01a07a63-e99f-7433-9cae-3d0181c7aba5)`

Deletion failures use the same URL with `?generation=3`. The worker message is
`provider delivery will be retried`. Read-only Kubernetes inspection found the
provider Service with **no endpoint addresses**, and all three provider API pods
Running but **0/1 ready**. This explains unavailable provider delivery; the
underlying reason those pods are unready was not investigated beyond this bounded
audit. No provider deployments or DNS were changed.

Exact CLI deletion error:

`error: operation on service 01a07a63-e99f-7433-9cae-3d0181c7aba5 did not converge within 120 seconds`

Remaining resource:
- ID: `01a07a63-e99f-7433-9cae-3d0181c7aba5`
- Display name: `cli-audit-20260907-9f11315d-syouyu-updated`
- Requested bucket name: `cli-audit-20260907-9f11315d-bucket`
- Last observed state: `deleting`, generation 3, at 05:49:41.988Z
- Deletion has been requested through the released CLI; worker retries remain.
- Actual backing-bucket creation or absence was not established. Do not claim
  full resource cleanup or an empty tenant until provider reconciliation completes.

## Final HTTP Reads

Organization prefix:
`/api/v1/organizations/01a07a62-55b0-7cb3-a503-36f313f2d3a2`.

| GET path/suffix | HTTP | Outcome at 05:49:40-05:49:42Z |
| --- | --- | --- |
| `realtime/services?project_id=<audit-project>` | 200 | Empty |
| `realtime/services/01a07a62-7dd5-7281-979d-f9f468d8f1c1` | 404 | not_found |
| `flash/services?project_id=<audit-project>` | 200 | Empty |
| `flash/services/01a07a62-af14-7152-bdbb-886cdff8c7bf` | 404 | not_found |
| `syouyu/buckets?project_id=<audit-project>` | 200 | One item |
| `syouyu/buckets/01a07a63-e99f-7433-9cae-3d0181c7aba5` | 200 | deleting, generation 3 |
| `/api/v1/health/live` | 200 | status ok |
| `/api/v1/health/ready` | 200 | status ready |

Initial unauthenticated session and service-list GETs returned 401.
General API readiness returning 200 did not imply Syouyu provider readiness.

## Retained Metadata and Secret Cleanup

The audit created dedicated metadata within the requested functional-test scope.
The API has no cleanup routes for these IAM and tenant records, so they remain:

| Record | ID |
| --- | --- |
| Organization | `01a07a62-55b0-7cb3-a503-36f313f2d3a2` |
| HeteroCloud user row | `01a07a62-55b0-7cb3-a503-36e799e33cf6` |
| Project | `01a07a62-5d42-78c0-9991-7190ec273502` |
| Service-account principal | `01a07a62-5f71-7e91-a2a9-c0b61aaec446` |
| Policy | `01a07a62-6116-7901-b7ce-810a06981f5f` |
| Binding | `01a07a62-6303-72b3-9812-b2927101fe45` |
| API-key metadata | `01a07a62-6478-7df3-87d4-47f7b6d2054d` |

Project/principal/policy/key name: `cli-audit-20260907-9f11315d`.
The automatically provisioned organization is named
`CLI cli-audit-20260907-9f11315d`; its generated slug is
`user-01a07a6255b07cb3a50336e799e33cf6`.
Automatic user membership/default metadata, audit history, and the pending Syouyu
deletion record may remain. No database rows were forced or removed.

The one-day key expires **2026-09-08T05:41:03.736422596Z**.
Its plaintext file was removed; the server-side key is not claimed revoked.
Deleting its file or Keycloak user does not revoke a service-account key.

Temporary Keycloak user `1122262b-9430-449b-afcb-7e203569b10b` was deleted
through the admin API (204), and subsequent GET returned 404.
Admin refresh-session logout returned 204. The remote password payload was removed,
shell token variables were unset, and SSH session 21864 closed successfully.

The local temporary audit directory, including password, API key, binary,
manifests, helper scripts, and intermediate evidence, was removed after this
sanitized report was written. No credential values are included here.
The original paused PID 50464 was terminated/reaped (exit 143); resume and final
read helpers completed, and no audit processes remain.

Only repository file changed by this audit:
`IPA-RS-HeteroCloud/docs/CLI_LIVE_AUDIT_2026-09-07.md`.
The earlier infra-repo report copy was removed. Unrelated work was preserved.
No commits, pushes, production user/password changes, deployment mutations,
DNS changes, or database mutations were performed.
