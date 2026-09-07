# CLI Live Recovery Audit

Date: 2026-09-07 UTC. Target: https://heterocloud.mizuame.app.
Status: **COMPLETE**, final authenticated reads 06:21:51-06:21:53Z.
All 15 final recovery leaves completed successfully on CLI 0.1.62 within the
operation limits. All six resources created during this recovery run are deleted.
The final post-correction Syouyu cycle completed promptly; earlier Syouyu delivery
retries and the rollout-time failed create remain documented separately below.

## Operational Status

| Stage | Latest independent outcome |
| --- | --- |
| Auth and isolated IAM setup | Complete; one-day service-only key |
| Syouyu first rollout-time attempt, CLI 0.1.61 | Create timed out; update recovered; deleted and verified absent |
| Syouyu fresh retest, CLI 0.1.62 | Create ready in 3.032s; update ready in 134.267s after transport retries; delete verified |
| Flow, CLI 0.1.62 | All five commands passed; HTTP 404/list empty at 06:14:19Z |
| Flash rollout | Independently verified image 0.1.24, API updated/ready 3/3, controller updated/ready 2/2 |
| Flash lifecycle | Initial create ready generation 1 in 67.980s without update; update/delete passed; cleanup HTTP 404/list empty 06:16:23Z |
| Previous audit's pending Syouyu | Parent read-only verification: automatic deletion completed at 06:50:05.6206Z on worker 0.1.63 |
| Final Syouyu transport retest | All five leaves passed; create 2.938s, update 3.075s, delete 2.611s; HTTP 404/list empty |
| Identity/credential cleanup | Keycloak user removed; secret files removed; all test processes reaped and SSH closed |

## Scope and Provenance

Independent ordinary authenticated lifecycle testing only. No source changes,
deployment changes, DNS mutation, database mutation, or data-plane tests were
performed by this audit. Parent-owned platform changes are not audit actions.

The initial recovery attempt used released CLI 0.1.61. Subsequent recovery tests
use the official Linux x64 CLI 0.1.62 release artifact, verified against its
published checksum, and reporting `heterocloud 0.1.62`.
Archive SHA-256:
`7fdef449eb2d75330ed5f57edb71c8beff829ba1fead444167cc121c4c96b2cd`.

Each CLI invocation has a hard 180-second harness limit and a configured
180-second CLI convergence limit. Creates use normal waiting, not `--no-wait`.
Read-only collection observations during create capture resource IDs for cleanup.
Updates change display name and audit metadata only. Deletes use `--yes`.

## Authorized Identity

A new temporary Keycloak user was created through the normal admin API (201),
then signed in through public OIDC: authentication POST 302, callback 303,
session GET 200. Normal project, principal, policy, binding, and API-key creation
each returned 201. HeteroCloud logout returned 204.

Name prefix: `cli-audit-recovery-20260907-1f706a17`.
The one-day key was requested with `expires_in_days: 1` and stored mode 0600
outside git, without printing its value. Its policy grants exactly the 15
Flow/Flash/Syouyu list/get/create/update/delete actions, scoped only to the new
organization's `realtime/*`, `flash/*`, and `syouyu/bucket/*` resource patterns.
It has no IAM, registry, exec, or other-tenant permissions.

| Retained audit metadata | ID |
| --- | --- |
| Organization | `01a07a78-cf7e-7793-abbd-44dd0249f3ea` |
| HeteroCloud user row | `01a07a78-cf7e-7793-abbd-44c7b21f15fe` |
| Project | `01a07a78-d8a1-7241-b6a6-4826a731e16d` |
| Service-account principal | `01a07a78-d9d9-7452-bdce-2b2b9f2c17a9` |
| Policy | `01a07a78-dc27-7272-b9be-48a77d5f15a4` |
| Binding | `01a07a78-ddb2-7600-bd4c-ad4bf8b5b8fa` |
| API-key metadata | `01a07a78-dfca-7dd3-8865-800cece1a7b4` |

Key expiry: **2026-09-08T06:05:37.098735804Z**.
Temporary Keycloak user: `7ae13b2a-40dc-4bc1-b864-bfb6a07430d7`.
Durable tenant/IAM metadata is explicitly authorized to remain; this does not
waive cleanup of actual test resources, credential files, or the Keycloak user.

