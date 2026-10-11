# Flash configuration update verification — 2026-10-11

HeteroCloud 0.1.105 (source `173571a0ae303ad6d1e4ad2b311f9e3bb9d4f42d`) and Flash 0.1.48 (source `1de906cb97ac88c4d96613b48ab033f3f79fcc78`) are deployed through Terraform and Argo CD. Both applications are Synced/Healthy. All nine previously Ready workload Pods and container IDs, including the protected recovery workload, stayed unchanged.

## Validation

- HeteroCloud release workflow [38116473291](https://github.com/IPA-CyberLab/IPA-RS-HeteroCloud/actions/runs/38116473291) passed Rust tests, PostgreSQL integrations, clippy, 142 console tests, desktop/mobile Playwright, and all six native CLI builds (Linux/macOS/Windows, x64/ARM64).
- Flash release workflow [38116474191](https://github.com/IPA-CyberLab/IPA-RS-HeteroCloud-Flash/actions/runs/38116474191) passed all 105 Rust tests, generated CRD parity, clippy, chart/launcher checks, and both architecture builds.
- Infrastructure workflow [38117179186](https://github.com/IPA-CyberLab/IPA-RS-HeteroNetwork/actions/runs/38117179186), attempt 2, passed its real GitHub runner WireGuard join and authenticated Chromium checks. Attempt 1 failed during the deployment window; its log did not expose enough information to establish a cause.
- The [live receipt](flash-config-updates-2026-10-11.json) records real API, published CLI and authenticated console operations, without API mocks. Desktop/mobile page readiness was 1,293/1,173 ms; the 3,000 ms limit was retained.
- A disposable VPC workload retained its marker and PVC identity across image, CPU, memory, rootfs, environment, command/args, metadata and port updates. Its persistent disk and mounted filesystem expanded from 1 to 2 GiB without replacing an otherwise unchanged container.
- Public LoadBalancer and private ClusterIP transitions worked in both directions. Console edits retained files, unsafe shrink/cross-tenant requests left generation/data unchanged, and project migration/VPC detachment retained service and disk identity. Removing explicit rootfs sizing restored automatic allocation.

## Limits and test recovery

Persistent disks can grow; shrinking is rejected to preserve data. Image, filesystem, CPU/memory or process updates can recreate containers; persistent `/root` remains. Total disk budget must accommodate the image, filesystem and retained persistent volume. Storage status reports allocation/capacity rather than consumed bytes or disk-time billing.

The first disposable workload failed to attach its new Longhorn RWX share within the startup deadline. Manager logs included etcd request timeouts and the share manager restarted; no precise underlying cause was established. The fixture was removed. A fresh fixture attached healthy replicas on all three workers with the original timeout. The browser test then found an incorrect test locator (detail-page label instead of form label); correcting the locator resumed the same recorded fixture after checking its original PVC UID and marker. No timing assertion was relaxed. All eight checks passed.

Both workload/VPC fixtures, three owned test usage records, both test organizations, the browser identity and disposable database were removed. Expiring credentials were revoked by identity cleanup and private credential files removed. Existing users' services and persistent disks were not resized by this verification.
