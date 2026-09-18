import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "@/lib/api-client";
import { OwnerGpusPage } from "./gpus-page";

const userId = "0198a3be-b69a-7b37-9ff2-934b8907685b";
const inactiveUserId = "0198a3be-b69a-7b37-9ff2-934b8907685d";
const gpu = {
  id: "0198a3be-b69a-7b37-9ff2-934b89076899",
  management_id: "uc-k8sp5/gpu-0",
  gpu_type: "nvidia-geforce-gtx-1080-ti",
  display_name: "NVIDIA GeForce GTX 1080 Ti",
  available: true,
  visibility: "private" as const,
  assigned_user_ids: [userId],
  created_at: "2026-09-18T00:00:00Z",
  updated_at: "2026-09-18T01:00:00Z",
};

describe("OwnerGpusPage", () => {
  beforeEach(() => {
    vi.spyOn(api.owner.gpus, "list").mockResolvedValue({ items: [gpu] });
    vi.spyOn(api.owner.gpus, "update").mockImplementation(async (_id, input) => ({
      ...gpu,
      visibility: input.visibility,
      assigned_user_ids: input.assigned_user_ids,
    }));
    vi.spyOn(api.owner.accounts, "list").mockResolvedValue({
      items: [
        {
          user: {
            id: userId,
            email: "gpu-user@example.test",
            display_name: "GPU User",
            status: "active",
            created_at: "2026-09-17T00:00:00Z",
          },
          has_local_password: false,
          external_identities: [],
          memberships: [],
          last_login: null,
          login_count: 0,
        },
        {
          user: {
            id: inactiveUserId,
            email: "inactive@example.test",
            display_name: "Inactive User",
            status: "suspended",
            created_at: "2026-09-16T00:00:00Z",
          },
          has_local_password: false,
          external_identities: [],
          memberships: [],
          last_login: null,
          login_count: 0,
        },
      ],
    });
  });

  it("provider由来のGPU identityを表示し、Openへ変更すると割当を空にする", async () => {
    const user = userEvent.setup();
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
    render(
      <QueryClientProvider client={queryClient}>
        <OwnerGpusPage />
      </QueryClientProvider>,
    );

    expect(await screen.findByRole("heading", { name: "GPU管理" })).toBeInTheDocument();
    expect(screen.getByText("uc-k8sp5/gpu-0")).toBeInTheDocument();
    expect(screen.getByText("利用可能")).toBeInTheDocument();
    expect(screen.getByText("GPU User")).toBeInTheDocument();

    await user.click(
      screen.getByRole("button", {
        name: "NVIDIA GeForce GTX 1080 Tiのアクセス設定",
      }),
    );
    await user.click(screen.getByText("Open", { selector: "span" }));
    await user.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() =>
      expect(api.owner.gpus.update).toHaveBeenCalledWith(gpu.id, {
        visibility: "open",
        assigned_user_ids: [],
      }),
    );
  });

  it("Privateをユーザー未割り当ての隔離状態で保存できる", async () => {
    vi.mocked(api.owner.gpus.list).mockResolvedValue({
      items: [{ ...gpu, assigned_user_ids: [] }],
    });
    const user = userEvent.setup();
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
    render(
      <QueryClientProvider client={queryClient}>
        <OwnerGpusPage />
      </QueryClientProvider>,
    );

    await user.click(
      await screen.findByRole("button", {
        name: "NVIDIA GeForce GTX 1080 Tiのアクセス設定",
      }),
    );
    expect(screen.getByRole("button", { name: "保存" })).toBeEnabled();
    expect(
      screen.getByText(/未割り当てのまま保存すると/),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() =>
      expect(api.owner.gpus.update).toHaveBeenCalledWith(gpu.id, {
        visibility: "private",
        assigned_user_ids: [],
      }),
    );
  });

  it("無効ユーザーの既存割り当てを表示し、再保存時に除去する", async () => {
    vi.mocked(api.owner.gpus.list).mockResolvedValue({
      items: [{ ...gpu, assigned_user_ids: [inactiveUserId] }],
    });
    const user = userEvent.setup();
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
    render(
      <QueryClientProvider client={queryClient}>
        <OwnerGpusPage />
      </QueryClientProvider>,
    );

    expect(await screen.findByText("Inactive User（無効）")).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", {
        name: "NVIDIA GeForce GTX 1080 Tiのアクセス設定",
      }),
    );
    expect(screen.queryByText("Inactive User", { exact: true })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() =>
      expect(api.owner.gpus.update).toHaveBeenCalledWith(gpu.id, {
        visibility: "private",
        assigned_user_ids: [],
      }),
    );
  });
});