## Rollout-Time Findings

The parent reported database NetworkPolicy targetPort and host-network worker
ingress corrections. Independent read-only inspection confirmed all three
Syouyu API pods ready, but that alone did not prove worker delivery worked.
The parent subsequently identified different source addresses for direct Pod
routes (flannel .0) and Service routes (cni0 .1), and applied exact node .1/32
allow entries in addition to .0/32 entries. This root-cause explanation is parent
evidence; the audit independently observed request-send failures and slow recovery,
not the original underlying TCP error. A read-only check confirmed the identified
retrying worker is hostNetwork on mizuame-nucboxg5, podIP 10.250.0.3, with
ClusterFirstWithHostNet DNS policy.
At approximately 06:20Z, bounded read-only commands in that existing worker
resolved the provider Service to 10.98.102.184 and performed a TCP/HTTP request
to `/health/ready`, which returned `HTTP/1.0 200 OK` (command exit 0).
This is current connectivity evidence, not a recovered nested cause for historical
request errors; those logs expose only reqwest's outer request-send message.

The first recovery Syouyu resource,
`01a07a79-6f6a-7f33-9215-627f1dbf0b78`, was accepted at 06:06:13Z. Its waited
create was terminated by the 180-second harness limit at 06:09:13Z; GET still
showed provisioning generation 1. Worker logs for this exact ID showed repeated
`error sending request for url` to the normal internal Syouyu provider endpoint,
including retries at 06:09:22Z, 06:09:31Z, and 06:09:47Z.

A name/metadata-only update reached ready generation 2 at 06:10:23Z in 67.989s.
Deletion passed in 2.831s; subsequent CLI GET returned 404, and independent HTTP
verification returned 404 with list HTTP 200/count 0 at 06:10:31Z. This resource
is cleaned up. Its initial create remains a failed convergence attempt, not a pass.

The CLI 0.1.61 Flow baseline also passed all five commands. Resource
`01a07a7d-5e1e-7e43-856e-bb8842b808e1` reached ready on create in 2.659s,
updated to ready generation 2 in 2.960s, and deleted in 2.804s. Cleanup was
independently verified HTTP 404/list count 0 at 06:10:44Z.

## CLI 0.1.62 Retest

Fresh Syouyu ID: `01a07a7e-6e44-7ae0-bd72-4c53c89d914a`.
Create reached ready generation 1 at 06:11:44Z in 3.032s without a manual update.
Repeated GETs preserved generation and specification. Subsequent update encountered
further provider transport retries, including worker pod
`heterocloud-heterocloud-worker-68b98c6f4b-6f6gt`; update eventually reached ready
generation 2 at 06:14:00Z in 134.267s. Name and metadata persisted. Delete passed
in 5.226s; direct HTTP cleanup verification returned 404 and list HTTP 200/count 0
at 06:14:07Z. This is a successful bounded lifecycle with observed intermittent
delivery errors, not evidence of error-free provider networking.

After the parent confirmed the final .0 plus .1 ingress correction, a third,
fresh Syouyu resource was tested on CLI 0.1.62:
`01a07a86-9185-75f3-b17e-d41998151741`, named
`cli-audit-recovery-20260907-1f706a17-syouyu-r3` (bucket name ends in `-bucket-r3`).
Initial create reached ready generation 1 in 2.938s at 06:20:37Z, without update.
Repeated GETs preserved generation/specification; update reached ready generation
2 in 3.075s and persisted name/metadata. Delete passed in 2.611s at 06:20:45Z.
Cleanup was independently verified HTTP 404/list HTTP 200/count 0 at 06:20:46Z.
No retry or error was surfaced by the CLI during this final cycle. This bounded
sample does not prove the absence of all possible intermittent networking faults.

Fresh CLI 0.1.62 Flow ID: `01a07a80-abb8-7d31-bf0f-feb3fda1adfe`.
Create reached ready in 3.195s; repeated GETs were unchanged. Update reached
ready generation 2 in 2.768s and persisted name/metadata. Delete passed in
2.649s; independent HTTP verification returned 404/list count 0 at 06:14:19Z.

