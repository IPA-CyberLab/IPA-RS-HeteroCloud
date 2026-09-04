# HeteroCloud

HeteroCloud is the public control plane for services running on a
HeteroNetwork Kubernetes cluster. It owns customer identity, organizations,
projects, IAM, quotas, service lifecycle metadata, and audit records. Service
data planes such as Flow and Flash remain independent repositories and are connected
through a versioned provider contract.

HeteroCloud does not own VPN membership, node reachability, public-IP
selection, packet forwarding, LiveKit, TURN, or matchmaking.

## Current vertical slice

- Invite/bootstrap-only local accounts with Argon2id password hashes.
- Optional Keycloak OIDC login using Authorization Code with PKCE. Keycloak may
  provide self-registration without exposing a HeteroCloud anonymous register
  endpoint.
- Server-side sessions using an opaque 256-bit cookie and database-backed
  revocation.
- Origin and CSRF validation on every cookie-authenticated mutation.
- Organizations, projects, user and service-account principals, IAM policies,
  policy bindings, Flow and Flash service instances, an outbox, and audit events.
- Default-deny IAM with explicit-deny precedence and an organization boundary.
- A Lean 4 authorization kernel proving the final decision invariants.
- IAM-authorized, five-minute-or-shorter Flow data-plane access contexts.
- A React and TypeScript operations console served by the Rust API.

There is deliberately no anonymous self-registration endpoint. Public
registration will remain invite-gated until phone/payment verification and
global abuse controls exist.

## Repository boundaries

```text
HeteroCloud
  identity, sessions, IAM, projects, audit, service metadata
      |
      | signed provider/v1 commands
      v
HeteroCloud Flow
  rooms, matching, P2P signaling, LiveKit, STUN/TURN, usage
HeteroCloud Flash
  gVisor-isolated containers, TCP/UDP Services, workload lifecycle
      |
      | Kubernetes Services
      v
HeteroNetwork
  VPN, reachability, direct/forwarded LoadBalancer data paths
```

See [Architecture](docs/ARCHITECTURE.md), [Security](docs/SECURITY.md), and
the [provider contract](contracts/provider/v1/README.md). Flow clients use the
separate [data-plane access contract](contracts/flow-access/v1/README.md).

## Public DNS onboarding

The `heterocloud` CLI publishes node-scoped `cloud-<node>` management names
and four cluster-scoped multi-address names: the canonical console domain,
`flow.<domain>`, `registry.<domain>`, and `s3.<domain>`. It discovers the public IPv4 addresses from the
HeteroNetwork-backed Flow TURN `LoadBalancer` Service by default.

For automatic DNS, `dns reconcile` installs a pinned ExternalDNS controller
and applies a provider-neutral `DNSEndpoint`. Node-scoped records remain in
that CRD. The canonical console RRset follows the addresses reported by its
HTTPRoute parent Gateway, while the unified Flow and registry RRsets follow
their annotated LoadBalancer Services. Syouyu's path-style S3 endpoint also
follows the public Gateway. Node failure and recovery therefore
remove and restore every cluster-scoped service address without rerunning the
CLI. The provider is an adapter, so
the same desired records work with Cloudflare, AWS, Google, RFC2136, or an
ExternalDNS webhook without putting provider API calls in HeteroCloud.
The command requires Helm 3, `kubectl`, and cluster permissions to manage a
namespace, the ExternalDNS CRD, and its RBAC resources.

For Cloudflare, create an API token limited to `Zone:Read` and `DNS:Edit` for
the parent zone. Store it without a trailing newline in a private file; do not
put it in a shell argument or `.env` file:

```bash
mkdir -p "$HOME/.config/heterocloud"
umask 077
read -r -s -p "Cloudflare API token: " CF_API_TOKEN
printf '%s' "$CF_API_TOKEN" > "$HOME/.config/heterocloud/cloudflare-token"
unset CF_API_TOKEN

heterocloud dns reconcile \
  --domain heterocloud.mizuame.app \
  --managed-zone mizuame.app \
  --provider cloudflare \
  --credential-file \
    CF_API_TOKEN="$HOME/.config/heterocloud/cloudflare-token" \
  --http-edge-property \
    external-dns.alpha.kubernetes.io/cloudflare-proxied=true \
  --kubeconfig /path/to/admin.conf
```

