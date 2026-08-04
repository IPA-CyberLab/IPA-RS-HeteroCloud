# Architecture

## Ownership

HeteroCloud is a multi-tenant management plane. Its durable identifiers and
authorization decisions are independent of any individual service provider.
Flow receives opaque organization, project, principal, and instance IDs; it
does not read HeteroCloud's customer tables.

HeteroNetwork remains an infrastructure dependency. It exposes the Kubernetes
`heteronetwork.io/public` load-balancer class. Flow fixes TURN to the direct
data path and all other public services to the forwarded data path; this is an
operator-owned deployment policy rather than a customer setting. No
HeteroCloud customer model is compiled into HeteroNetwork.

## DNS reconciliation

HeteroCloud declares public DNS as an `externaldns.k8s.io` `DNSEndpoint`.
ExternalDNS owns provider communication, authentication conventions, retries,
and TXT record conflict detection. The CLI installs a pinned controller chart,
restricts it to the requested domain and HeteroCloud-managed CRDs, then waits
for all expected A records to resolve. Provider selection does not alter the
record model.

This boundary supports native ExternalDNS providers, cloud workload identity,
RFC2136, and provider webhooks. Migrating providers changes controller
configuration and credentials, not HeteroCloud code or service manifests.
The generated endpoint contains node-scoped `cloud-<node>` records and two
multi-address RRsets: the canonical console domain and `flow.<domain>`. The
Flow RRset contains every discovered public gateway address and replaces
separate Flow, RTC, and TURN hostnames. Internal VPN and Kubernetes addresses
are never published by default.

## Request path

1. The Rust API authenticates an opaque session cookie or service-account key.
2. The API resolves one organization-scoped principal.
3. The IAM evaluator rejects cross-organization resources before policy
   matching.
4. Applicable explicit denies override allows. Missing allows deny by default.
5. The decision and Lean semantics digest are written to the audit log.
6. A service mutation commits desired state and an outbox event atomically.
7. A provider worker signs a short-lived `provider/v1` command for Flow.

Flow data-plane access is a separate path. An authenticated user or API key
requests an instance-scoped context. IAM evaluates
`flow:IssueAccessContext` against
`hc:org:<organization UUID>:flow/instance/<instance UUID>`, then the API signs
only the requested allow-listed permissions. For non-owner principals, every
requested data-plane permission also maps to a distinct IAM action evaluated
against that same instance resource. Each allow or deny is audited, and one
explicit or default deny aborts the whole issuance. The instance must already
be `ready`; provisioning and error instances cannot mint credentials. The
project identifier is loaded from the service instance; clients cannot supply
it. The production response carries the unified `flow.<domain>` endpoint;
public DNS selects a gateway from its multi-address RRset.

The browser never receives database credentials, provider credentials,
LiveKit API secrets, or TURN shared secrets.

## IAM

Policy documents use a deliberately small initial language:

```json
{
  "version": "2026-07-31",
  "statements": [
    {
      "effect": "Allow",
      "actions": ["flow:ListInstances", "flow:CreateInstance"],
      "resources": ["hc:org:018f...:flow/*"]
    }
  ]
}
```

Only exact matches and terminal-prefix wildcards are accepted. Unknown fields,
middle wildcards, unknown policy versions, empty statements, and oversized
documents are rejected. Organization owners are a database membership role;
all other principals require an applicable policy.

The Lean kernel consumes the three security facts emitted by matching:
`sameOrganization`, `applicableAllow`, and `applicableDeny`. It proves
cross-organization denial, explicit-deny precedence, default denial, and that
an allow implies every guard. The SHA-256 digest of that source is recorded
with admitted policies and audit decisions.

## Availability

The API is stateless apart from PostgreSQL and runs as three replicas with
anti-affinity, topology spread, readiness checks, and a disruption budget.
Opaque sessions are shared through PostgreSQL, so a replica loss does not log
users out. PostgreSQL must be a separate synchronous HA service with its own
database and role; the HeteroNetwork operator and Keycloak databases are not
reused.

The current HeteroNetwork PostgreSQL primary proxy is loopback-only on every
cluster node. The production profile therefore uses host networking and
required hostname anti-affinity so each HeteroCloud replica reaches its local
HA proxy at `127.0.0.1:25432`. This is an explicit infrastructure constraint,
not a shared-schema dependency. If a private PostgreSQL Service is introduced
later, `hostNetwork` can be disabled without changing the API.

Provider commands use a transactional outbox. A replica or provider outage
therefore delays reconciliation without losing accepted desired state.
After Flow accepts a reconcile operation, the worker records its operation ID
and provider status and changes the instance to `ready` only when the stored
generation still matches the delivered generation.

The HeteroNet production profile schedules all three API replicas and all
three provider workers on the three control-plane nodes. Required anti-affinity
keeps each component at one replica per node.

With host networking enabled, each API process binds only to the downward-API
`status.hostIP` on port 8443. The production Kubernetes Service is ClusterIP;
it does not create a HeteroNetwork public LoadBalancer or expose port 10443.
Public HTTPS terminates only at Caddy on port 443. Each public control-plane
Caddy instance proxies the three HeteroNetwork node IP upstreams on port 8443,
so losing one API or control-plane node does not require a public bypass port.

Flow uses a different gateway policy. Every Caddy instance serves the same
`flow.<domain>` host and sends `/rtc` and `/rtc/*` to LiveKit on its local VPN
address and port 7880, `/v1/signal/*` to local signaling on port 8082, and all
other public paths to the local Flow API on port 8080. `/internal` is denied at
the edge. This keeps the selected public gateway and data-plane process on the
same node.

Each gateway maintains its own public certificate. Flow certificate policies
disable TLS-ALPN validation and use HTTP-01. If the gateway receiving a
challenge does not own its token, it forwards the request to the next gateway
over VPN port 80. The three configurations form a ring. A per-gateway visited
header terminates an unrecognized token after one complete pass, so malformed
requests cannot circulate indefinitely.