Flash recovery testing started after parent deployment confirmation and an
independent read-only check showing API updated/ready 3/3 and controller
updated/ready 2/2, all image `ghcr.io/ipa-cyberlab/ipa-rs-heterocloud-flash:0.1.24`.
Initial create began at 06:14:47Z and reached ready generation 1 at 06:15:55Z
in 67.980s without any update. ID: `01a07a81-4669-7450-92ca-90db4463f331`.
Repeated GETs were unchanged. Update reached ready generation 2 at 06:16:14Z
in 16.402s; the name/metadata change persisted. Delete passed in 2.825s, and
independent HTTP cleanup reads returned 404/list HTTP 200/count 0 at 06:16:23Z.

### Fifteen-Command Recovery Matrix

All rows below use released CLI 0.1.62. A successful CLI operation prints JSON,
not its success HTTP status; separately observed HTTP cleanup statuses are
reported explicitly rather than inferred from command success. Syouyu rows use
the final post-correction cycle, not the earlier slow-but-successful attempt.

| Leaf | UTC completion | Exit | Outcome |
| --- | --- | --- | --- |
| flow list | 06:14:07 | 0 | Empty initially; own single resource after create |
| flow create | 06:14:10 | 0 | Ready generation 1; 3.195s |
| flow get | 06:14:12 | 0 | Ready; repeat unchanged |
| flow update | 06:14:16 | 0 | Ready generation 2; 2.768s; name/metadata persisted |
| flow delete | 06:14:19 | 0 | 2.649s; subsequent GET 404/list empty |
| flash list | 06:14:47 | 0 | Empty initially; own single resource after create |
| flash create | 06:15:55 | 0 | Ready generation 1 without update; 67.980s |
| flash get | 06:15:57 | 0 | Ready; repeat unchanged |
| flash update | 06:16:14 | 0 | Ready generation 2; 16.402s; name/metadata persisted |
| flash delete | 06:16:22 | 0 | 2.825s; subsequent GET 404/list empty |
| syouyu list | 06:20:34 | 0 | Empty initially; own single resource after create |
| syouyu create | 06:20:37 | 0 | Ready generation 1 without update; 2.938s |
| syouyu get | 06:20:39 | 0 | Ready; repeat unchanged |
| syouyu update | 06:20:42 | 0 | Ready generation 2; 3.075s; name/metadata persisted |
| syouyu delete | 06:20:45 | 0 | 2.611s; subsequent GET 404/list empty |

Flow used one room/participant and a small rate limit. Syouyu used an empty
1 MiB/one-object bucket; no S3 credentials or objects were created. Test updates
did not alter image, command, resource limits, ports, bucket name, or quotas.

### Flash Scheduling Evidence

The audit pod had FailedScheduling events at 06:14:51Z and 06:14:53Z with
`0/6 nodes are available: pod has unbound immediate PersistentVolumeClaims. not found`.
The PVC provisioned and pod scheduled at 06:14:53Z; volume attachment succeeded
at 06:15:12Z and the container started at 06:15:26Z. Read observations transitioned
from provisioning to ready, generation 1, without an observed error state.
The same small Flash specification was used as in the initial audit: one replica,
100 millicores, 64 MiB memory, 1 GiB ephemeral storage, BusyBox 1.37.0 running
`/bin/sleep 600`, no ports, internal exposure, and disabled workload egress.
No HTTP 503 was surfaced in this audit's Flash command output. Provider readiness
HTTP codes during pending startup were not captured, so an independent readiness
503 must not be confused with the successful final CLI create/update/delete results.

## Previous Audit Resource

The earlier audit's key was destroyed and its Keycloak user deleted. No attempt
was made to access that organization with this new tenant's key or fabricate
authorization. Bounded authorized worker-log reads for old resource
`01a07a63-e99f-7433-9cae-3d0181c7aba5` showed another deletion-delivery retry at
06:11:36Z, attempt 13. A later bounded log query did not return a completion
record. This audit does not claim that old resource is absent or still present
now: no current authorized tenant read was available. Parent read-only
verification can supplement this record without changing database rows.

Parent follow-up at approximately 06:30Z used authorized read-only PostgreSQL
queries: this ID had no Syouyu service row and zero Syouyu bucket rows, while
the HeteroCloud service row remained. Worker logs showed attempts 14-16 rejected
with provider HTTP 404. Thus cleanup was blocked on deleting a resource whose
creation never reached the provider, not on removing stored objects. No database
rows were manually changed.

