# Independent CLI audit: released 0.1.61

Audit date: 2026-09-07. No prior audit results were used. No AGENTS.md exists in
this repository or its ancestor directories. Initial worktree was clean.
Only audit files are added by this audit; no implementation changes, commits,
or pushes. The parent subsequently changed the implementation to address the
findings; its local binary is validated separately below.

## Reproduce

```sh
python3 scripts/tests/cli_audit.py --report /tmp/cli-audit.json
# Alternatively, test a local build or future release:
python3 scripts/tests/cli_audit.py --binary /path/to/heterocloud --report /tmp/cli-audit.json
# Fast focused pending/retry budget checks (also included in the full suite):
python3 scripts/tests/cli_audit.py --binary /path/to/heterocloud --wait-budget-only --report /tmp/cli-budget.json
```

Python 3.9+ standard library, Linux x64. The harness downloads the published
artifact, checks its pinned SHA-256, extracts only `heterocloud`, and verifies
the binary digest. `--binary` accepts local/future builds without pin enforcement
and records their actual SHA-256 and `--version`; these results must not be
presented as released 0.1.61 evidence unless the artifact digest matches.

- Release: https://github.com/IPA-CyberLab/IPA-RS-HeteroCloud/releases/tag/v0.1.61
- Archive SHA-256: `41bc6c308581093e9ca1021c930332bec02391e54657ffaeb21bf1e4a0e0f491`
- Binary SHA-256: `cf80f6b2c5a0669843eaba26ca24520bb2fbee7c135d8af45f362109119cc653`
- Each subprocess has a 12-second deadline unless explicitly extended for the
  30-second HTTP timeout (35), negative DNS verify (20), or DNS convergence (45).
- A timed-out process group is killed. Temporary files and the mock server are
  cleaned up. The JSON report retains argv, fixture stdin, environment overrides,
  exit code, stdout/stderr, elapsed time, HTTP response fixtures and requests,
  stub calls, and assertions. The version discovery preflight is an additional
  bounded subprocess, outside the counted test cases.
- Nonzero audit exit means at least one assertion failed. Known release defects
  remain failures, not expected-success baselines.

## Confirmed Findings

### F1: Poll results bypass the requested wait deadline

`--wait-timeout-seconds 1 flow create`, with an immediate pending POST response
and a GET returning ready after two seconds, exits 0 after 2.015 seconds.
`--wait-timeout-seconds 1 flow delete ID --yes`, with an immediate deleting DELETE
response and a GET returning 404 after two seconds, exits 0 after 2.016 seconds.
Expected: a timeout error once the one-second polling budget expires.

Reproduce with harness cases `wait:deadline-late-ready` and
`wait:deadline-late-deleted`. Both use loopback HTTP and dummy identifiers.
The final suite also tests two-second delayed error-state responses on create
and delete. All deadline cases require a timeout diagnostic and less than 1.75
seconds elapsed (one-second budget plus 0.75 seconds scheduling allowance).
Four additional `wait:budget:*` cases test create/delete with persistent pending
state and repeated HTTP 503 responses delayed by 0.4 seconds each. The latter
include read retry/backoff inside the wait budget. On released 0.1.61, pending
cases take 2.018-2.020 seconds and unavailable/retry cases take 1.970-1.972 seconds.
The shared wait loops in `crates/heterocloud-cli/src/services.rs` return terminal
results before checking the deadline and await GET without the remaining budget.
The same code handles all three providers; direct delayed-response evidence is
for Flow create/delete, not a claim of six separately tested delayed cases.

### F2: DNS records omit the documented S3 hostname

```sh
heterocloud dns records --domain audit.invalid --public-ip 192.0.2.10 --allow-non-public --format json
```

Actual: four records, for `cloud-a.audit.invalid`, `audit.invalid`,
`flow.audit.invalid`, and `registry.audit.invalid`. No `s3.audit.invalid`.
README's Public DNS onboarding section promises `s3.<domain>` alongside the
other cluster-scoped names. Case `dns:records-documented-s3` checks that contract.
This is a CLI/documentation mismatch, not evidence that existing deployed S3 DNS
is missing. `build_records` in `crates/heterocloud-cli/src/lib.rs` omits S3.

## Per-Leaf Coverage

All 18 executable leaves are independently discovered from released `--help`
and invoked. The built-in help command, version flags, group help, and globals
are additional invocations, not additional executable service/DNS leaves.

