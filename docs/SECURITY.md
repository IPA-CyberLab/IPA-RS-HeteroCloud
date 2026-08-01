# Security model

## Registration and account abuse

HeteroCloud's local anonymous registration endpoint is absent. The first
local administrator is bootstrapped from a root-owned password file, and
subsequent local users must be admitted by an organization invitation flow.
An optional Keycloak realm may expose its own self-registration policy. A
first successful OIDC login creates only a user, personal organization, user
principal, and owner membership. Account creation alone never grants a
project, service instance, quota, or billing eligibility.

OIDC identities are keyed by the exact verified `(issuer, subject)` pair.
HeteroCloud never links an OIDC identity to an existing local or external
account based only on a matching email address. An email collision rejects
the new identity instead. OIDC-only users have no password hash.

Organization invitations are single-use and expire within 24 hours. The API
performs an indexed availability lookup before running Argon2id, then locks
and revalidates the invitation row in the registration transaction. This
limits CPU amplification from invalid invitation traffic while ensuring that
only one of several concurrent registration attempts can consume a code.
Unavailable, expired, revoked, and consumed codes share one public error.

Future public admission should combine phone verification, payment
verification, per-phone uniqueness using a keyed HMAC index, cooldowns, and
IP/ASN/device rate limits. SMS alone is not a Sybil defense.

## Browser sessions

- Passwords are hashed with Argon2id using 64 MiB, three iterations, and a
  random 128-bit salt.
- Session cookies contain 256 random bits and only a SHA-256 digest is stored.
- Cookies are `HttpOnly`, `Secure` in production, `SameSite=Lax`, and
  path-limited to `/`.
- Every mutation requires an exact configured `Origin` and a session-bound
  HMAC CSRF token.
- Session revocation and expiry are database-backed and consistent across API
  replicas.
- OIDC uses Authorization Code with S256 PKCE. State, nonce, verifier, and
  issuance time are held in a five-minute, signed, `HttpOnly`, `SameSite=Lax`
  cookie. Production cookies are `Secure`.
- Discovery metadata must report the exact configured issuer. Authorization,
  token, and JWKS endpoints must remain on that issuer origin. ID tokens are
  checked against an asymmetric JWKS key for signature, algorithm, issuer,
  audience, expiry, subject, authorized party, and nonce.

TLS is mandatory on public deployments. Database URLs, CSRF keys, bootstrap
passwords, OIDC client secrets, TLS private keys, Flow access HMAC secrets,
provider signing keys, and service credentials must be mounted as Kubernetes
Secrets; they are never accepted as command-line values.

DNS provider tokens follow the same rule. The supported credential paths for
`heterocloud dns reconcile` are a regular local file with mode `0400` or
`0600` on Unix, or a pre-existing Secret in the ExternalDNS namespace. Local
contents are validated in zeroizing memory and piped from `kubectl` to
`kubectl`; they are not embedded in child-process arguments, manifests,
dry-run output, or repository configuration. Provider values and flags must
contain only non-secret configuration. Kubernetes Secret encryption at rest
and least-privilege provider tokens remain deployment requirements.

ExternalDNS is restricted by an exact domain filter, a HeteroCloud label
selector, A-record type filtering, and a unique TXT owner ID. Cloudflare
records remain DNS-only by default so non-HTTP RTC and TURN traffic cannot be
silently routed through an incompatible proxy.

## Authorization

IAM is default-deny. The organization boundary is evaluated before policy
documents, explicit deny wins, policy versions are pinned, and every decision
is audited. HeteroCloud IDs rather than email addresses are authorization
keys.

Lean proves the final authorization truth table. Rust unit tests cover the
same table; release validation must build both implementations before images
are published.

## Service isolation

HeteroCloud and Flow use separate databases, Kubernetes service accounts,
Secrets, NetworkPolicies, container images, and release lifecycles. Provider
tokens are short-lived, audience-bound, and contain only opaque tenant
context. A Flow compromise must not grant access to HeteroCloud password
hashes, sessions, IAM policy storage, or provider signing keys.

Flow data-plane contexts expire after 30 to 300 seconds and contain exact,
allow-listed permissions. Wildcards are rejected. Their HMAC secret is a
dedicated file readable only by the API process owner or its dedicated runtime
group, and is not the Ed25519 key used by the provider worker. Responses
containing signed headers are marked `Cache-Control: no-store`.

`flow:IssueAccessContext` is necessary but cannot delegate permissions by
itself. Each requested Flow permission requires its fixed instance-scoped IAM
action, and the existing explicit-deny/default-deny semantics apply to every
mapping. Each decision is audited before any credential is signed. Access
contexts are issued only after generation-guarded reconciliation has moved the
service instance to `ready`.

Secure mode accepts only HTTPS Flow public endpoints. HeteroCloud returns the
configured concrete endpoint list without converting it into a synthetic
single-host failover claim.

The HeteroNet production API does not listen on every host interface and does
not use a public Kubernetes LoadBalancer. It binds to the Kubernetes host IP on
port 8443, while Caddy is the only public listener on port 443 and proxies over
HeteroNetwork node addresses. This prevents direct access that bypasses the
public TLS gateway policy.
