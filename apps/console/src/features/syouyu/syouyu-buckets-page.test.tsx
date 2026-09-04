import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "@/lib/api-client";
import type { SyouyuBucket, SyouyuQuotaLimits } from "@/lib/api-types";
import { GIBIBYTE } from "./syouyu-utils";
import { SyouyuBucketsPage } from "./syouyu-buckets-page";

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

vi.mock("@/components/shared/resource-selectors", () => ({
  ProjectSelector: ({
    onValueChange,
  }: {
    onValueChange: (value: string) => void;
  }) => (
    <button type="button" onClick={() => onValueChange("project-1")}>
      テストプロジェクトを選択
    </button>
  ),
}));

const quota: SyouyuQuotaLimits = {
  max_buckets: 100,
  max_bytes_per_bucket: 100 * GIBIBYTE,
  max_objects_per_bucket: 10_000_000,
  max_total_bytes: 1_000 * GIBIBYTE,
  max_credentials_per_bucket: 10,
  max_total_credentials: 1_000,
};

const bucket: SyouyuBucket = {
  id: "bucket-1",
  organization_id: "organization-1",
  project_id: "project-1",
  provider: "syouyu",
  name: "assets-prod",
  generation: 1,
  state: "ready",
  spec: {
    region: "heteronet-global",
    bucket_name: "assets-prod",
    quota_bytes: 10 * GIBIBYTE,
    quota_objects: 1_000_000,
    metadata: {},
  },
  status: {
    phase: "ready",
    endpoint: "https://s3.heterocloud.example",
    bytes: 2 * GIBIBYTE,
    objects: 42,
  },
  created_at: "2026-09-04T01:00:00Z",
  updated_at: "2026-09-04T02:00:00Z",
};

function renderPage() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={["/syouyu/buckets"]}>
        <Routes>
          <Route path="/syouyu/buckets" element={<SyouyuBucketsPage />} />
          <Route
            path="/syouyu/buckets/:bucketId"
            element={<div>Syouyuバケット詳細ルート</div>}
          />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

describe("SyouyuBucketsPage", () => {
  beforeEach(() => {
    vi.spyOn(api.syouyu, "quota").mockResolvedValue(quota);
    vi.spyOn(api.syouyu.buckets, "list").mockResolvedValue({ items: [bucket] });
    vi.spyOn(api.projects, "list").mockResolvedValue({
      items: [
        {
          id: "project-1",
          organization_id: "organization-1",
          slug: "storage",
          name: "Storage",
          created_at: "2026-09-04T01:00:00Z",
        },
      ],
    });
  });

  it("バケット一覧の行全体から詳細へ移動できる", async () => {
    const user = userEvent.setup();
    renderPage();

    expect(await screen.findByText("assets-prod")).toBeInTheDocument();
    expect(screen.getByText("https://s3.heterocloud.example")).toBeInTheDocument();
    await user.click(
      screen.getByRole("link", { name: "assets-prodの詳細を開く" }),
    );

    expect(screen.getByText("Syouyuバケット詳細ルート")).toBeInTheDocument();
  });

  it("プロジェクトとクォータを指定して1バケットを作成する", async () => {
    const user = userEvent.setup();
    const create = vi
      .spyOn(api.syouyu.buckets, "create")
      .mockResolvedValue({ ...bucket, id: "bucket-2", name: "logs-prod", spec: { ...bucket.spec, bucket_name: "logs-prod" } });
    renderPage();

    await user.click(await screen.findByRole("button", { name: "バケットを作成" }));
    await user.click(screen.getByRole("button", { name: "テストプロジェクトを選択" }));
    await user.type(screen.getByRole("textbox", { name: "バケット名" }), "logs-prod");
    await user.click(screen.getByRole("button", { name: "作成" }));

    await waitFor(() =>
      expect(create).toHaveBeenCalledWith("organization-1", {
        project_id: "project-1",
        name: "logs-prod",
        spec: {
          region: "heteronet-global",
          bucket_name: "logs-prod",
          quota_bytes: 10 * GIBIBYTE,
          quota_objects: 1_000_000,
          metadata: {},
        },
      }),
    );
    expect(await screen.findByText("Syouyuバケット詳細ルート")).toBeInTheDocument();
  });
});
