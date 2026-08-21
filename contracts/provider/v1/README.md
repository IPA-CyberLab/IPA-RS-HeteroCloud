# Provider API v1

HeteroCloud calls independently deployed service providers through a private
Kubernetes endpoint. Provider API v1 supports `flow` and `flash` service
instances. Service-instance outbox events are routed by their immutable
provider field.

| Provider | Required JWT audience | Worker endpoint setting |
| --- | --- | --- |
| `flow` | `heterocloud-flow` | `HETEROCLOUD_FLOW_ENDPOINT` |
| `flash` | `heterocloud-flash` | `HETEROCLOUD_FLASH_ENDPOINT` |

Every request carries `Authorization: Bearer <JWT>`. The JWT is signed by the
HeteroCloud provider key and contains:

```json
{
  "iss": "heterocloud",
  "aud": "heterocloud-flash",
  "sub": "principal UUID",
  "organization_id": "organization UUID",
  "project_id": "project UUID",
  "service_instance_id": "instance UUID",
  "action": "service-instance.reconcile",
  "generation": 1,
  "jti": "request UUID",
  "iat": 1785480000,
  "nbf": 1785480000,
  "exp": 1785480060
}
```

Providers must validate the signature, exact issuer and audience, action,
expiry, and monotonic generation. They must scope every database operation by
both `organization_id` and `project_id`. Repeated `jti` values are idempotent.
An otherwise valid token for one provider must not be accepted by another
provider because the audiences are distinct.

## Flash management API

The public HeteroCloud API exposes Flash instances through these
organization-scoped routes:

| Method | Route | IAM action | IAM resource |
| --- | --- | --- | --- |
| `GET` | `/api/v1/organizations/{organization_id}/flash/services` | `flash:ListInstances` | `hc:org:{organization_id}:flash/*` |
| `POST` | `/api/v1/organizations/{organization_id}/flash/services` | `flash:CreateInstance` | `hc:org:{organization_id}:flash/*` |
| `GET` | `/api/v1/organizations/{organization_id}/flash/services/{id}` | `flash:GetInstance` | `hc:org:{organization_id}:flash/instance/{id}` |
| `PUT` | `/api/v1/organizations/{organization_id}/flash/services/{id}` | `flash:UpdateInstance` | `hc:org:{organization_id}:flash/instance/{id}` |
| `DELETE` | `/api/v1/organizations/{organization_id}/flash/services/{id}` | `flash:DeleteInstance` | `hc:org:{organization_id}:flash/instance/{id}` |

Create accepts `project_id`, `name`, and `spec`. Update is a complete
replacement and accepts `name` and `spec`; omitted fields are not inherited.
The Flash spec is strict and rejects unknown fields:

```json
{
  "region": "heteronet-global",
  "image": "ghcr.io/example/game-server:v1",
  "replicas": 3,
  "cpu_millis": 500,
  "memory_mib": 512,
  "ports": [
    {
      "name": "game-udp",
      "protocol": "udp",
      "container_port": 7777,
      "service_port": 7777
    }
  ],
  "exposure": {
    "type": "public",
    "traffic_mode": "direct"
  },
  "env": {"LOG_LEVEL": "info"},
  "command": ["/app/server"],
  "args": ["--port=7777"],
  "metadata": {}
}
```

`protocol` is `tcp` or `udp`; exposure `type` is `internal` or `public`;
`traffic_mode` is `forwarded` or `direct`. Internal exposure always uses
`forwarded`. Hard validation limits are 1..100 replicas, 10..64000 CPU millis,
16..262144 MiB memory, 1..16 unique ports,
128 environment variables, 128 command elements, 256 argument elements, and
64 KiB of serialized metadata. Port names and protocol/service-port pairs are
unique. The provider owns enforcement of gVisor execution and exposure policy;
clients cannot select a runtime class through this contract.

## Reconcile

`PUT /internal/v1/service-instances/{service_instance_id}`

```json
{
  "generation": 1,
  "name": "production-realtime",
  "spec": {
    "region": "heteronet-global",
    "max_participants": 500,
    "max_rooms": 100,
    "rate_limit": {
      "requests_per_second": 20,
      "burst": 40
    },
    "metadata": {}
  }
}
```

For a `flash` instance, the same envelope carries the strict Flash spec from
the management API. HeteroCloud does not translate image, port, environment,
resource, or exposure fields before signing the provider request.

TURN is not a service mode. Flow always supplies STUN and short-lived TURN
credentials so normal ICE can prefer a direct path and use TURN automatically
when direct connectivity checks fail.

The provider returns `202 Accepted`, an operation identifier, and a required
provider status object. HeteroCloud records both and marks the instance ready
only if its generation still matches. Status updates never overwrite newer
desired state.

## Delete

`DELETE /internal/v1/service-instances/{service_instance_id}?generation=2`

Deletion is idempotent. Provider-owned rooms, queues, credentials, and usage
state are retained or removed according to the provider retention policy;
HeteroCloud owns only the management-plane instance record.
