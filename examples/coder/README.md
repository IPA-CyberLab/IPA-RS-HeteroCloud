# Coder workspace template for Flash

This is the deployed `standard-develop` template. It retains one Flash service ID and persistent `/root` storage per workspace. Stopping updates `spec.stopped`; starting uses the same service again.

The provisioner API URL points to the Coder host's local credential bridge. The bridge uses the attached task IAM role; no API key belongs in Terraform variables or state. The template retries transient connection and server errors three times with bounded backoff. IAM rejection and other client errors are not treated as retryable server failures.

Set `coder_private_url` to the Coder endpoint reachable from the workspace's VPC. Agent bootstrap downloads use this private URL, and the agent connects to it through `CODER_AGENT_URL`. The public URL is read from `data.coder_workspace.me.access_url`, so changing the Coder deployment's public domain does not leave a stale string replacement. A precondition rejects a bootstrap that fails to include the private URL.

The `coder_url` variable is retained for compatibility with older template variable sets. Bootstrap rewriting uses the live deployment's access URL. Set the other organization, project, VPC and image variables for the target installation.

The checked-in lock file pins the provider versions used in the deployed template. Coder validates the provider schema and performs template import before promotion. Workspace build verification must also confirm the agent connects; a successful Terraform apply alone is insufficient.

The default allocation is 4 vCPU, 8 GiB RAM, 37 GiB total disk and 10 GiB for
the container filesystem. Increasing the container filesystem from 3 to 10 GiB
also increased the total disk budget by 7 GiB to preserve the existing home.
`workspace_storage_overrides` retains larger disk budgets by workspace ID.
An optional `persistent_storage_gib` pins the home size independently of the
container filesystem. The combined budget must also cover the image; keep the
configured home at least as large as the existing PVC. Automatic homes use the
remaining space and are allocated at CSI's MiB granularity.
`workspace_memory_overrides` sets memory in MiB by workspace ID, for example
`{"workspace-uuid":32768}` for 32 GiB. Store these values as template variables
and rebuild the affected workspace. A subsequent stop/start preserves the
allocation, and workspaces without an override retain the default. Both
Kubernetes memory requests and limits use the configured value; an eligible
node must have sufficient unallocated memory. A memory change recreates the
container while retaining the same persistent `/root` volume.

The IDE runs under a detached, single-instance supervisor. Its script, lock and
diagnostic logs live in `/tmp/coder-code-server-<uid>` on the container filesystem,
so a full persistent home cannot disable retries. Diagnostic write failures are
ignored, and logs rotate between launches above 2 MiB. The supervisor restarts
code-server after an unexpected exit, including after space is freed. User files,
IDE settings and extensions stay in `/root`. The agent startup shell returning
does not terminate the IDE. Installing this supervisor in an existing workspace
does not require a Pod restart; only the IDE process needs to be replaced.

Keep every workspace disk expansion in `workspace_storage_overrides` when
publishing a new template version. Increasing only the total disk budget, while
retaining `rootfs_storage_gib`, expands the persistent home without changing the
container resources. It does not increase the workspace memory limit.
The deployed HyperEVM allocation is recorded in
[`deploy/coder/workspace-overrides.json`](../../deploy/coder/workspace-overrides.json).
Merge those entries into the active template variables when publishing; retain
entries for other workspaces. This deployment-specific file is not automatically
applied to unrelated Coder installations.

Run the native full-disk regression test with a Kubernetes administrator context:

```sh
python3 scripts/e2e/coder_disk_full.py --kubeconfig "$KUBECONFIG" \
  --reference-pod "$WORKSPACE_POD" --node "$TEST_NODE" --report /tmp/coder-disk-full.json
```

Choose a workload node with at least 25% and 10 GiB free on both nodefs and
imagefs, and the reference image already cached. `--preflight-only` verifies
these requirements without creating resources. The test uses
`imagePullPolicy: Never`, so it cannot download an uncached image. It creates a
separate 64 MiB Longhorn home and a gVisor Pod using
that exact workspace image. It fills only this fixture, terminates only its IDE,
then verifies automatic recovery and retained files after space is freed.
The fixture's own share-manager allocates the blocks because gVisor cannot
preallocate this NFS filesystem. UID and claim-reference guards protect existing
volumes. Fixture resources are removed on completion; the test never fills or
signals the reference workspace. Optional `--legacy-supervisor` compares a
previous supervisor with the fixed implementation.

Scheduling defaults are declared in `scheduling.json`: automatic time-based
stopping is disabled. Apply them separately from the workspace Terraform:

```sh
python3 scripts/reconcile-coder-scheduling.py --endpoint "$CODER_ENDPOINT" \
  --template-id "$CODER_TEMPLATE_ID" --token-file "$CODER_TOKEN_FILE" --apply
```

The token file must have owner-only permissions. The reconciler disables the
existing user's workspace TTLs without rebuilding running workspaces. Add
`--resume-autostopped` to resume workspaces whose latest stop reason is
`autostop`; manually stopped workspaces remain stopped. New workspaces inherit
the disabled default, and users can still opt into a stop timer explicitly.

Production policies live in `deploy/coder/scheduling.json`; pass that file with
`--policy deploy/coder/scheduling.json`. Its optional `workspace_automatic_updates`
map selects `always` or `never` for individual workspace UUIDs. `always` uses the
active template on the next start; applying this setting does not rebuild a
running workspace. Unlisted workspaces keep their update preference, and the
reconciler rejects entries outside the authenticated owner and template before
writing anything. HyperEVM follows the corrected disk template on its next start.
