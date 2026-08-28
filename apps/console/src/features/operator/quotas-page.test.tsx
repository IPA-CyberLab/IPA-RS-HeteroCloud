import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "@/lib/api-client";
import type { ResourceQuotaLimits } from "@/lib/api-types";
import { OwnerQuotasPage, normalizeResourceQuotaLimits } from "./quotas-page";

const limits: ResourceQuotaLimits = {
  flow: {
    max_services: 100,
    max_rooms_per_service: 10_000,
    max_total_rooms: 10_000,
    max_participants_per_service: 100_000,
    max_rate_limit_requests_per_second: 1_000,
    max_rate_limit_burst: 2_000,
    max_developer_credentials_per_service: 100,
  },
  flash: {
    max_services: 100,
    max_replicas_per_service: 100,
    max_cpu_millis_per_vm: 4_000,
    max_memory_mib_per_vm: 8_128,
    max_disk_gib_per_vm: 10,
    max_total_replicas: 100,
    max_total_cpu_millis: 20_000,
    max_total_memory_mib: 32_768,
    max_total_disk_gib: 100,
  },
  registry: { storage_gib: 10, max_credentials: 10 },
};

it("API安全上限を超える古いハードリミットを保存前に正規化する", () => {
  const staleLimits = structuredClone(limits);
  staleLimits.flash.max_disk_gib_per_vm = 20;
  staleLimits.flash.max_total_disk_gib = 5;
  staleLimits.flow.max_rate_limit_burst = 100;

  const normalized = normalizeResourceQuotaLimits(staleLimits);

  expect(normalized.flash.max_disk_gib_per_vm).toBe(10);
  expect(normalized.flash.max_total_disk_gib).toBe(10);
  expect(normalized.flow.max_rate_limit_burst).toBe(1_000);
});

describe("OwnerQuotasPage", () => {
  beforeEach(() => {
    vi.spyOn(api.owner.accounts, "list").mockResolvedValue({
      items: [
        {
          user: {
            id: "0198a3be-b69a-7b37-9ff2-934b8907685b",
            email: "user@example.test",
            display_name: "Example User",
            status: "active",
            created_at: "2026-08-27T00:00:00Z",
          },
          has_local_password: false,
          external_identities: [
            {
              issuer: "https://idp.example.test/realms/heterocloud",
              subject: "keycloak-subject-1",
              created_at: "2026-08-27T00:00:00Z",
            },
          ],
          memberships: [
            {
              organization_id: "0198a3be-b69a-7b37-9ff2-934b8907685a",
              organization_slug: "user-example",
              organization_name: "Example account",
              principal_id: "0198a3be-b69a-7b37-9ff2-934b8907685c",
              role: "owner",
            },
          ],
          last_login: {
            id: 1,
            user_id: "0198a3be-b69a-7b37-9ff2-934b8907685b",
            source_ip: "203.0.113.42",
            authentication_method: "oidc",
            occurred_at: "2026-08-28T01:00:00Z",
          },
          login_count: 1,
        },
      ],
    });
    vi.spyOn(api.owner.accounts, "logins").mockResolvedValue({
      items: [
        {
          id: 1,
          user_id: "0198a3be-b69a-7b37-9ff2-934b8907685b",
          source_ip: "203.0.113.42",
          authentication_method: "oidc",
          occurred_at: "2026-08-28T01:00:00Z",
        },
      ],
    });
    vi.spyOn(api.owner.quotas, "overview").mockResolvedValue({
      defaults: structuredClone(limits),
      tenants: [
        {
          organization: {
            id: "0198a3be-b69a-7b37-9ff2-934b8907685a",
            slug: "user-example",
            name: "Example account",
            created_at: "2026-08-28T00:00:00Z",
          },
          override_limits: null,
          effective_limits: structuredClone(limits),
          usage: {
            flow_services: 2,
            flow_configured_rooms: 20,
            flash_services: 3,
            flash_replicas: 5,
            flash_cpu_millis: 5_000,
            flash_memory_mib: 10_240,
            flash_disk_gib: 25,
          },
        },
      ],
    });
    vi.spyOn(api.owner.quotas, "updateDefaults").mockImplementation(
      async (next) => next,
    );
    vi.spyOn(api.owner.quotas, "updateOrganization").mockImplementation(
      async (_id, next) => next,
    );
    vi.spyOn(api.owner.quotas, "clearOrganization").mockResolvedValue(
      structuredClone(limits),
    );
  });

  it("サービス全体と全アカウントの制限を表示して既定値を保存する", async () => {
    const user = userEvent.setup();
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
    render(
      <QueryClientProvider client={queryClient}>
        <OwnerQuotasPage />
      </QueryClientProvider>,
    );

    expect(
      await screen.findByRole("heading", { name: "全アカウント管理" }),
    ).toBeInTheDocument();
    expect(screen.getAllByText("Example account").length).toBeGreaterThan(0);
    expect(screen.getByText("203.0.113.42")).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "全アカウントの既定ハードリミット" }),
    ).toBeInTheDocument();

    await user.click(
      screen.getByRole("link", { name: "Example Userの登録情報を表示" }),
    );
    expect(
      await screen.findByRole("heading", { name: "Example User のアカウント情報" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Subject: keycloak-subject-1")).toBeInTheDocument();
    expect(api.owner.accounts.logins).toHaveBeenCalledWith(
      "0198a3be-b69a-7b37-9ff2-934b8907685b",
      100,
      expect.any(AbortSignal),
    );

    await user.click(screen.getByRole("button", { name: "既定値を保存" }));
    await waitFor(() => {
      expect(api.owner.quotas.updateDefaults).toHaveBeenCalledWith(limits);
    });
  });
});
