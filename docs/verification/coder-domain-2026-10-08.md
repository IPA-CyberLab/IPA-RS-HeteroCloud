# Coder public hostname: 2026-10-08

The existing Coder Flash service now uses [https://coder.mizuame.app/workspaces](https://coder.mizuame.app/workspaces).

The existing Cloudflare DNS record was changed from the old Tunnel target to a DNS-only CNAME pointing to the service's canonical Flash hostname. The HeteroCloud domain binding is stored as desired state; the existing GitOps certificate and gateway controllers issue and renew TLS and maintain the per-host route. No path rewrite is needed: `/workspaces` is served by Coder itself.

After trusted HTTPS was verified at both public origins, the service's `CODER_ACCESS_URL` was changed to `https://coder.mizuame.app` through the HeteroCloud API. This rolled the Coder control-plane container; its existing PVC, task role, VPC attachment, image and resource limits were retained. Other Flash Pods were unchanged.

Desktop and mobile Chromium both rendered the GitHub sign-in control on the new hostname without runtime errors. HTTP `/workspaces`, `/login`, `/api/v2/buildinfo` and the public auth-methods API responded successfully. An authenticated user login was not performed. The existing GitHub authentication configuration was retained.

Provisioning used an expiring service-account credential restricted to this service and retaining only its existing task role and VPC security group. Temporary credentials were removed after completion. No passwords, API keys, cookie values or database URLs are stored in this record.

[Sanitized configuration and verification](coder-domain-2026-10-08.json).
