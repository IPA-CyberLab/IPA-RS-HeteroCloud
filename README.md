# HeteroCloud

HeteroCloud is the public control plane for services running on a
HeteroNetwork Kubernetes cluster. It owns customer identity, organizations,
projects, IAM, quotas, service lifecycle metadata, and audit records. Service
data planes such as Flow remain independent repositories and are connected
through a versioned provider contract.

HeteroCloud does not own VPN membership, node reachability, public-IP
selection, packet forwarding, LiveKit, TURN, or matchmaking.

## Current vertical slice

- Invite/bootstrap-only local accounts with Argon2id password hashes.
- Server-side sessions using an opaque 256-bit cookie and database-backed
  revocation.
- Origin and CSRF validation on every cookie-authenticated mutation.
- Organizations, projects, user and service-account principals, IAM policies,
  policy bindings, Flow service instances, an outbox, and audit events.
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

The `heterocloud` CLI manages one hostname per public failure domain. It
discovers the public IPv4 addresses from the HeteroNetwork-backed Flow RTC
`LoadBalancer` Service by default.

For automatic DNS, `dns reconcile` installs a pinned ExternalDNS controller
and applies a provider-neutral `DNSEndpoint`. The provider is an adapter, so
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
  --provider cloudflare \
  --credential-file \
    CF_API_TOKEN="$HOME/.config/heterocloud/cloudflare-token" \
  --kubeconfig /path/to/admin.conf
```

This creates `cloud-a/b/c`, `flow-a/b/c`, `rtc-a/b/c`, and `turn-a/b/c` as
DNS-only A records. Cloudflare proxying is deliberately not enabled because
the RTC and TURN endpoints are not ordinary HTTP traffic. ExternalDNS uses a
TXT ownership registry and is restricted to the requested domain and
HeteroCloud-labelled resources.

Provider authentication may instead use workload identity or a Secret that
already exists in the controller namespace:

```sh
heterocloud dns reconcile \
  --domain heterocloud.example.com \
  --provider aws \
  --provider-values /secure/external-dns-aws-values.yaml \
  --kubeconfig /path/to/admin.conf

heterocloud dns reconcile \
  --domain heterocloud.example.com \
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

When the cluster Service IP is not a reliable path to the Kubernetes API,
configure the controller Pod and an explicit API endpoint independently of
the DNS provider:

```sh
heterocloud dns reconcile \
  --domain heterocloud.example.com \
  --provider cloudflare \
  --credential-secret CF_API_TOKEN=cloudflare-credentials:api-token \
  --controller-node-selector node-role.kubernetes.io/control-plane= \
  --controller-dns-policy Default \
  --controller-kube-api-server https://10.250.0.4:6443 \
  --kubeconfig /path/to/admin.conf
```

The generated ConfigMap contains the API URL and references to the Pod's
projected service-account CA and token files. The service-account token itself
remains in the Kubernetes-managed volume and is never stored in the ConfigMap.

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

Per-node names are deliberate: they preserve ordered endpoint failover and
allow each gateway to obtain its own TLS certificate. Do not collapse them
into one unmanaged round-robin A record.

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
