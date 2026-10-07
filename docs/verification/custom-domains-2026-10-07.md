# Flash custom domains: deployed verification, 2026-10-07

HeteroCloud API/CLI 0.1.104, console 0.1.66 and Flash 0.1.47 were deployed through Terraform and Argo CD. All five related applications were Synced and Healthy.

Owned disposable services and domain records verified CNAME delegation, trusted HTTPS at both public origins, retention of the original service URL, and the alternative A-record-plus-TXT proof. The published CLI successfully registered, listed and deleted a domain. Desktop and mobile Chromium opened the deployed domain settings and completed actual Keycloak login with callbacks returning to the custom hostname. The console used an explicitly synthetic owner session; the service OIDC login used the real identity provider and a disposable test user.

Manual certificate renewal changed the served leaf certificate at both origins. All 62 HTTP/TLS probes during rotation succeeded. The test container kept its Pod and restart count. All seven existing workloads retained their Pod UIDs, restart counts, limits and PVC references.

Initial validation caught a console reference error before distribution. Live testing also caught recursive DNS cache disagreement that withdrew verified aliases; the controller now preserves a verified binding on inconsistent answers and requires agreement before withdrawal. Native CLI checks passed on Linux, macOS and Windows, each on x64 and ARM64.

The real Actions VM joined HeteroNetwork and passed authenticated Chromium checks. The canonical console opened in 827 ms after 19,925 ms of initial VPN route convergence; the browser gate remained three seconds. Earlier route warmup failures were retained in CI history, and the initial route convergence allowance was raised to 60 seconds.

Owned aliases, routes, certificates, DNS records, test service, credentials and identity-provider resources were removed. Retired gateway key files have a one-hour reload grace period before collection.

[Sanitized results and CI links](custom-domains-2026-10-07.json). No tokens, session cookies, passwords, private keys or database URLs are included.
