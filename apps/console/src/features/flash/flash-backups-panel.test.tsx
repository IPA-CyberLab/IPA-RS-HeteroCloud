import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "@/lib/api-client";
import type { FlashBackup } from "@/lib/api-types";
import { FlashBackupsPanel } from "./flash-backups-panel";

const backup: FlashBackup = { id: "backup-1", organization_id: "org-1", project_id: "project-1", service_instance_id: "source-1", source_name: "source", name: "before-update", volume_size_bytes: 1024 ** 3, phase: "ready", status: {}, delete_requested: false, created_at: "2026-10-19T00:00:00Z", updated_at: "2026-10-19T00:00:01Z" };
function show(serviceId?: string) {
  return render(<QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })}>
    <MemoryRouter><Routes>
      <Route path="/" element={<FlashBackupsPanel organizationId="org-1" serviceId={serviceId} />} />
      <Route path="/flash/services/:id" element={<div>復元先のサービス</div>} />
    </Routes></MemoryRouter>
  </QueryClientProvider>);
}
describe("FlashBackupsPanel", () => {
  beforeEach(() => {
    vi.spyOn(api.flash.backups, "list").mockResolvedValue({ items: [backup] });
    vi.spyOn(api.projects, "list").mockResolvedValue({ items: [{ id: "project-1", organization_id: "org-1", slug: "test", name: "Test", created_at: "2026-10-19T00:00:00Z" }] });
  });
  it("復元先を選んでから復元を要求し新サービスへ移動する", async () => {
    const restore = vi.spyOn(api.flash.backups, "restore").mockResolvedValue({ id: "restored-1" } as Awaited<ReturnType<typeof api.flash.backups.restore>>);
    show();
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: "復元" }));
    expect(restore).not.toHaveBeenCalled();
    expect(screen.getByText(/停止状態の非公開サービス/)).toBeInTheDocument();
    await user.clear(screen.getByRole("textbox", { name: "復元先サービス名" }));
    await user.type(screen.getByRole("textbox", { name: "復元先サービス名" }), "recovered");
    await user.click(screen.getByRole("button", { name: "新しいサービスへ復元" }));
    await screen.findByText("復元先のサービス");
    expect(restore).toHaveBeenCalledWith("org-1", "backup-1", { name: "recovered", project_id: "project-1", request_id: expect.stringMatching(/^[0-9a-f-]{36}$/) });
  });
  it("保存中は復元できず削除にも確認が必要", async () => {
    vi.mocked(api.flash.backups.list).mockResolvedValue({ items: [{ ...backup, phase: "creating" }] });
    const remove=vi.spyOn(api.flash.backups,"delete").mockResolvedValue({ ...backup, phase: "deleting", delete_requested: true });
    show();
    const user=userEvent.setup();
    expect(await screen.findByRole("button", { name: "復元" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "削除" }));
    expect(remove).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "バックアップを削除" }));
    await waitFor(() => expect(remove).toHaveBeenCalledWith("org-1", "backup-1"));
  });
  it("失敗の再送では同じ作成リクエストIDを使う", async () => {
    const create=vi.spyOn(api.flash.backups,"create").mockRejectedValue(new Error("temporarily unavailable"));
    show("source-1");
    const user=userEvent.setup();
    await user.type(screen.getByRole("textbox", { name: "バックアップ名" }),"safe-point");
    await user.click(screen.getByRole("button", { name: "バックアップを作成" }));
    await waitFor(() => expect(create).toHaveBeenCalledTimes(1));
    await user.click(screen.getByRole("button", { name: "バックアップを作成" }));
    await waitFor(() => expect(create).toHaveBeenCalledTimes(2));
    expect(create.mock.calls[0]).toEqual(create.mock.calls[1]);
    expect(create.mock.calls[0]?.slice(0,3)).toEqual(["org-1", "source-1", "safe-point"]);
  });
  it("取得失敗を空の一覧として表示しない", async () => {
    vi.mocked(api.flash.backups.list).mockRejectedValue(new Error("unavailable"));
    show();
    await screen.findByText("予期しないエラーが発生しました。");
    expect(screen.queryByText("バックアップはありません")).not.toBeInTheDocument();
  });
  it("保存先に接続できない場合は最終取得状態と明示する", async () => {
    vi.mocked(api.flash.backups.list).mockResolvedValue({ items: [{ ...backup, status: { provider_unavailable: true } }] });
    show();
    await screen.findByText(/表示は最後に取得した状態です/);
  });
});
