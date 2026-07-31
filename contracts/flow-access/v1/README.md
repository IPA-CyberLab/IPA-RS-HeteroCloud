# Flow data-plane access v1

HeteroCloud issues short-lived, instance-scoped authentication contexts for
HeteroCloud Flow. This credential is independent of the Ed25519-signed private
provider API.

Create a context with:

`POST /api/v1/organizations/{organization_id}/flow/instances/{service_instance_id}/access-contexts`

Cookie sessions must include the configured `Origin` and
`x-heterocloud-csrf`. HeteroCloud API keys use `Authorization: Bearer` and do
not use CSRF. The caller needs `flow:IssueAccessContext` on the exact instance
resource. The Flow service instance must be in `ready` state. Non-owner
principals must also be authorized for every requested permission on that same
resource:

| Data-plane permission | Required IAM action |
| --- | --- |
| `flow.queue.read` | `flow:QueueRead` |
| `flow.queue.write` | `flow:QueueWrite` |
| `flow.room.create` | `flow:RoomCreate` |
| `flow.room.read` | `flow:RoomRead` |
| `flow.room.join` | `flow:RoomJoin` |
| `flow.turn.issue` | `flow:TurnIssue` |
| `flow.signal.connect` | `flow:SignalConnect` |

HeteroCloud evaluates `flow:IssueAccessContext` first and then each mapped
action in sorted permission order. Explicit deny and default deny reject the
entire issuance. Every decision is written as a separate audit event. An
organization owner retains the existing owner override for each decision.

```json
{
  "permissions": ["flow.room.join", "flow.signal.connect"],
  "expires_in_seconds": 300
}
```

The lifetime defaults to 300 seconds and must be between 30 and 300 seconds.
Permissions are a nonempty subset of:

- `flow.queue.read`
- `flow.queue.write`
- `flow.room.create`
- `flow.room.read`
- `flow.room.join`
- `flow.turn.issue`
- `flow.signal.connect`

Wildcards are not accepted. The signed JSON object has this exact field order
and shape; `permissions` is sorted and duplicate-free:

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

The API response returns these three header values, all opaque scope IDs, and
the configured `endpoints` array. Clients try the concrete endpoints according
to their own failover policy and pass the header values unchanged to Flow.
Responses are non-cacheable. HeteroCloud does not treat a hostname backed by a
single embedded-IP DNS service as multi-endpoint failover.

The API requires `--flow-access-secret-file`; the resolved target must be
regular, nonempty, and at least 32 bytes. It may be owner-readable or readable
by the dedicated runtime group, but it must not be group-writable/executable
or accessible by other users. This permits Kubernetes projected Secret
symlinks without accepting a broadly accessible target. The Helm chart mounts
it from the separate `heterocloud-flow-access` Secret by default. HeteroCloud
and Flow must use the same HMAC secret, issuer, and audience.

`--flow-public-endpoints` (or `HETEROCLOUD_FLOW_PUBLIC_ENDPOINTS`) is a required
comma-delimited list of one to sixteen origin-only URLs. Credentials, paths,
queries, fragments, and duplicates are rejected. Every endpoint must use HTTPS
when secure mode is enabled. The HeteroNet production profile returns one
concrete HTTPS endpoint for each of the three public control-plane nodes.
