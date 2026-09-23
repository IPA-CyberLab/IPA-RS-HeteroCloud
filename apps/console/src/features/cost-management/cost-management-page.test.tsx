import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "@/lib/api-client";
import type { FlashCostManagement, OwnerFlashCostManagement } from "@/lib/api-types";
import { CostManagementPage, OwnerCostManagementPage } from "./cost-management-page";

vi.mock("@/features/organizations/organization-context", () => ({
  useActiveOrganization: () => ({
    activeOrganization: {
      organization_id: "organization-1",
      organization_slug: "example",
      organization_name: "Example",
      principal_id: "principal-1",
      role: "owner",
    },
  }),
}));

const limits = {
  max_services: 100,
  max_replicas_per_service: 100,
  max_cpu_millis_per_vm: 4_000,
  max_memory_mib_per_vm: 8_128,
  max_disk_gib_per_vm: 10,
  max_total_replicas: 100,
  max_total_cpu_millis: 20_000,
  max_total_memory_mib: 32_768,
  max_total_disk_gib: 100,
  max_weekly_cpu_millicore_seconds: 3_245_760_000,
  max_weekly_memory_mib_seconds: 2_836_280_317,
  max_weekly_gpu_seconds: 40_320,
};

const accountUsage: FlashCostManagement = {
  generated_at: 345_720,
  week_started_at: 345_600,
  week_ends_at: 950_400,
  limits,
  usage: {
    cpu_millicore_seconds: 3_600_000,
    memory_mib_seconds: 3_686_400,
    gpu_seconds: 3_600,
  },
  current: {
    active_services: 1,
    ready_replicas: 2,
    cpu_millis: 1_000,
    memory_mib: 2_048,
    gpus: 0,
  },
  services: [
    {
      organization_id: "organization-1",
      project_id: "project-1",
      service_instance_id: "service-1",
      display_name: "deleted-worker",
      active: false,
      ready_replicas: 0,
      cpu_millis: 500,
      memory_mib: 1_024,
      gpu_count: 0,
      weekly_usage: {
        week_started_at: 345_600,
        cpu_millicore_seconds: 3_600_000,
        memory_mib_seconds: 3_686_400,
        gpu_seconds: 0,
        last_metered_at: 345_700,
        max_cpu_millicore_seconds: limits.max_weekly_cpu_millicore_seconds,
        max_memory_mib_seconds: limits.max_weekly_memory_mib_seconds,
        max_gpu_seconds: limits.max_weekly_gpu_seconds,
      },
    },
  ],
};

function renderPage(page: ReactNode) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter>{page}</MemoryRouter>
    </QueryClientProvider>,
  );
}

describe("cost management pages", () => {
  beforeEach(() => {
    vi.spyOn(api.flash, "usage").mockResolvedValue(accountUsage);
    vi.spyOn(api.owner, "costManagement").mockResolvedValue({
      generated_at: accountUsage.generated_at,
      week_started_at: accountUsage.week_started_at,
      week_ends_at: accountUsage.week_ends_at,
      usage: accountUsage.usage,
      current: accountUsage.current,
      tenants: [
        {
          organization: {
            id: "organization-1",
            slug: "example",
            name: "Example account",
            created_at: "2026-09-01T00:00:00Z",
          },
          limits,
          usage: accountUsage.usage,
          current: accountUsage.current,
          services: accountUsage.services,
        },
      ],
    } satisfies OwnerFlashCostManagement);
  });

  it("shows account totals and retains deleted service usage", async () => {
    renderPage(<CostManagementPage />);

    expect(await screen.findByRole("heading", { name: "コスト管理" })).toBeInTheDocument();
    expect(screen.getAllByText("1 vCPU 時間").length).toBeGreaterThan(0);
    expect(screen.getAllByText("1 GiB 時間").length).toBeGreaterThan(0);
    expect(screen.getByText(/1 GPU 時間/)).toBeInTheDocument();
    expect(screen.getByText("deleted-worker")).toBeInTheDocument();
    expect(screen.getByText("今週削除済み")).toBeInTheDocument();
    expect(api.flash.usage).toHaveBeenCalledWith(
      "organization-1",
      expect.any(AbortSignal),
    );
  });

  it("shows every account to the system owner", async () => {
    renderPage(<OwnerCostManagementPage />);

    expect(await screen.findByText("Example account")).toBeInTheDocument();
    expect(screen.getAllByText("1 vCPU 時間").length).toBeGreaterThan(0);
    expect(api.owner.costManagement).toHaveBeenCalledWith(expect.any(AbortSignal));
  });
});
