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
the [provider contract](contracts/provider/v1/README.md).

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
chmod 600 /tmp/heterocloud-database-url /tmp/heterocloud-csrf-key

cargo run -p heterocloud-api -- \
  --database-url-file /tmp/heterocloud-database-url \
  --csrf-key-file /tmp/heterocloud-csrf-key \
  --public-origin http://localhost:8080 \
  --secure-cookie=false \
  --console-dir apps/console/dist
```

Bootstrap configuration is accepted only as a complete email/password-file
pair. Remove those settings after the first successful deployment.

