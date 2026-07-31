import { queryOptions, useQuery } from "@tanstack/react-query";
import { ApiError, api } from "@/lib/api-client";

export const sessionQueryOptions = queryOptions({
  queryKey: ["auth", "session"],
  queryFn: async ({ signal }) => {
    try {
      return await api.auth.session(signal);
    } catch (error) {
      if (error instanceof ApiError && error.status === 401) return null;
      throw error;
    }
  },
  retry: false,
  staleTime: 15_000,
});

export function useSession() {
  return useQuery(sessionQueryOptions);
}
