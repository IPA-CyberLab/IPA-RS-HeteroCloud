# Provider API v1

HeteroCloud calls independently deployed service providers through a private
Kubernetes endpoint. The first provider is `flow`.

Every request carries `Authorization: Bearer <JWT>`. The JWT is signed by the
HeteroCloud provider key and contains:

```json
{
  "iss": "heterocloud",
  "aud": "heterocloud-flow",
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

## Reconcile

`PUT /internal/v1/service-instances/{service_instance_id}`

```json
{
  "generation": 1,
  "name": "production-realtime",
  "spec": {
    "region": "heteronet-global",
    "traffic_mode": "direct",
    "max_participants": 500,
    "turn_enabled": true,
    "metadata": {}
  }
}
```

The provider returns `202 Accepted`, an operation identifier, and a required
provider status object. HeteroCloud records both and marks the instance ready
only if its generation still matches. Status updates never overwrite newer
desired state.

## Delete

`DELETE /internal/v1/service-instances/{service_instance_id}?generation=2`

Deletion is idempotent. Provider-owned rooms, queues, credentials, and usage
state are retained or removed according to the provider retention policy;
HeteroCloud owns only the management-plane instance record.