| Leaf | Boundary | Executed contracts |
| --- | --- | --- |
| flow list | Loopback mock API | JSON/table, empty list, project query, HTTP errors, read retries/transport, malformed response |
| flow get | Loopback mock API | JSON/table, item path, HTTP errors, retries, malformed response, provider mismatch |
| flow create | Loopback mock API | File/stdin, POST body/path, no-wait, pending-to-ready, error, timeout, delayed-ready/error deadline, persistent pending and retry budget, invalid input, no write retry |
| flow update | Loopback mock API | File/stdin, PATCH body/path, no-wait, pending-to-ready, error, timeout, no write retry |
| flow delete | Loopback mock API | Confirmation blocks requests, DELETE, no-wait, 404 convergence, JSON/table, error, timeout, delayed-404/error deadline, persistent pending and retry budget |
| flash list | Loopback mock API | JSON/table, empty list, project query, HTTP errors, read retries/transport, malformed response |
| flash get | Loopback mock API | JSON/table, item path, HTTP errors, retries, malformed response |
| flash create | Loopback mock API | File/stdin, POST body/path, no-wait, pending-to-ready, error, timeout, invalid input, no write retry |
| flash update | Loopback mock API | File/stdin, PUT body/path, no-wait, pending-to-ready, error, timeout, no write retry |
| flash delete | Loopback mock API | Confirmation, DELETE, no-wait, 404 convergence, JSON/table, error, timeout |
| syouyu list | Loopback mock API | JSON/table, empty list, project query, HTTP errors, read retries/transport, malformed response |
| syouyu get | Loopback mock API | JSON/table, item path, HTTP errors, retries, malformed response |
| syouyu create | Loopback mock API | File/stdin, POST body/path, no-wait, pending-to-ready, error, timeout, invalid input, no write retry |
| syouyu update | Loopback mock API | File/stdin, PUT body/path, no-wait, pending-to-ready, error, timeout, no write retry |
| syouyu delete | Loopback mock API | Confirmation, DELETE, no-wait, 404 convergence, JSON/table, error, timeout |
| dns records | Local rendering; fake kubectl discovery | Zone/table/JSON, TTL, address dedup/sort, invalid domain/address/options, discovery args/errors, documented S3 check |
| dns verify | System resolver; negative `.invalid` and positive container hosts fixture | Expected negative resolution/nonzero exit; successful verification on both release and patched binary |
| dns reconcile | Local dry-run; fake kubectl/helm; container hosts fixture | Five provider dry-runs, option/credential validation, sanitized output, fake apply/restart/rollout, subprocess errors, negative DNS timeout and positive fixture convergence |

Global coverage: endpoint, HTTP opt-in, key argument/environment/file (0600,
0400, unsafe mode, missing file, symlink, conflict), organization argument/env,
wait bounds/env, JSON/table output, env override precedence, `--help`/`-h`,
`--version`/`-V`, `help flow create`, missing/unknown command. HTTP 400/401/403/
404/409/422/500 error cases are single-attempt; 429/502/503/504 are retried three
times on reads. Writes are not retried. Dummy bearer and user-agent headers are
asserted on every observed HTTP request.

## Boundaries And Gaps

Live production API commands: **0**. Production API keys and Kubernetes config
are never loaded. CLI processes get a minimal environment, loopback API origin,
temporary HOME/KUBECONFIG, and PATH containing only fake kubectl/helm scripts.
Those scripts log arguments/input and return fixtures; they do not call real
tools, clusters, providers, or DNS mutation APIs. The only external lookups during
CLI execution are reserved `.invalid` DNS names. No DNS records are changed.

Mock success demonstrates CLI request construction and response handling, not
live IAM authorization, server validation, actual provider provisioning, cluster
RBAC, Helm installation, ExternalDNS reconciliation, or successful DNS propagation.
Those integration checks remain with the parent. HTTPS certificate behavior,
Windows/macOS binaries, exhaustive flag combinations, interactive TTY behavior,
and a full 90-second read-request timeout/retry exhaustion are not covered.
The write-side 30-second request timeout is exercised. DNS verify/reconcile
positive convergence is covered only by the disposable container hosts fixture
below; it is not public DNS or actual ExternalDNS validation.

## Results

