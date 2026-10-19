# HyperEVM IDE unavailable after persistent home exhaustion, 2026-10-19

The reported code-server URL was unavailable while the Coder login page was
healthy. HyperEVM's Coder agent remained connected and its Flash container was
Running, with zero container restarts. Its IDE was no longer listening on port
13337. Existing Codex processes were still running.

The persistent home had no free space. Its requested capacity was 26.1 GiB;
project files occupied about 16.7 GiB and Codex state about 6.7 GiB. The latter
was preserved. IDE logs contained repeated `ENOSPC` and failed log flushes.
The old supervisor stored its lock and logs in the same persistent home and used
`set -e`: an unsuccessful diagnostic write after a child exit terminated the
supervisor itself. That retry failure was reproduced with the actual workspace
image and gVisor. The precise reason the original IDE child exited on October 19
was not captured; the full filesystem also prevented its final logs from being
written. This incident is not evidence of a host reboot or container OOM.

Recovery removed approximately 80 MiB of regenerable Go build cache and started
the IDE again. No projects, Codex state, IDE settings or extensions were deleted.
Authenticated Chromium rendered the VS Code workbench at 14:29:58 UTC with
HTTP 200 and no server errors. An anonymous request that merely redirects to
the Coder login page is insufficient verification for this application.

The Flash total disk budget was increased from 37 to 45 GiB through its API,
retaining the 10 GiB container filesystem. Longhorn expanded the existing
persistent volume by 8 GiB online. Its PVC UID, PV, workspace Pod UID and
container ID were preserved. The home had approximately 8 GiB free afterwards.
CPU remains 4 vCPU and memory remains **8 GiB**.

The corrected supervisor keeps its script, lock and diagnostic logs on the
container filesystem in `/tmp/coder-code-server-<uid>`. Log failures cannot
terminate retries. It retains single-instance locking, rotates diagnostics
between launches, and can adopt an existing healthy IDE without restarting it.
The replacement was installed live; both the IDE and existing Codex process IDs
were retained. The existing agent's startup-script record was updated from the
validated template after an exact comparison with the old script, so automatic
container recreation also loads the corrected supervisor.

The imported and activated `standard-develop` template retains all existing
template inputs and other workspace allocations. HyperEVM's 45 GiB disk budget
and 8 GiB memory allocation are recorded in
[`deploy/coder/workspace-overrides.json`](../../deploy/coder/workspace-overrides.json).
Existing workspaces were not rebuilt. As usual, a deliberately selected older
immutable template version retains its original inputs.

Validation includes nine supervisor/scheduling checks and eight Terraform
allocation tests. The native regression uses a separate 64 MiB Longhorn PVC
and the actual immutable workspace image under gVisor. It verifies a real
NFS `ENOSPC`, the old supervisor's fatal diagnostic write, the corrected
supervisor surviving an IDE exit, HTTP 200 after space is freed, unchanged
fixture Pod/container identity, and retained file contents. `/dev/full` makes
the legacy diagnostic failure deterministic if the exiting IDE frees the last
filesystem block. No fault was injected into a user workspace.

The first fixture's uncached image pull on p5 induced ephemeral-storage pressure
and evicted the existing Whisper Pod. Subsequent Whisper image pulls repeatedly
interrupted its recovery. This was an impact introduced by this verification;
the original HyperEVM outage preceded the test. The fixture was stopped before
fault injection, but that alone did not restore sufficient host disk headroom.

Whisper's existing persistent disk was retained. The GPU host's root LV was
grown online from 200 to 220 GiB using 20 GiB of existing unused VG extents.
Its root filesystem then had approximately 50 GiB free. The declared device,
filesystem and capacity are managed by the Network repository's
`node_root_storage_policy` Terraform resource and guarded Ansible reconciler.
The reconciler refuses an unexpected root device, filesystem, shrink, or
insufficient free extents. It does not restart the host or container runtime.
Whisper recovered; its public `/healthz` returned `ready`, `model_loaded: true`
and `device: cuda:0`, with the GPU model using approximately 1817 MiB.
The obsolete failed Whisper Pod was removed after its replacement was healthy.

The completed fault test ran on p4. Preflight now requires at least 25% and
10 GiB free on both nodefs and imagefs, verifies that the image is cached, and
sets `imagePullPolicy: Never`. Thus a cache race cannot start a download.
The p4 preflight passed and the low-headroom p5 preflight was rejected without
creating resources. Final verification records 15 original Pod identities
retained, the recreated Whisper Pod, and all original persistent disk identities.

[Sanitized verification](coder-hyperevm-disk-full-2026-10-19.json) records the
production checks and native test. Temporary Coder API credentials were revoked.
This change preserves retries when storage is exhausted; it does not allow
writes beyond a full disk or remove the configured storage limit.
