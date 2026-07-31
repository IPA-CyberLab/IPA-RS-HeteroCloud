import { queryOptions } from "@tanstack/react-query";
import { api } from "@/lib/api-client";

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

export function flowInstancesQueryOptions(organizationId: string) {
  return queryOptions({
    queryKey: ["organizations", organizationId, "flow", "instances"],
    queryFn: ({ signal }) =>
      api.flow.instances.list(organizationId, undefined, signal),
  });
}

export function auditEventsQueryOptions(organizationId: string) {
  return queryOptions({
    queryKey: ["organizations", organizationId, "audit-events"],
    queryFn: ({ signal }) => api.auditEvents.list(organizationId, 500, signal),
  });
}