Released 0.1.61, corrected main batch: **278 cases, 273 passed, 5 failed**.
All **18/18** executable leaves were invoked. Production API commands: **0**.
The five failures are four manifestations of F1 (late ready, late deleted,
late error on create, late error on delete) and one F2 assertion. No additional
substantive failures were found. Full evidence:
`/tmp/heterocloud-cli-audit-0161-final.json`.
The subsequently requested four budget cases were executed as a separate batch,
with **0 passed, 4 failed**, recorded in `/tmp/heterocloud-cli-audit-0161-budget.json`.
The generic harness now includes these cases in every normal full invocation.
Thus the final coverage is **282 distinct cases, 273 passed, 9 failed**, spanning
two actual product issues, not nine independent defects.

An earlier 275-case run had four harness false positives, now corrected:
cross-level key conflict required an unnecessarily specific exit 2; two DNS
credential fixtures incorrectly included a newline; an invalid chart-version
fixture used the accepted symbolic token `bad`. The corrected harness requires
nonzero rejection for the cross-level conflict, uses newline-free DNS tokens,
and tests invalid whitespace in the chart version. These are not product bugs.

### Parent-Patched Local Build

Independent full-suite rerun completed against `target/debug/heterocloud`:
**278 passed, 0 failed**, plus **4 passed, 0 failed** in the added budget batch.
Its reported version remains 0.1.61, but it is **not the released artifact**.
Binary SHA-256: `e5433d11053c32b354175cd0d44337e45bbba8e94933959e1133a5ff46ebfdef`.
Evidence: `/tmp/heterocloud-cli-audit-patched.json` and
`/tmp/heterocloud-cli-audit-patched-budget.json`.

| Batch | Cases | Released Pass/Fail | Patched Pass/Fail |
| --- | ---: | ---: | ---: |
| Main full-leaf suite | 278 | 273 / 5 | 278 / 0 |
| Added wait-budget cases | 4 | 0 / 4 | 4 / 0 |
| Positive container DNS cases | 2 | 2 / 0 | 2 / 0 |
| Combined distinct cases | 284 | 275 / 9 | 284 / 0 |

Patched delayed ready/deleted/error cases return a timeout after 1.070-1.088
seconds, and persistent pending/retry cases after 1.076-1.155 seconds. The S3
record assertion passes. No further substantive failure was observed.
The main batches took approximately 130-131 seconds each (summed command time),
including intentional 30-second HTTP and DNS timeout tests. This is mock/local
regression validation only; production integration remains unverified here.

### Positive DNS Hosts Fixture

```sh
python3 scripts/tests/cli_dns_hosts_audit.py --released /tmp/heterocloud-independent-0161/heterocloud --patched target/debug/heterocloud
```

Four additional CLI executions passed: `dns verify` and `dns reconcile` on each
binary. The released binary reports `Verified 4 A records`; the patched binary
reports `Verified 5 A records`, including `s3.audit.invalid`. Both reconcile
commands report `ExternalDNS converged all records`, without `--no-wait-dns`.
This does not erase the release's missing-S3 contract failure: its verification
only checks the four records it generates.

Boundary: disposable Docker container, `--network none`, no bind mounts, all
capabilities dropped, no-new-privileges. `--add-host` maps `cloud-a.audit.invalid`,
`audit.invalid`, `flow.audit.invalid`, `registry.audit.invalid`, and
`s3.audit.invalid` to `192.0.2.10` inside the container only. Binaries and recording
shell stubs are transferred with `docker cp`, compatible with the remote daemon.
No host/system DNS settings or production resources are changed. The writable
container filesystem is disposable; each container is forcibly removed in cleanup.

Fake kubectl/helm recorded exactly seven calls per reconcile (14 total), never
contacting Kubernetes or installing ExternalDNS. Resolver success comes from the
container's `/etc/hosts`, not authoritative DNS or propagation. Both binary
SHA-256 values match the earlier before/after evidence above.

Image: `ubuntu:24.04`, observed image ID
`sha256:ef91e4b15da8323a1523adb2b371998dcd3063dae8553cc2744c178ccc065bc4`.
Every Docker call has a 30-second bound, CLI cases 40 seconds; the container's
keepalive exits after 300 seconds. Evidence (argv, stdout/stderr, hosts, hashes,
stub calls, removal): `/tmp/heterocloud-dns-hosts-audit.json`.
Two preliminary fixture setup failures (read-only copy and directory permissions)
were corrected before CLI cases ran; neither was a product failure.

Audit-owned changed files: **3** (`scripts/tests/cli_audit.py`,
`scripts/tests/cli_dns_hosts_audit.py`, and this report).
Parent-owned source, documentation, ignore, and workflow changes were not edited
or reverted by this audit. No commits or pushes were made.
