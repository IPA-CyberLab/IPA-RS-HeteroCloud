# HyperEVM IDE 404 after an ephemeral-storage eviction, 2026-10-21

HyperEVM's code-server URL returned an application-unavailable page because its
workspace container had been evicted and its replacement was blocked. Coder's
login page being reachable did not demonstrate that the workspace was healthy.

At 03:09:32 UTC, kubelet evicted the workspace with the recorded reason
`Pod ephemeral local storage usage exceeds the total limit of containers 10737418240`.
This is an eviction at the configured 10 GiB temporary-storage limit. The
subsequent exit code 137 and `ContainerStatusUnknown` do not establish a memory
OOM or host restart.

Our capacity-admission change then prevented the legacy ReplicaSet from
recreating the last working revision. Its Pod template had no image-size
annotation. The admission service's fallback relied on the current Flash
status, but a pending storage-allocation error had discarded that status's
verified image metadata. Admission repeatedly rejected the replacement with
`Flash capacity: image_capacity_unknown`. The earlier tests covered admission
checks but did not cover replacement of an older ReplicaSet while the desired
revision was invalid. This platform regression extended the outage.

A second defect prevented the requested persistent-disk expansion from staying
ready. Automatic home allocation used the exact remainder after subtracting
the compressed image and container filesystem from the total budget. The CSI
driver rounds requests up to a MiB. On the next reconciliation, the rounded
request could exceed that same budget. Retrying the identical configuration
could therefore produce a storage error after initial provisioning.

## Recovery and retained data

Recovery changed HyperEVM through its Flash API. A replacement container started
on p4 using the existing home volume. Authenticated Chromium rendered the actual
VS Code workbench with HTTP 200 at 05:43:42 UTC and no server errors. No project
files or Codex state were removed. The persistent PVC UID remains
`856a597c-1676-423f-8d16-26752b85bf80`.

The final allocation is 69 GiB total disk, a 20 GiB container filesystem and a
48 GiB persistent home. Memory remains **8 GiB** and CPU remains 4 vCPU. An
explicit home allocation avoids the CSI rounding trap. The home was expanded
online from 36 to 48 GiB after recovery, retaining the recovered Pod and
container identity. At 06:00:59 UTC, the mounted filesystem had approximately
14.2 GiB available; both `/root/projects/HyperEVM` and `/root/.codex` existed.

These settings are declared in
[`workspace-overrides.json`](../../deploy/coder/workspace-overrides.json).
The active Coder template version is
`6bb601fd-d18d-4bec-a1de-2b7e887a1f7f`. HyperEVM's previous build still references
an immutable older template. Its next-start policy now follows the active
version, declared explicitly in
[`deploy/coder/scheduling.json`](../../deploy/coder/scheduling.json). Applying
that policy did not create a build or restart the running workspace; other
workspaces' update preferences were not changed. Coder documents this behavior
in its [workspace management guide](https://coder.com/docs/user-guides/workspace-management).

## Platform fixes

Flash 0.1.55 retains verified image identity and size when reporting an invalid
storage allocation. Admission accepts status metadata only when it belongs to
the Pod's image. If a legacy immutable image has no trustworthy size metadata,
admission can place it only on an eligible worker that already has that exact
digest cached, and forces `imagePullPolicy: Never`. The normal memory, CPU,
filesystem-headroom and reservation checks still apply. Unknown uncached
images remain rejected without a download.

Automatic new-home allocations now round down to a MiB within the budget;
existing retained homes are never shrunk. The workload helper image remains
pinned at 0.1.41 so that this control-plane rollout does not replace otherwise
healthy user containers.

## Regression verification

The Flash release passed 94 Rust library tests, 27 Python operation tests and
the release CI, including strict Clippy. Coder configuration passed ten
Terraform tests and thirteen scheduling/supervisor tests. The latter include
scoped next-start updates, unchanged running builds, other-owner rejection,
dry runs and repeated application without additional builds.

The native recovery regression is maintained in the Network repository as
`scripts/flash_legacy_recovery_e2e.py`. It uses only a new disposable Flash
service and volume with a pinned image already cached on a worker. It checks
MiB-aligned automatic allocation across CSI reconciliations, verified metadata
after a storage error, cached legacy replacement while the desired image is
invalid, retained marker contents and PVC identity, and rejection of an
uncached image without a pull. The pre-fix run failed during automatic-allocation
verification; all its fixture resources were removed. The final post-fix run
passed all four checks and removed its service, Pods, network policy, claim,
physical volume and storage replicas. Initial harness attempts were corrected
to reach the storage allocator and tolerate conflicting status writes; their
fixtures were also removed. Fault injection is never performed on a user
workspace.

Authenticated Chromium again rendered the workbench at 06:23:10 UTC. The Coder
agent was connected and both code-server and filebrowser were healthy. Seven
other running workload Pods retained their identities and restart counts; all
ten original workload PVCs retained their identities and physical volumes.
No user Pods run on the three dedicated control-plane nodes. The Cloud and
Flash Argo CD Applications were both Synced and Healthy. The final Network
master also passed Actions with an actual VPN connection and Chromium session.

Final production and regression results are recorded in the accompanying
[sanitized verification](coder-hyperevm-eviction-2026-10-21.json).
