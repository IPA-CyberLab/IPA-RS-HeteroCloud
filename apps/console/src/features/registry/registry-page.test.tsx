import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "@/lib/api-client";
import { RegistryPage } from "./registry-page";

vi.mock("@/features/organizations/organization-context", () => ({
  useActiveOrganization: () => ({
    activeOrganization: {
      organization_id: "organization-1",
      organization_slug: "example",
      organization_name: "Example",
      principal_id: "principal-1",
      role: "owner",
    },
    memberships: [],
    setActiveOrganizationId: vi.fn(),
  }),
}));

describe("RegistryPage", () => {
  beforeEach(() => {
    vi.spyOn(api.registry, "get").mockResolvedValue({
      endpoint: "https://registry.example.com",
      project: "hc-organization-1",
      image_prefix: "registry.example.com/hc-organization-1",
      storage_limit_bytes: 10 * 1024 * 1024 * 1024,
      storage_used_bytes: 512 * 1024 * 1024,
      max_credentials: 5,
      credentials: [],
    });
    vi.spyOn(api.registry, "listImages").mockResolvedValue({
      items: [
        {
          reference: "registry.example.com/hc-organization-1/game-server:latest",
          repository: "game-server",
          tag: "latest",
          digest: `sha256:${"a".repeat(64)}`,
          size_bytes: 128 * 1024 * 1024,
          pushed_at: "2026-08-22T05:00:00Z",
        },
      ],
    });
    vi.spyOn(api.registry, "deleteImage").mockResolvedValue(undefined);
  });

  it("確認後に選択したコンテナイメージを削除する", async () => {
    const user = userEvent.setup();
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
    render(
      <QueryClientProvider client={queryClient}>
        <RegistryPage />
      </QueryClientProvider>,
    );

    await user.click(
      await screen.findByRole("button", { name: "game-server:latestを削除" }),
    );
    expect(
      screen.getByRole("dialog", { name: "コンテナイメージを削除" }),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "削除" }));

    await waitFor(() => {
      expect(api.registry.deleteImage).toHaveBeenCalledWith(
        "organization-1",
        "game-server",
        `sha256:${"a".repeat(64)}`,
      );
    });
  });
});
