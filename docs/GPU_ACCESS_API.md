# GPU inventory and access API

Flash GPU inventory comes from the provider's `FlashGpuDevice` resources. The
HeteroCloud database is a synchronized cache used for authorization and service
requester identity; operators do not create physical GPUs in HeteroCloud.

The canonical type for the two uc-k8sp5 cards is
`nvidia-geforce-gtx-1080-ti`. Its display name is
`NVIDIA GeForce GTX 1080 Ti`.

## User catalog

`GET /api/v1/flash/gpu-types` requires a session or API key. It returns only
types the actor can request:

```json
{
  "items": [
    {
      "gpu_type": "nvidia-geforce-gtx-1080-ti",
      "display_name": "NVIDIA GeForce GTX 1080 Ti",
      "access": "open",
      "total": 2,
      "available": 2
    }
  ]
}
```

`total` counts physical devices of that type visible to the actor. `available`
counts provider-reported devices that are healthy and currently unleased.
Availability is advisory and may change immediately. A busy type remains
requestable: create/update checks access, accepts the service, and the Flash
scheduler queues it until a lease is available. The provider is the only source
of live lease capacity.
If any accessible device of a type is open, `access` is `open`; otherwise it is
`private`.

Physical IDs, node names, PCI addresses, device UUIDs, and assignment lists are
never included in this response. A Flash create or update specifies only:

```json
{"gpu_type": "nvidia-geforce-gtx-1080-ti"}
```

Omitting `gpu_type` requests no GPU. `gpu_count`, device IDs, and other unknown
fields are rejected. GPU services currently require `replicas: 1`; when
autoscaling is present its `max_replicas` must also be `1` (`min_replicas` may
be `0` or `1`). A GPU type is accepted only when it is a lowercase DNS label,
exists in the synchronized inventory, and is visible to the authenticated user.
It does not need to be available when the request is created. Private access
uses the internal `users.id` resolved from the existing session/OIDC identity;
raw OIDC subjects are not stored in GPU assignments.

## Owner API

`GET /api/v1/owner/gpus` requires the existing owner boundary. It refreshes the
provider catalog and returns physical records:

```json
{
  "items": [
    {
      "id": "0199...",
      "management_id": "uc-k8sp5/GPU-...",
      "gpu_type": "nvidia-geforce-gtx-1080-ti",
      "display_name": "NVIDIA GeForce GTX 1080 Ti",
      "available": true,
      "visibility": "private",
      "assigned_user_ids": ["0199..."],
      "created_at": "2026-09-18T00:00:00Z",
      "updated_at": "2026-09-18T00:00:00Z"
    }
  ]
}
```

`PUT /api/v1/owner/gpus/{id}` requires same-origin and CSRF checks. The complete
request is:

```json
{
  "visibility": "private",
  "assigned_user_ids": ["0199..."]
}
```

For `open`, `assigned_user_ids` must be empty. IDs must be unique active users
from `GET /api/v1/owner/accounts` (`items[].user.id`). The API pushes the update
to the provider CRD, pulls the catalog again, and then returns the updated
record. A provider failure returns 503 and does not mutate the cache.

`private` with an empty assignment list is valid and intentionally isolates the
device from every user. Removing the final assignment never changes visibility
to `open`.

Changing visibility, assignments, type, or physical inventory enqueues
reconciliation for affected Flash services. Removing a user's private access
does not silently authorize an old request: subsequent creates and updates are
rejected, while already accepted services are sent back to the provider for
reconciliation.

## Provider synchronization contract

HeteroCloud uses the existing signed provider JWT with nil resource UUIDs,
generation `1`, and these actions:

- `GET /internal/v1/gpus`, action `flash.gpus.catalog.list`, returns
  `{ "items": [{ "management_id", "gpu_type", "display_name", "visibility",
  "assigned_user_ids", "available" }] }`.
- `PUT /internal/v1/gpus/access`, action `flash.gpus.access.update`, accepts
  `{ "management_id", "gpu_type", "display_name", "visibility",
  "assigned_user_ids" }`. HeteroCloud echoes the catalog type and display name,
  so the provider can reject stale identity updates.

Service reconcile JWTs retain `sub` as the IAM `PrincipalId` and add an optional
signed `user_id`. HeteroCloud resolves that value from the active user principal;
service accounts and disabled/deleted users produce no `user_id` and therefore
can use open GPUs only. The scheduler must use `user_id`, never `sub`, for
private GPU matching.

Each catalog item also has `available`, which is true only while that physical
device is healthy and unleased. Missing `visibility`, `assigned_user_ids`, and
`available` default to `open`, an empty list, and false. A full catalog pull
inserts new devices, updates existing devices by `management_id`, and removes
devices no longer present in the provider.

Migration `0017_gpu_inventory_access.sql` upgrades stored `gpu_count: 1` Flash
specifications to `gpu_type: "nvidia-geforce-gtx-1080-ti"`, removes zero-valued
`gpu_count`, constrains migrated GPU services to one replica, and records their
latest reconcile requester metadata, when present, for later access
reconciliation. Weekly GPU quota fields and behavior are unchanged.