The follow-up worker correction was released as **0.1.63**, commit `dc933bd`.
Only DELETE plus HTTP 404 plus a complete, bounded JSON provider error envelope
with code `not_found` acknowledges an absent resource. Generic proxy responses,
authentication errors, and reconcile 404s remain failures. Existing generation,
organization, project, provider, and database completion guards remain enforced.
Thirteen focused worker tests, strict Clippy, and formatting passed. Release run
`34091770437` passed validation, image publication, and all three CLI builds;
the Linux release job also passed the CLI contract audit.

GitOps commit `7ed0c2b2` deploys the release. API and owner-console rollouts
completed and all four workers were updated/ready by 06:49Z. Public readiness
returned HTTP 200. The published Linux CLI checksum was verified and the local
CLI updated to 0.1.63. The fifteen-command tenant lifecycle matrix above was run
on 0.1.62, not rerun under a newly created account for this worker-only fix.

At **06:50:05.6206Z**, the existing outbox event completed on its normal retry
(attempt 22). A subsequent parent read-only query confirmed zero matching
HeteroCloud service rows and a populated `delivered_at` timestamp on event
`01a07a66-e72a-7d23-9d60-cad7bb47e417`. This clears the previous audit's pending
resource without manual database updates or another tenant credential. Argo CD
reported `Synced Healthy`. The escape Pod UID and restart count remained unchanged
after this rollout too.

## Parent Network Verification

After the final ingress correction, Service readiness returned HTTP 200 three
times from each of all six cluster nodes (18/18). The network preflight reported
five database targets, four worker nodes, three ready API endpoints, and no
failures. The ingress policy permits the specific host routing source addresses,
not the whole tenant Pod CIDR. New worker-node source addresses must be included
in this configuration; the preflight detects missing entries.

The existing escape workload retained Pod UID
`08942b6b-7a5d-408d-9a88-16acfbe24b40` and restart count zero across the Flash
API/controller rollout. These parent checks are separate from the independent
CLI audit.

## Cleanup

Final authenticated GETs at 06:21:51-06:21:53Z verified every recovery-run ID:

| Resource | ID | HTTP |
| --- | --- | --- |
| Syouyu rollout-time attempt | `01a07a79-6f6a-7f33-9215-627f1dbf0b78` | 404 |
| Flow 0.1.61 baseline | `01a07a7d-5e1e-7e43-856e-bb8842b808e1` | 404 |
| Syouyu 0.1.62 intermediate retest | `01a07a7e-6e44-7ae0-bd72-4c53c89d914a` | 404 |
| Flow 0.1.62 final retest | `01a07a80-abb8-7d31-bf0f-feb3fda1adfe` | 404 |
| Flash 0.1.24 provider retest | `01a07a81-4669-7450-92ca-90db4463f331` | 404 |
| Syouyu post-correction final retest | `01a07a86-9185-75f3-b17e-d41998151741` | 404 |

All three project-filtered service lists returned HTTP 200 with zero items.
Public `/api/v1/health/live` and `/api/v1/health/ready` returned HTTP 200 with
status `ok` and `ready`, respectively. No recovery-run service resource remains.

Temporary Keycloak user `7ae13b2a-40dc-4bc1-b864-bfb6a07430d7` was deleted through
the normal admin API (204), and subsequent GET returned 404. Admin refresh-session
logout returned 204. The remote generated password payload was removed, shell
credential variables unset, and SSH session 47327 closed successfully.

The local temporary directory `/tmp/cli-audit-recovery.PHnEk4`, including generated
password, API-key plaintext, binaries, manifests, helper scripts, and intermediate
evidence, was removed after this sanitized report was written. All CLI/browser
drivers and diagnostic subprocesses completed or were stopped and reaped. No
secret values were written to this report or git.

The dedicated organization, user/membership/default metadata, project, principal,
policy, binding, API-key metadata, and audit history remain as authorized.
The key expires **2026-09-08T06:05:37.098735804Z**; deleting its plaintext and
Keycloak user does not revoke a service-account key, and revocation is not claimed.

Only repository file changed by this recovery audit:
`docs/CLI_LIVE_RECOVERY_2026-09-07.md` in IPA-RS-HeteroCloud.
No source/deployment changes, commits, or pushes were made by this audit.
