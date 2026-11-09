import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactNode } from "react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "@/lib/api-client";
import type { FlashCostManagement, OwnerFlashCostManagement } from "@/lib/api-types";
import { CostManagementPage, OwnerCostManagementPage } from "./cost-management-page";
import { toMicrounits } from "./flash-billing-panels";

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
  can_manage_budget: true,
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
  estimate: {
    rate_card: { effective_week_start: 345_600, currency: "USD", cpu_microunits_per_vcpu_hour: 1_000_000,
      memory_microunits_per_gib_hour: 2_000_000, gpu_microunits_per_gpu_hour: 3_000_000 },
    budget: { organization_id: "organization-1", currency: "USD", amount_microunits: 7_500_000, warning_percentage: 80 },
    amount_microunits: 6_000_000, warning_reached: true,
  },
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
    vi.spyOn(api.owner.flashRateCards, "list").mockResolvedValue({items: []});
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

  it("shows the budget warning and saves an exact decimal budget", async () => {
    const user = userEvent.setup();
    const save = vi.spyOn(api.flash.weeklyBudget, "set").mockResolvedValue({
      organization_id: "organization-1", currency: "USD", amount_microunits: 12_340_001, warning_percentage: 80,
    });
    renderPage(<CostManagementPage />);
    expect(await screen.findByText(/週間予算の警告率に達しました/)).toBeInTheDocument();
    const input = screen.getByLabelText("週間予算");
    await user.clear(input);
    await user.type(input, "12.340001");
    await user.click(screen.getByRole("button", {name: "予算を保存"}));
    expect(save).toHaveBeenCalledWith("organization-1", {
      currency: "USD", amount_microunits: 12_340_001, warning_percentage: 80,
    });
  });

  it("converts currency amounts without rounding or accepting excess precision", () => {
    expect(toMicrounits("12.340001")).toBe(12_340_001);
    expect(toMicrounits("0.000001")).toBe(1);
    expect(toMicrounits("0.0000001")).toBeNull();
    expect(toMicrounits("-1")).toBeNull();
  });

  it("hides budget editing when the server denies permission", async () => {
    vi.mocked(api.flash.usage).mockResolvedValue({ ...accountUsage, can_manage_budget: false });
    renderPage(<CostManagementPage />);
    expect(await screen.findByText(/週間予算の警告率に達しました/)).toBeInTheDocument();
    expect(screen.queryByRole("button", {name: "予算を保存"})).not.toBeInTheDocument();
  });
});
