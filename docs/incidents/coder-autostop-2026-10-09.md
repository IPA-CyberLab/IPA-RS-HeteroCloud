# Coder automatic workspace stops, 2026-10-09

The reported coconel Terraform log is a successful stop operation requested by
Coder. Its build reason is `autostop`, not a failed container launch. The
`standard-develop` template and coconel, Reverse, and HyperEVM each had an
eight-hour TTL. Recent Reverse and HyperEVM stops also have reason `autostop`.
The automatic scheduler records the workspace owner as initiator, so the
initiator username alone does not indicate a manual stop.

The template's default TTL is now zero, and the existing owner's three
workspaces have disabled TTLs. Their resumed builds have no stop deadline.
User opt-in to a stop timer is retained. The declarative policy is
[scheduling.json](../../examples/coder/scheduling.json), applied through the
[API reconciler](../../scripts/reconcile-coder-scheduling.py). The reconciler
limits changes to the authenticated owner's workspaces on the named template,
preserves running workspaces, and only resumes stops marked `autostop`.

All three workspaces resumed with their existing Flash service IDs and
persistent homes. Their agents are connected and ready, and both editor and
file-browser application health checks pass. The template version used for
resuming is the active version, including the existing IDE supervisor and
private agent bootstrap fixes.

The scheduling guard tests cover owner/template isolation, preservation of
manual stops, dry-run behavior, and repeat application without duplicate
builds. Production verification reads back the disabled TTLs and empty build
deadlines instead of waiting eight hours. Temporary maintenance credentials
are revoked after verification. Private receipts live in
`/workspace/.heteronetwork-iac/coder-autostop-20261009/`.
