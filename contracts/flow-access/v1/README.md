# Flow data-plane access v1

HeteroCloud issues short-lived, service-scoped principal contexts for
HeteroCloud Flow. The context is independent of the Ed25519-signed private
provider API. There are two issuance paths:

- An operator can issue a context directly from an authenticated console or
  HeteroCloud API session for testing.
- An application backend can hold a long-lived `hcf_...` developer credential
  and mint contexts for its own end users.

The developer credential must never be shipped to a browser, mobile client,
or other end-user device. Only the resulting short-lived Flow headers are sent
to that client.

## Operator issuance

Create a context for the authenticated HeteroCloud principal with:

`POST /api/v1/organizations/{organization_id}/realtime/services/{service_instance_id}/access-credentials`

```json
{
  "permissions": ["flow.room.join", "flow.signal.connect"],
  "expires_in_seconds": 300
}
```

Cookie sessions must include the configured `Origin` and
`x-heterocloud-csrf`. HeteroCloud API keys use `Authorization: Bearer` and do
not use CSRF. The caller needs `realtime:IssueAccessCredential` on the exact
service resource and the IAM action mapped to every requested permission.

## Developer credentials

Manage service-scoped developer credentials with these authenticated operator
endpoints:

| Method | Path | Effect |
| --- | --- | --- |
| `GET`, `POST` | `/api/v1/organizations/{organization_id}/realtime/services/{service_instance_id}/developer-credentials` | List or create credentials |
| `POST` | `/api/v1/organizations/{organization_id}/realtime/services/{service_instance_id}/developer-credentials/{credential_id}/rotate` | Replace the secret and revoke every active child context |
| `DELETE` | `/api/v1/organizations/{organization_id}/realtime/services/{service_instance_id}/developer-credentials/{credential_id}` | Revoke the credential and every active child context |
| `GET` | `/api/v1/organizations/{organization_id}/realtime/services/{service_instance_id}/access-contexts` | List all contexts issued for the service |
| `DELETE` | `/api/v1/organizations/{organization_id}/realtime/services/{service_instance_id}/access-contexts/{context_id}` | Revoke one context |

Create a credential with a name, an expiration of 1 to 365 days, and the
maximum permissions that it may delegate:

```json
{
  "name": "production-backend",
  "expires_in_days": 90,
  "permissions": ["flow.room.join", "flow.signal.connect"]
}
```

The response returns the complete `credential` and `mint_endpoint` only when
the credential is created or rotated. HeteroCloud stores only a SHA-256 hash
and the non-secret prefix. The complete secret cannot be read again.

The application backend mints an end-user context with the returned
credential:

```http
POST /api/v1/flow/v1/access-credentials
Authorization: Bearer hcf_<prefix>_<secret>
Content-Type: application/json
```

```json
{
  "principal_id": "0198a118-073f-79e4-9ca4-0c1c2501c031",
  "permissions": ["flow.room.join", "flow.signal.connect"],
  "expires_in_seconds": 300
}
```

`principal_id` is the application's stable UUID for the end user. Requested
permissions must be a subset of the developer credential's permission ceiling.
The developer credential can list only its own issued contexts with
`GET /api/v1/flow/v1/access-credentials` and can idempotently revoke one with:

```http
DELETE /api/v1/flow/v1/access-credentials/{context_id}
Authorization: Bearer hcf_<prefix>_<secret>
```

Mint and revoke operations are written to the HeteroCloud audit log without
the credential secret, hash, or prefix.

## Permissions and lifetime

The context lifetime defaults to 300 seconds and must be between 30 and 300
seconds. Permissions are a nonempty subset of:

| Data-plane permission | Required operator IAM action |
| --- | --- |
| `flow.queue.read` | `flow:QueueRead` |
| `flow.queue.write` | `flow:QueueWrite` |
| `flow.room.create` | `flow:RoomCreate` |
| `flow.room.read` | `flow:RoomRead` |
| `flow.room.join` | `flow:RoomJoin` |
| `flow.turn.issue` | `flow:TurnIssue` |
| `flow.signal.connect` | `flow:SignalConnect` |
| `flow.metrics.read` | `realtime:GetMetrics` |

Wildcards are not accepted. The Flow service must be in `ready` state.
HeteroCloud evaluates the issuance action and each mapped permission in sorted
order. Explicit deny and default deny reject the entire operation.

## Signed context

The signed JSON object has this shape. `permissions` is sorted and
duplicate-free:

```json
{
  "issuer": "heterocloud",
  "audience": "heterocloud-flow-data",
  "organization_id": "organization UUID",
  "project_id": "project UUID",
  "service_instance_id": "service instance UUID",
  "principal_id": "principal UUID",
  "permissions": ["flow.room.join", "flow.signal.connect"],
  "issued_at": 1785480000,
  "expires_at": 1785480300,
  "context_id": "context UUID"
}
```

Compact JSON is encoded with unpadded base64url as `x-flow-principal`.
`x-flow-timestamp` is the decimal `issued_at`. `x-flow-signature` is unpadded
base64url HMAC-SHA256 over:

```text
x-flow-timestamp + "." + x-flow-principal
```

The API response returns these three header values, the scoped IDs, the
service rate-limit policy, and the configured `endpoints` array. Clients pass
the header values unchanged to Flow. Responses containing secrets or signed
contexts use `Cache-Control: no-store`.

## Revocation semantics

HeteroCloud persists every context before returning it. Individual revoke,
developer credential revoke, and credential rotation write revocation events
through the transactional outbox. Flow shares revocation state in PostgreSQL
across all replicas.

- Flow REST requests reject a revoked context with `401 invalid_credentials`.
- P2P signaling checks at authentication and every heartbeat, so an existing
  socket closes within 15 seconds of Flow receiving the revocation.
- If the revocation database is unavailable, REST and signaling fail closed.
- LiveKit participant JWTs and coturn REST credentials already issued by Flow
  cannot be recalled by those downstream protocols. Their lifetime is capped
  to the remaining context lifetime, which is at most 300 seconds.

Revoking or rotating a developer credential invalidates that credential in
HeteroCloud immediately and transactionally marks all of its unexpired child
contexts revoked. Repeating a context revoke is safe and does not create a
second provider event.

## Runtime secret

The API requires `--flow-access-secret-file`; the resolved target must be
regular, nonempty, and at least 32 bytes. It may be owner-readable or readable
by the dedicated runtime group, but it must not be group-writable/executable
or accessible by other users. The Helm chart mounts it from the separate
`heterocloud-flow-access` Secret by default. HeteroCloud and Flow must use the
same HMAC secret, issuer, and audience.

`--flow-public-endpoints` (or `HETEROCLOUD_FLOW_PUBLIC_ENDPOINTS`) is a required
comma-delimited list of one to sixteen origin-only URLs. Credentials, paths,
queries, fragments, and duplicates are rejected. Every endpoint must use HTTPS
when secure mode is enabled.
