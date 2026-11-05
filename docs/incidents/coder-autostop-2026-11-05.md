# Coder workspace automatic stops, 2026-11-05

`standard-develop` already had a zero default TTL. The separately copied
`big-develop-copy` template still had a 24-hour default, however, and
both existing workspaces using it inherited that TTL. `ramen` stopped with
build reason `autostop`; `CKB-VM` was running with a future stop deadline.
The earlier repair of `standard-develop` did not include the copied template.

The default TTL has been set to zero on both templates. The workspace TTLs
were cleared through the Coder API, which also removed the existing stop
deadline from the running `CKB-VM` build. `ramen` was started because its
latest stop was automatic. Other running workspaces were not restarted, and
manual stops are not resumed by the repair command.

Afterward, the latest builds of `CKB-VM`, `dYdx`, and `ramen` were running,
their deadlines and maximum deadlines were unset, and all three agents were
connected and ready. The temporary scoped maintenance credential was revoked.

The managed template list and desired default are in HeteroNetwork's
`deploy/coder/workspace-scheduling.json`, applied with the targeted Terraform
resource `terraform_data.coder_workspace_scheduling`. The reconciler leaves
individual workspace choices untouched on subsequent applies, so users may
still explicitly opt in to an automatic stop. Coder's template creation CLI
defaults to 24 hours: a new or copied template must also be added to
the managed list and the Terraform resource reapplied. The Terraform resource
is not a continuously running drift controller.
