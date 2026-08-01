import { queryOptions } from "@tanstack/react-query";
import { api } from "@/lib/api-client";
import type { RealtimeMetricsRange } from "@/lib/api-types";

export const organizationsQueryOptions = queryOptions({
  queryKey: ["organizations"],
  queryFn: ({ signal }) => api.organizations.list(signal),
});

export function projectsQueryOptions(organizationId: string) {
  return queryOptions({
    queryKey: ["organizations", organizationId, "projects"],
    queryFn: ({ signal }) => api.projects.list(organizationId, signal),
  });
}

export function iamPrincipalsQueryOptions(organizationId: string) {
  return queryOptions({
    queryKey: ["organizations", organizationId, "iam", "principals"],
    queryFn: ({ signal }) => api.iam.principals.list(organizationId, signal),
  });
}

export function iamPoliciesQueryOptions(organizationId: string) {
  return queryOptions({
    queryKey: ["organizations", organizationId, "iam", "policies"],
    queryFn: ({ signal }) => api.iam.policies.list(organizationId, signal),
  });
}

export function realtimeServicesQueryOptions(organizationId: string) {
  return queryOptions({
    queryKey: ["organizations", organizationId, "realtime", "services"],
    queryFn: ({ signal }) =>
      api.realtime.services.list(organizationId, undefined, signal),
  });
}

export function realtimeServiceQueryOptions(
  organizationId: string,
  serviceId: string,
) {
  return queryOptions({
    queryKey: [
      "organizations",
      organizationId,
      "realtime",
      "services",
      serviceId,
    ],
    queryFn: ({ signal }) =>
      api.realtime.services.get(organizationId, serviceId, signal),
  });
}

export function realtimeServiceMetricsQueryOptions(
  organizationId: string,
  serviceId: string,
) {
  return queryOptions({
    queryKey: [
      "organizations",
      organizationId,
      "realtime",
      "services",
      serviceId,
      "metrics",
    ],
    queryFn: ({ signal }) =>
      api.realtime.services.metrics(organizationId, serviceId, signal),
    refetchInterval: 15_000,
    staleTime: 5_000,
  });
}

export function realtimeServiceMetricHistoryQueryOptions(
  organizationId: string,
  projectId: string,
  serviceId: string,
  range: RealtimeMetricsRange,
) {
  return queryOptions({
    queryKey: [
      "organizations",
      organizationId,
      "projects",
      projectId,
      "realtime",
      "services",
      serviceId,
      "metrics",
      "history",
      range,
    ],
    queryFn: ({ signal }) =>
      api.realtime.services.metricsHistory(
        organizationId,
        projectId,
        serviceId,
        range,
        signal,
      ),
    refetchInterval: 15_000,
    staleTime: 5_000,
  });
}

export function auditEventsQueryOptions(organizationId: string) {
  return queryOptions({
    queryKey: ["organizations", organizationId, "audit-events"],
    queryFn: ({ signal }) => api.auditEvents.list(organizationId, 500, signal),
  });
}
