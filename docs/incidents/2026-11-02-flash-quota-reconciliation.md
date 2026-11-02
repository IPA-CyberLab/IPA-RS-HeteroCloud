# Flash quota refresh retries for an unavailable service

On 2026-11-02, organization-wide quota activation checks found an undelivered
`service-instance.flash-quota` outbox event. It had been retried over one
thousand times since 2026-10-30 for service
`01a1005f-0743-7d32-9e86-271cf23f8da7`. The Flash provider had applied the
requested policy but responded with HTTP 503 because the workload was still
provisioning. Its referenced `whisper-auth-server` image digest was absent from
the registry. This report does not establish why that image was absent.

The same endpoint previously treated a quota refresh as a full service
reconciliation. It therefore required the workload to become ready before the
quota event could be acknowledged. A quota event that succeeded also flowed
through the normal service lifecycle update, which would have been unsafe if an
unready workload were acknowledged early.

Flash now accepts a quota-only reconciliation only for an existing service with
the same generation and unchanged workload, name and project. It performs a
merge patch containing only the quota fields, guarded by the Kubernetes resource
version; a concurrent service edit causes a conflict instead of being overwritten.
The worker acknowledges that quota event without marking the service ready or
error. Older provider versions reject the new request field and the worker
retries, so the Flash provider must be deployed before the worker. A missing
container image still prevents the affected workload from starting.

Validation covers a signed Flash API request for an unready service, rejection
of changed workload content, and normal reconciliation returning 503 while
unready. A worker integration test uses PostgreSQL and a mock provider to verify
that successful quota updates leave a provisioning/error service's lifecycle
untouched. A Kubernetes E2E creates a stopped service in an isolated namespace,
verifies that an outdated resource-version patch is rejected, and deletes the
test namespace. Organization weekly activation checks the actual pod images,
provider CRD, outbox and every current provider policy before reporting success.

The production rollout completed on 2026-11-02: the previously stuck event was
acknowledged on attempt 1,064 while the service correctly remained `updating`.
The subsequent organization-quota activation converged for all 12 existing
Flash services and left no pending quota-refresh events. Both Flash and
HeteroCloud maintained two ready serving replicas per component; the desired
HeteroCloud replica count is three, with one pending while `uc-k8sp4` remains
excluded. The affected image itself has not been recovered.
