import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "@/lib/api-client";
import type {
  SyouyuBucket,
  SyouyuCredential,
  SyouyuCredentialSecret,
  SyouyuQuotaLimits,
} from "@/lib/api-types";
import { SyouyuBucketDetailPage } from "./syouyu-bucket-detail-page";
import { GIBIBYTE } from "./syouyu-utils";

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
  },
  created_at: "2026-09-04T01:00:00Z",
  updated_at: "2026-09-04T02:00:00Z",
};

const credential: SyouyuCredential = {
  id: "credential-1",
  service_instance_id: bucket.id,
  name: "production-backend",
  access_key_id: "HKCEXAMPLEKEY",
  permissions: ["read", "write"],
  status: "active",
  created_at: "2026-09-04T03:00:00Z",
  revoked_at: null,
};

const secret: SyouyuCredentialSecret = {
  credential: { ...credential, id: "credential-2", name: "ue5-client" },
  secret_access_key: "one-time-secret-value",
  endpoint: "https://s3.heterocloud.example",
  region: "heteronet-global",
  bucket: "assets-prod",
};

function renderPage() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={[`/syouyu/buckets/${bucket.id}`]}>
        <Routes>
          <Route
            path="/syouyu/buckets/:bucketId"
            element={<SyouyuBucketDetailPage />}
          />
          <Route path="/syouyu/buckets" element={<div>Syouyu一覧ルート</div>} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

describe("SyouyuBucketDetailPage", () => {
  beforeEach(() => {
    vi.spyOn(api.syouyu, "quota").mockResolvedValue(quota);
    vi.spyOn(api.syouyu.buckets, "get").mockResolvedValue(bucket);
    vi.spyOn(api.syouyu.buckets, "usage").mockResolvedValue({
      quota_bytes: 10 * GIBIBYTE,
      quota_objects: 1_000_000,
      used_bytes: 2 * GIBIBYTE,
      object_count: 42,
      unfinished_upload_bytes: 128 * 1024,
      credential_count: 1,
    });
    vi.spyOn(api.syouyu.buckets.credentials, "list").mockResolvedValue({
      items: [credential],
    });
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

  it("エンドポイント、リージョン、クォータ、実使用量を表示して編集する", async () => {
    const user = userEvent.setup();
    const update = vi
      .spyOn(api.syouyu.buckets, "update")
      .mockResolvedValue({
        ...bucket,
        spec: { ...bucket.spec, quota_bytes: 20 * GIBIBYTE, quota_objects: 2_000_000 },
      });
    renderPage();

    expect(await screen.findByRole("heading", { name: "assets-prod" })).toBeInTheDocument();
    expect(screen.getByText("https://s3.heterocloud.example")).toBeInTheDocument();
    expect(screen.getByText("heteronet-global")).toBeInTheDocument();
    expect((await screen.findAllByText("2 GiB")).length).toBeGreaterThan(0);
    expect(screen.getAllByText("42").length).toBeGreaterThan(0);

    await user.click(screen.getByRole("button", { name: "クォータを編集" }));
    const bytes = screen.getByRole("spinbutton", { name: "容量上限 (GiB)" });
    const objects = screen.getByRole("spinbutton", { name: "オブジェクト数上限" });
    await user.clear(bytes);
    await user.type(bytes, "20");
    await user.clear(objects);
    await user.type(objects, "2000000");
    await user.click(screen.getByRole("button", { name: "変更を保存" }));

    await waitFor(() =>
      expect(update).toHaveBeenCalledWith("organization-1", "bucket-1", {
        name: "assets-prod",
        spec: {
          ...bucket.spec,
          quota_bytes: 20 * GIBIBYTE,
          quota_objects: 2_000_000,
        },
      }),
    );
  });

  it("発行したシークレットを一度だけ表示して閉じると破棄する", async () => {
    const user = userEvent.setup();
    const create = vi
      .spyOn(api.syouyu.buckets.credentials, "create")
      .mockResolvedValue(secret);
    renderPage();

    await user.click(await screen.findByRole("button", { name: "認証情報を発行" }));
    await user.type(screen.getByRole("textbox", { name: "名前" }), "ue5-client");
    await user.click(screen.getByRole("button", { name: "発行" }));

    expect(await screen.findByText("one-time-secret-value")).toBeInTheDocument();
    expect(create).toHaveBeenCalledWith("organization-1", "bucket-1", {
      name: "ue5-client",
      permissions: ["read", "write"],
    });
    await user.click(screen.getByRole("button", { name: "閉じる" }));
    await waitFor(() =>
      expect(screen.queryByText("one-time-secret-value")).not.toBeInTheDocument(),
    );
    await user.click(screen.getByRole("button", { name: "認証情報を発行" }));
    expect(screen.queryByText("one-time-secret-value")).not.toBeInTheDocument();
  });

  it("アクセス認証情報を確認付きで失効する", async () => {
    const user = userEvent.setup();
    const revoke = vi
      .spyOn(api.syouyu.buckets.credentials, "revoke")
      .mockResolvedValue(undefined);
    renderPage();

    await user.click(
      await screen.findByRole("button", { name: "production-backendを失効" }),
    );
    expect(
      screen.getByRole("heading", { name: "アクセス認証情報を失効" }),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "失効する" }));

    await waitFor(() =>
      expect(revoke).toHaveBeenCalledWith(
        "organization-1",
        "bucket-1",
        "credential-1",
      ),
    );
  });

  it("バケット名の入力確認後に空バケットを削除する", async () => {
    const user = userEvent.setup();
    const remove = vi
      .spyOn(api.syouyu.buckets, "delete")
      .mockResolvedValue(bucket);
    renderPage();

    await user.click(await screen.findByRole("button", { name: "削除" }));
    const confirmation = screen.getByRole("textbox", { name: "確認" });
    expect(screen.getByRole("button", { name: "削除する" })).toBeDisabled();
    await user.type(confirmation, "assets-prod");
    await user.click(screen.getByRole("button", { name: "削除する" }));

    await waitFor(() =>
      expect(remove).toHaveBeenCalledWith("organization-1", "bucket-1"),
    );
    expect(await screen.findByText("Syouyu一覧ルート")).toBeInTheDocument();
  });
});
