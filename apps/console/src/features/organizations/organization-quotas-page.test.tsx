import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "@/lib/api-client";
import type { OrganizationMemberQuota, ResourceQuotaLimits, ResourceQuotaTenant } from "@/lib/api-types";
import { OrganizationQuotasPage } from "./organization-quotas-page";

const organization = { organization_id: "org-1", organization_name: "Example org", role: "member" };
vi.mock("@/features/organizations/organization-context", () => ({
  useActiveOrganization: () => ({ activeOrganization: organization }),
}));
const limits: ResourceQuotaLimits = {
  flow: { max_services: 5, max_rooms_per_service: 10, max_total_rooms: 50, max_participants_per_service: 20,
    max_rate_limit_requests_per_second: 1, max_rate_limit_burst: 1, max_developer_credentials_per_service: 3 },
  flash: { max_services: 5, max_replicas_per_service: 1, max_total_replicas: 5, max_cpu_millis_per_vm: 1000,
    max_total_cpu_millis: 5000, max_memory_mib_per_vm: 512, max_total_memory_mib: 2560,
    max_disk_gib_per_vm: 1, max_total_disk_gib: 5, max_weekly_cpu_millicore_seconds: 10000,
    max_weekly_memory_mib_seconds: 20000, max_weekly_gpu_seconds: 300 },
  registry: { storage_gib: 10, max_credentials: 5 },
  syouyu: { max_buckets: 5, max_bytes_per_bucket: 1048576, max_total_bytes: 5242880,
    max_objects_per_bucket: 100, max_credentials_per_bucket: 2, max_total_credentials: 10 },
};
const member = { user_id: "user-1", principal_id: "p-1", display_name: "Member One", email: "one@example.test",
  role: "member", override_limits: null, effective_limits: limits, usage: {} } as OrganizationMemberQuota;
const tenant = { organization: { id: "org-1", name: "Example org", slug: "example", created_at: "2026-10-30" },
  effective_limits: limits, override_limits: null, usage: {} } as ResourceQuotaTenant;

function show() {
  return render(<QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
    <OrganizationQuotasPage />
  </QueryClientProvider>);
}

describe("organization quotas", () => {
  beforeEach(() => { organization.role = "member"; vi.restoreAllMocks(); });
  it("shows only a member's own limits and marks organization shared resources", async () => {
    vi.spyOn(api.organizations, "quotas").mockResolvedValue({ organization: null, member, members: [] });
    show();
    expect(await screen.findByRole("heading", { name: "自分の上限" })).toBeInTheDocument();
    expect(screen.getByText("保存容量 (GiB)（組織共通）")).toBeInTheDocument();
    expect(screen.queryByText("メンバーの上限")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "上限を変更" })).not.toBeInTheDocument();
  });
  it("allows an admin to edit a member allocation", async () => {
    organization.role = "admin";
    vi.spyOn(api.organizations, "quotas").mockResolvedValue({ organization: tenant, member, members: [member] });
    vi.spyOn(api.organizations, "unassignedQuotaResources").mockResolvedValue({ items: [] });
    const update = vi.spyOn(api.organizations, "updateMemberQuota").mockResolvedValue(limits);
    show();
    const user = userEvent.setup();
    await user.click(await screen.findByText(/Member One · one@example.test/));
    await user.click(screen.getByRole("button", { name: "上限を変更" }));
    expect(screen.getByText("Flash Registry の保存容量と認証情報数は組織で共有されます。")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => expect(update).toHaveBeenCalledWith("org-1", "user-1", limits));
  });
});
