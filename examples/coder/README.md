# Coder workspace template for Flash

This is the deployed `standard-develop` template. It retains one Flash service ID and persistent `/root` storage per workspace. Stopping updates `spec.stopped`; starting uses the same service again.

The provisioner API URL points to the Coder host's local credential bridge. The bridge uses the attached task IAM role; no API key belongs in Terraform variables or state. The template retries transient connection and server errors three times with bounded backoff. IAM rejection and other client errors are not treated as retryable server failures.

Set `coder_private_url` to the Coder endpoint reachable from the workspace's VPC. Agent bootstrap downloads use this private URL, and the agent connects to it through `CODER_AGENT_URL`. The public URL is read from `data.coder_workspace.me.access_url`, so changing the Coder deployment's public domain does not leave a stale string replacement. A precondition rejects a bootstrap that fails to include the private URL.

The `coder_url` variable is retained for compatibility with older template variable sets. Bootstrap rewriting uses the live deployment's access URL. Set the other organization, project, VPC and image variables for the target installation.

The checked-in lock file pins the provider versions used in the deployed template. Coder validates the provider schema and performs template import before promotion. Workspace build verification must also confirm the agent connects; a successful Terraform apply alone is insufficient.

The IDE runs under a detached, single-instance supervisor in the persistent home. The supervisor restarts code-server after an unexpected exit. The agent startup shell returning does not terminate the IDE. Installing or starting this supervisor in an existing workspace does not restart its Pod or interactive processes.
