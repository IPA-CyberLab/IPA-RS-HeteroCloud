# dYdx workspace start failure on 2026-11-07

The Coder `dYdx` start build failed after reporting insufficient CPU or memory.
The Flash controller kept the previous revision and the persistent home while
repeatedly attempting the requested revision. At investigation time the live
container used 4 vCPU and 8 GiB memory; the home PVC remained bound to
`pvc-211bb193-20c1-4729-9234-e257122f8211`.

The message described only one candidate's failure. `uc-k8sp5` lacked CPU
request headroom, while `ichikawap1` had compute headroom but failed the disk
reserve check. The capacity ledger retained a reservation for the already
running dYdx Pod. Dry-run preflight excluded that Pod for the `Recreate`
deployment but then counted its retained reservation as an additional pending
Pod. In a read-only reconstruction, `ichikawap1` had about 178.8 GiB free,
with a 147.5 GiB safety floor, 12.6 GiB of remaining existing writable
budgets, a 10 GiB new writable budget, and a duplicate 16 GiB reservation.
The retained reservation alone made the replacement fail admission.

The ledger's first observation of a running Pod set `running_at` only in
memory. A dry-run preflight never wrote that timestamp, so successive dry-runs
could perpetually restart the two-minute timer. The fix derives elapsed time
from the running containers, requires fresh node and image filesystem metrics
and a matching Pod sample, and retains reservations when those observations
are missing. Restartable init sidecars must also be accounted for. Read-only
evaluation against the production inventory then selected `ichikawap1` without
lowering disk or CPU safety thresholds. When candidates fail for different
reasons, disk shortage is no longer hidden by another node's CPU shortage.

The Coder waiter also exited after three repeated capacity warnings, although
Flash continues retrying. It now reports the warning and waits through the
configured deadline, returning an error if the latest diagnosis persists.
Terraform reconciled the reviewed change into both Coder templates while
preserving their current variables and workspace images. Build 9 then succeeded,
the dYdx agent connected, and the code-server health check passed. The original
PVC and volume were retained.

The home filesystem has approximately 26 GiB allocated and was 100% full.
File Browser initially had not completed its startup download; it later started
and returned HTTP 200 on its local endpoint. Browser access was not verified.
Outbound GitHub requests timed out during investigation, although later probes
succeeded; this is an intermittent observation, not a diagnosis of the network
path. The disk remained full and the volume was attached but degraded with only
one read/write replica. Longhorn attempted to rebuild another replica, but it
had not become healthy during verification. No files were deleted and no disk
expansion was performed while the requested size change was awaiting the
owner's response.

The capacity service's admission workers also became saturated and caused
its shared liveness endpoint to return 503, resulting in repeated restarts.
Its liveness check now uses a loopback-only listener with a main-loop heartbeat;
readiness continues to reflect admission saturation. Usage queries can reuse a
recent measured snapshot and busy admission/refresh locks fail closed promptly,
instead of allowing waiting requests to occupy every worker. Explicitly stopped
and unready nodes are excluded from kubelet metrics requests. Tests exercised the
actual HTTP preflight path, safe reservation expiry, stale/missing samples,
sidecars, concurrent usage, saturation, and a stuck listener. The
[capacity image workflow](https://github.com/IPA-CyberLab/IPA-RS-HeteroCloud-Flash/actions/runs/42371306371)
and [Coder template checks](https://github.com/IPA-CyberLab/IPA-RS-HeteroCloud/actions/runs/42369423297)
passed. These tests and a successful workspace build do not prove recovery of
the missing storage replica or establish an independent backup.
