import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "@/lib/api-client";
import type { FlashService } from "@/lib/api-types";
import { FlashServiceDetailPage } from "./flash-service-detail-page";

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

const service: FlashService = {
  id: "flash-1",
  organization_id: "organization-1",
  project_id: "project-1",
  provider: "flash",
  name: "game-server",
  generation: 3,
  state: "ready",
  spec: {
    region: "heteronet-global",
    image: "ghcr.io/example/game-server:v1",
    replicas: 3,
    cpu_millis: 1_000,
    memory_mib: 2_048,
    ports: [
      { name: "game", protocol: "udp", container_port: 7777, service_port: 7777 },
    ],
    exposure: { type: "public", traffic_mode: "forwarded" },
    env: { GAME_MODE: "production" },
    command: ["/app/server"],
    args: ["--listen", "0.0.0.0:7777"],
    metadata: {},
  },
  status: {
    runtime_class: "gvisor",
    ready_replicas: 2,
    endpoints: [
      { name: "game", protocol: "udp", host: "203.0.113.10", port: 7777 },
    ],
  },
  created_at: "2026-08-21T08:00:00Z",
  updated_at: "2026-08-21T09:00:00Z",
};

describe("FlashServiceDetailPage", () => {
  beforeEach(() => {
    vi.spyOn(api.flash.services, "get").mockResolvedValue(service);
    vi.spyOn(api.projects, "list").mockResolvedValue({
      items: [
        {
          id: "project-1",
          organization_id: "organization-1",
          slug: "games",
          name: "Games",
          created_at: "2026-08-21T08:00:00Z",
        },
      ],
    });
  });

  it("状態、レプリカ、UDPエンドポイントを表示して編集を開く", async () => {
    const user = userEvent.setup();
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
    render(
      <QueryClientProvider client={queryClient}>
        <MemoryRouter initialEntries={[`/flash/services/${service.id}`]}>
          <Routes>
            <Route path="/flash/services/:serviceId" element={<FlashServiceDetailPage />} />
          </Routes>
        </MemoryRouter>
      </QueryClientProvider>,
    );

    expect(await screen.findByRole("heading", { name: "game-server" })).toBeInTheDocument();
    expect(screen.queryByText(/gVisor/)).not.toBeInTheDocument();
    expect(screen.getByText("203.0.113.10:7777")).toBeInTheDocument();
    expect(screen.getByText("公開・転送")).toBeInTheDocument();
    expect(screen.getByText("GAME_MODE")).toBeInTheDocument();
    expect(screen.getAllByText("2").length).toBeGreaterThan(0);
    expect(screen.getAllByText("3").length).toBeGreaterThan(0);

    await user.click(screen.getByRole("button", { name: "編集" }));
    expect(screen.getByRole("dialog", { name: "Flashサービスを編集" })).toBeInTheDocument();
    expect(screen.queryByText(/gVisor/)).not.toBeInTheDocument();
    expect(screen.getByDisplayValue("ghcr.io/example/game-server:v1")).toBeInTheDocument();
  });
});
