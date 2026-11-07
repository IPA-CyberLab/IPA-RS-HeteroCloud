# Cloud capability work: status on 2026-11-07

This is a record of what was deployed and verified, not a claim of full AWS
compatibility. Production observations below are point-in-time measurements.

## Available and verified

- Flash service logs are accessible through the HeteroCloud API, console and
  CLI, with organization, project and service authorization. A real tenant,
  cross-tenant denial, CLI and browser were tested; see
  [Flash logs verification](flash-logs-2026-11-07.md).
- Overlay DNS discovery runs on two separate eligible workers. A
  [disposable-cluster test](https://github.com/IPA-CyberLab/IPA-RS-HeteroNetwork/actions/runs/42360502284)
  paused one worker, verified ongoing discovery, and checked that controllers
  again used separate workers after recovery. The latest
  [real VPN and authenticated Chromium acceptance run](https://github.com/IPA-CyberLab/IPA-RS-HeteroNetwork/actions/runs/42361243111)
  passed, including its existing three-second deadlines. Earlier runs were
  intermittent failures; a pass is not evidence they can no longer recur.
- Flow TURN has two ingress nodes and two instances in each TURN pool. External
  relay and Chromium tests exercised UDP and TCP on both configured TURN ports;
  see the [TURN verification](https://github.com/IPA-CyberLab/IPA-RS-HeteroCloud-Flow/blob/master/docs/verification/turn-failover-2026-11-07.md).
  This does not establish that new credential issuance survives loss of the
  single shared HTTPS ingress.
- Volume health and the number of read/write replicas are monitored. A
  critical alert was observed firing for an actual single-copy volume; failed
  or ambiguous inventory is reported separately. The two collectors now run on
  different workers. Alert delivery to an external receiver is not configured.
- Encrypted control-plane backup and isolated restore tooling was exercised
  using disposable PostgreSQL, etcd and Syouyu fixtures. Flash backup/restore
  supports an independent S3 destination and has its own verification path.

## Not complete

- Multiple attached volumes are degraded, including Coder PostgreSQL and
  workspace homes with only one read/write replica. Four detached volumes were
  observed faulted. Replica counts change as rebuilds retry. The Longhorn
  rebuild race investigation and current impact are in the
  [incident report](https://github.com/IPA-CyberLab/IPA-RS-HeteroNetwork/blob/master/docs/incidents/2026-11-07-identity-postgres-latency.md).
  Monitoring does not repair these volumes or prove their recoverability.
- Independent R2 buckets exist, but the available Cloudflare authorization was
  denied when creating scoped object credentials. Scheduled offsite backups
  have therefore not been activated or restored end to end. Syouyu in the same
  cluster is not an independent disaster recovery site. A separately retained
  recovery key and a tested restore are also required.
- Only two workload/storage workers are currently eligible. The three
  dedicated control-plane/custodian nodes must remain free of ordinary
  workloads, and `uc-k8sp4` is administratively stopped. A third independent
  eligible worker is needed for three-way storage redundancy and the current
  three-replica Redis configuration. Both Argo applications reported
  `Progressing`, despite the DNS, monitoring and TURN replicas noted above
  being ready.
- AWS SigV4/API/SDK and STS protocol compatibility and a managed PostgreSQL
  offering are not implemented. Existing Registry and Syouyu data-plane
  credentials do not immediately re-evaluate IAM/SCP changes. See
  [IAM compatibility](../iam/aws-compatibility.md). Complete account isolation,
  revocation semantics and cost/usage coverage still require validation.
- A whole-platform restoration, recovery of the four faulted volumes, and
  external notification delivery have not been verified.
