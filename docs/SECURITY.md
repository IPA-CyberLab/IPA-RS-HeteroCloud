# Security model

## Registration and account abuse

Anonymous registration is absent. The first administrator is bootstrapped
from a root-owned password file, and subsequent users must be admitted by an
organization invitation flow. Account creation alone must never grant free
service capacity; quotas and billing eligibility are separate HeteroCloud
state.

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

TLS is mandatory on public deployments. Database URLs, CSRF keys, bootstrap
passwords, TLS private keys, provider signing keys, and service credentials
must be mounted as Kubernetes Secrets; they are never accepted as command-line
values.

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