This creates the dynamically reconciled canonical
`heterocloud.mizuame.app` console and identity RRset, node-specific
`cloud-a/b/c` records, and dynamic Flow and private OCI registry RRsets
containing every healthy public gateway address.
The unified Flow name covers HTTPS, WebSocket, LiveKit, STUN, and TURN client
configuration; the protocol selects the service port. In this Cloudflare
deployment only the canonical console and identity endpoint is proxied.
`flow`, `registry`, `s3`, and node-specific records remain DNS-only because Cloudflare's HTTP
proxy cannot carry the complete RTC and TURN protocol surface. Other
providers can use their equivalent `--http-edge-property` without changing
the desired record set. ExternalDNS uses a TXT ownership registry and is
restricted to HeteroCloud-labelled resources. `--managed-zone` optionally
limits provider discovery to the authoritative parent zone; omit it when the
provider account or workload identity is already scoped to exactly one zone.

Provider authentication may instead use workload identity or a Secret that
already exists in the controller namespace:

```sh
heterocloud dns reconcile \
  --domain heterocloud.example.com \
  --managed-zone example.com \
  --provider aws \
  --provider-values /secure/external-dns-aws-values.yaml \
  --kubeconfig /path/to/admin.conf

heterocloud dns reconcile \
  --domain heterocloud.example.com \
  --managed-zone example.com \
  --provider webhook \
  --credential-secret DNS_API_TOKEN=provider-credentials:api-token \
  --provider-values /secure/external-dns-webhook-values.yaml \
  --kubeconfig /path/to/admin.conf
```

`--provider-values` is for non-secret Helm configuration. Pass provider
secrets with `--credential-file`, an existing Secret, or workload identity.
Use `--provider-arg=--name=value` for non-secret provider flags. Run with
`--dry-run` to inspect the sanitized controller configuration and
`DNSEndpoint` before applying it. Re-running the command is idempotent and
reconciles address changes.

ExternalDNS does not support leader election, so the chart intentionally runs
one writer. The generated Pod uses the `system-cluster-critical` priority and
15-second `NotReady`/`Unreachable` eviction tolerations so Kubernetes replaces
that writer on a surviving control-plane node after a machine failure. Override
the delay with `--controller-failover-seconds` when the cluster's node-failure
detection policy requires a different value.

The controller uses the in-cluster Kubernetes Service by default. This is the
recommended HA path because it follows the surviving API servers. If the
cluster Service IP is not usable, configure an explicit HA virtual endpoint;
never point ExternalDNS at one control-plane node:

```sh
heterocloud dns reconcile \
  --domain heterocloud.example.com \
  --managed-zone example.com \
  --provider cloudflare \
  --credential-secret CF_API_TOKEN=cloudflare-credentials:api-token \
  --controller-node-selector node-role.kubernetes.io/control-plane= \
  --controller-dns-policy Default \
  --controller-kube-api-server https://k8s-api.heteronetwork.internal:7443 \
  --kubeconfig /path/to/admin.conf
```

The generated ConfigMap contains the API URL and references to the Pod's
projected service-account CA and token files. The service-account token itself
remains in the Kubernetes-managed volume and is never stored in the ConfigMap.
The explicit URL must remain reachable when any two control-plane nodes are
offline; a node-scoped URL makes DNS reconciliation a single point of failure.

For manual DNS onboarding, generate a copy-paste-ready zone block:

```sh
heterocloud dns records \
  --domain heterocloud.example.com \
  --kubeconfig /path/to/admin.conf
```

Paste the complete BIND-compatible output into the provider's zone importer.
When Kubernetes discovery is not available, provide every public address
explicitly to either `dns records` or `dns reconcile`:

```sh
heterocloud dns records \
  --domain heterocloud.example.com \
  --public-ip 163.220.236.51 \
  --public-ip 163.220.236.52 \
  --public-ip 163.220.236.53
```

