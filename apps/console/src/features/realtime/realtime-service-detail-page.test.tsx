import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "@/lib/api-client";
import type { RealtimeService } from "@/lib/api-types";
import { RealtimeServiceDetailPage } from "./realtime-service-detail-page";

vi.mock("@/features/organizations/organization-context", () => ({
  useActiveOrganization: () => ({
    activeOrganization: {
      organization_id: "0198a117-0d8c-70e2-a457-a83c253b9f21",
      organization_slug: "example",
      organization_name: "Example",
      principal_id: "0198a118-073f-79e4-9ca4-0c1c2501c031",
      role: "owner",
    },
    memberships: [],
    setActiveOrganizationId: vi.fn(),
  }),
}));

const service: RealtimeService = {
  id: "0198a121-ffbd-70c2-a3c8-c65516d7b8fb",
  organization_id: "0198a117-0d8c-70e2-a457-a83c253b9f21",
  project_id: "0198a11b-b519-7177-b6fd-bb131b5fb9e6",
  provider: "flow",
  name: "realtime-production",
  generation: 2,
  state: "ready",
  spec: {
    region: "heteronet-global",
    traffic_mode: "forwarded",
    max_participants: 500,
    turn_enabled: true,
    metadata: {},
  },
  status: {},
  created_at: "2026-08-01T08:00:00Z",
  updated_at: "2026-08-01T09:00:00Z",
};

describe("RealtimeServiceDetailPage", () => {
  beforeEach(() => {
    vi.spyOn(api.realtime.services, "get").mockResolvedValue(service);
    vi.spyOn(api.realtime.services, "metrics").mockResolvedValue({
      measured_at: "2026-08-01T09:01:00Z",
      active_rooms: 12,
      concurrent_connections: 48,
      sfu_participants: 40,
      p2p_connections: 8,
      ingress_bytes: 1_250_000,
      egress_bytes: 2_500_000,
      transferred_bytes: 3_750_000,
      room_limit: null,
      endpoints: {
        api: ["https://api.realtime.example.com"],
        signaling: ["wss://signal.realtime.example.com"],
        livekit: ["wss://livekit.realtime.example.com"],
        stun: ["stun:turn.realtime.example.com:3478"],
        turn: ["turns:turn.realtime.example.com:5349"],
      },
    });
    vi.spyOn(api.projects, "list").mockResolvedValue({
      items: [
        {
          id: service.project_id,
          organization_id: service.organization_id,
          slug: "realtime",
          name: "Realtime",
          created_at: "2026-08-01T08:00:00Z",
        },
      ],
    });
  });

  it("実メトリクス、実エンドポイント、無制限ルームと同時参加者上限を表示する", async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
    render(
      <QueryClientProvider client={queryClient}>
        <MemoryRouter
          initialEntries={[
            `/realtime/services/${service.id}`,
          ]}
        >
          <Routes>
            <Route
              path="/realtime/services/:serviceId"
              element={<RealtimeServiceDetailPage />}
            />
          </Routes>
        </MemoryRouter>
      </QueryClientProvider>,
    );

    expect(
      await screen.findByRole("heading", { name: "realtime-production" }),
    ).toBeInTheDocument();
    expect(await screen.findByText("12")).toBeInTheDocument();
    expect(screen.getByText("48")).toBeInTheDocument();
    expect(screen.getByText("1.25 MB")).toBeInTheDocument();
    expect(screen.getByText("2.5 MB")).toBeInTheDocument();
    expect(screen.getByText("3.75 MB")).toBeInTheDocument();
    expect(screen.getByText("無制限")).toBeInTheDocument();
    expect(screen.getByText("500")).toBeInTheDocument();
    expect(
      screen.getByText("wss://livekit.realtime.example.com"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("turns:turn.realtime.example.com:5349"),
    ).toBeInTheDocument();
  });
});
