# Flash Autoscaling and Endpoint Modes

The [Flash OpenAPI 3.1 contract](../contracts/api/v1/flash.openapi.json) describes
the read, create and replacement-update schemas. The API uses the same typed
`FlashSpec` validation before enqueueing provider reconciliation.

`spec.autoscaling` is optional. When present, it contains required integer
`min_replicas` and `max_replicas`, and optional integer
`target_cpu_utilization_percent` and `target_memory_utilization_percent`.
At least one non-null target is required; each target must be in `1..100`.
`min_replicas >= 1`, `max_replicas >= min_replicas`, and the existing `replicas`
value must be within these bounds. Existing per-field and tenant limits apply.
Omission or null disables autoscaling. Updates replace the entire spec.

`spec.exposure.endpoint_mode` accepts `ip` (default), `load_balancer`, or `web`.
`load_balancer` and `web` are valid only with `type: public` and
`traffic_mode: forwarded`.
Existing IP exposure rules remain unchanged, including internal exposure requiring
forwarded traffic. Autoscaling and endpoint mode are independent settings.

Fixed-service JSON is unchanged: absent autoscaling and the default `ip` endpoint
mode are omitted from serialized specs. Provider reconciliation receives the
non-default fields unchanged. Provider implementation and endpoint provisioning
remain the provider's responsibility.

## GPU selection

`spec.gpu_type` is optional. A canonical type such as
`nvidia-geforce-gtx-1080-ti` assigns one exclusive physical GPU of that type to
the service. The API checks open/private access, while provider-reported
availability remains advisory. A request is accepted when every accessible GPU
of that type is busy; the Flash scheduler queues it until a lease is available.
GPU services currently require `replicas: 1` and, when autoscaling is enabled,
`max_replicas: 1`. Users cannot submit a node, device UUID, or other physical
identity. Omitting the field keeps the existing gVisor path. Inventory and owner
contracts are described in [GPU_ACCESS_API.md](GPU_ACCESS_API.md).

## Quota Reservation

Creation and update reserve `autoscaling.max_replicas`, or `replicas` when fixed.
This value is checked against the per-service replica ceiling and counted toward
tenant aggregate replicas, CPU (`reserved * cpu_millis`), memory
(`reserved * memory_mib`) and disk (`reserved * ephemeral_storage_gib`). Existing
autoscaled services are counted using their ceilings too. A service update replaces
its previous reservation rather than adding to it. The existing tenant/allocation
transaction locks serialize concurrent reservations; rejected writes do not change
the spec, generation or reconcile outbox. Deleting services retain existing quota
release behavior. Owner usage reports use the same reservation calculation, not
live running replica counts.

## Weekly Runtime Quotas and Usage

Flash enforces three organization-shared weekly runtime limits:
`max_weekly_cpu_millicore_seconds`, `max_weekly_memory_mib_seconds`, and
`max_weekly_gpu_seconds`. The defaults divide the schedulable fleet across 30
accounts: 901.6 vCPU-hours, about 769.39 GiB-hours, and 11.2 GPU-hours per
account per week. All counters reset Monday at 00:00 UTC. A zero limit disables
that resource for the account.

The provider counts allocated resources for Ready replicas, so scale-to-zero
time is free. It stores each service's current-week counter independently from
the service resource; deleting a service therefore does not erase usage already
consumed. The authenticated tenant endpoint
`GET /api/v1/organizations/{organization_id}/flash/usage` returns totals,
current allocation, limits, and service-level history. The owner-only
`GET /api/v1/owner/cost-management` returns the same view across every cloud
account. Both console views label the values as vCPU-hours, GiB-hours, and
GPU-hours; no currency rate is implied.

## Web Publication

`web` publishes an HTTP application through a provider-managed ClusterIP Service
and HTTPRoute. It requires exactly one port with `protocol: tcp`. The application
serves HTTP on that port's `container_port`; the external endpoint is a hostname
with `port: 443` and `protocol: tcp`, opened as `https://<hostname>/` without a
custom port suffix. Wildcard TLS terminates at the parent-managed Caddy gateway,
which forwards to Envoy. The assigned spec `service_port` is not the public web
port. Existing IP and load-balancer endpoint semantics are unchanged.

Both `allowed_source_cidrs` and `denied_source_cidrs` must be omitted or empty for
`web`, including when changing an existing service to web mode. Nonempty lists
are rejected, not discarded: network-layer source ACLs cannot be assumed to
enforce original client addresses through the HTTP proxy path. Web publication
does not yet support these ACLs. Application authentication remains separate.
Egress configuration retains its existing validation and behavior.

Web publication works with fixed replicas or autoscaling; quota reservation is
unchanged. Configure it with the [web manifest](../examples/cli/flash-web.json):

```sh
heterocloud flash create --file examples/cli/flash-web.json
```

Set the example project ID and image for your environment. For replacement
updates, omit `project_id` and keep `name` and `spec`.

## Live Status on Reads

Authorized Flash detail and list GETs refresh autoscaled services using signed
`GET /internal/v1/service-instances/{id}?generation=N` with provider action
`flash.status.get`. The provider returns raw `FlashServiceStatus` JSON. Its
`observed_generation` must exactly match the service generation; malformed or
mismatched responses are not displayed as current. The response's inner
`status.status` is replaced with this live status, including `desired_replicas`,
`ready_replicas`, `phase` and `endpoints`. Fixed services are returned unchanged.

Refresh is response-only: GET never calls provider PUT, writes the database,
changes lifecycle state/generation, or enqueues reconciliation. A temporary
provider `provisioning` phase during HPA scaling does not reset HeteroCloud's
stored `ready` lifecycle state. Deleting services are not queried.

Each detail refresh has a two-second deadline. Lists preserve order, use at most
four concurrent provider requests, and share one absolute two-second refresh
budget including queue time, regardless of list size (the current list query has
no pagination limit). This budget covers provider refresh, not authentication or
the database query.

On timeout, unavailable provider configuration, non-success HTTP, malformed JSON,
or generation mismatch, the API still returns the service/list successfully.
The inner status has `live_status_unavailable: true`; `desired_replicas` and
`ready_replicas` are removed. Cached endpoints and other cached status fields may
remain, but are not guaranteed current. Clients must not substitute requested
replicas for missing current counts. Success removes the unavailable marker.

## CLI

The CLI uses JSON manifests, with no separate replica or exposure flags:

```sh
heterocloud flash create --file examples/cli/flash-autoscaling.json
```

Set the project ID and image for your environment before submitting. The separate
[fixed example](../examples/cli/flash.json) remains unchanged. For `flash update`,
use a manifest with `name` and `spec` only (no `project_id`). The autoscaling example
reserves four replicas, 4000 millicores, 8192 MiB memory and 40 GiB disk.