Documentation-range and private addresses are rejected unless
`--allow-non-public` is explicitly set for a lab. After publishing the
records, run the verification command printed at the bottom of the generated
zone block. It checks every A record and fails on missing, extra, IPv6, or
incorrect destinations. `--format table` and `--format json` are available
for providers that do not accept zone imports.

Every public gateway serves the same `flow.<domain>` HTTPS host and forwards
HTTP/WebSocket traffic to the GitOps-managed Envoy Gateway through the
HeteroNetwork LoadBalancer. Envoy routes `/rtc` to the LiveKit signaling
service, `/v1/signal/*` to Flow signaling, and other public paths to the Flow
API. The Flow API, signaling, and LiveKit signaling Services remain private
ClusterIP Services; the Envoy Gateway applies the shared source-IP rate limit
before forwarding. Each Caddy instance obtains its own certificate. Because an
ACME HTTP-01 request can arrive at any address in the RRset, unknown challenge
tokens are forwarded around a bounded gateway ring over HeteroNetwork.
TLS-ALPN validation is disabled for this host. Port 80 must therefore be
reachable between adjacent gateway VPN addresses, and all gateway Caddyfiles
must be deployed before switching DNS to the unified RRset.

## Development

Prerequisites are Rust 1.96.1, Node.js 24, PostgreSQL 17+, and Lean 4.32.2.

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

cd lean
lake build

cd ../apps/console
npm ci
npm test
npm run build
```

Create root-only local secret files and start PostgreSQL before launching the
API:

```sh
printf '%s\n' 'postgres://heterocloud:password@127.0.0.1/heterocloud' \
  > /tmp/heterocloud-database-url
openssl rand -base64 48 > /tmp/heterocloud-csrf-key
openssl rand -base64 48 > /tmp/heterocloud-flow-access-secret
chmod 600 /tmp/heterocloud-database-url /tmp/heterocloud-csrf-key \
  /tmp/heterocloud-flow-access-secret

cargo run -p heterocloud-api -- \
  --database-url-file /tmp/heterocloud-database-url \
  --csrf-key-file /tmp/heterocloud-csrf-key \
  --flow-access-secret-file /tmp/heterocloud-flow-access-secret \
  --flow-public-endpoints http://localhost:8090 \
  --public-origin http://localhost:8080 \
  --secure-cookie=false \
  --console-dir apps/console/dist
```

Bootstrap configuration is accepted only as a complete email/password-file
pair. Remove those settings after the first successful deployment.

## Keycloak OIDC

OIDC is optional. When enabled, configure all four settings together. The
client secret is accepted only through a file:

```sh
printf '%s' 'replace-with-keycloak-client-secret' \
  > /tmp/heterocloud-oidc-client-secret
chmod 600 /tmp/heterocloud-oidc-client-secret

cargo run -p heterocloud-api -- \
  --database-url-file /tmp/heterocloud-database-url \
  --csrf-key-file /tmp/heterocloud-csrf-key \
  --flow-access-secret-file /tmp/heterocloud-flow-access-secret \
  --flow-public-endpoints https://flow.example.test \
  --public-origin https://heterocloud.example.test \
  --oidc-issuer-url https://id.example.test/realms/heterocloud \
  --oidc-client-id heterocloud-web \
  --oidc-client-secret-file /tmp/heterocloud-oidc-client-secret \
  --oidc-public-callback-url \
    https://heterocloud.example.test/api/v1/auth/oidc/callback
```

The equivalent environment variables are
`HETEROCLOUD_OIDC_ISSUER_URL`, `HETEROCLOUD_OIDC_CLIENT_ID`,
`HETEROCLOUD_OIDC_CLIENT_SECRET_FILE`, and
`HETEROCLOUD_OIDC_PUBLIC_CALLBACK_URL`. Configure the Keycloak client as a
confidential OpenID Connect client with Standard Flow enabled and the exact
callback URL above as a valid redirect URI. Realm self-registration remains a
Keycloak policy decision.

The browser starts login at `GET /api/v1/auth/oidc/start`. A successful
callback creates only the user, a personal organization, and its owner
membership, then issues the existing HeteroCloud server session and redirects
to `/`. Projects, services, and quota are never created by registration.
