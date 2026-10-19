import { expect, test } from "@playwright/test";

test("a user restores a retained backup into a separate service", async ({ page }) => {
  const time = "2026-10-19T00:00:00Z";
  const errors: string[] = [];
  const actions: string[] = [];
  const backup = { id: "backup-1", organization_id: "org-1", project_id: "project-1", service_instance_id: "deleted-source", source_name: "old-workspace", name: "before-update", phase: "ready", status: {}, delete_requested: false, volume_size_bytes: 1024 ** 3, created_at: time, updated_at: time };
  const service = { id: "restored-1", organization_id: "org-1", project_id: "project-1", provider: "flash", name: "recovered", generation: 1, state: "ready", created_at: time, updated_at: time,
    spec: { region: "test", image: "example/workspace:v1", replicas: 1, stopped: true, restore_from_backup_id: "backup-1", cpu_millis: 100, memory_mib: 128, ephemeral_storage_gib: 2, env: {}, command: [], args: [], metadata: {}, ports: [], exposure: { type: "internal", traffic_mode: "forwarded" }, egress: { mode: "disabled", allow_same_organization: false, allowed_destination_cidrs: [], denied_destination_cidrs: [] } },
    status: { status: { stopped: true, ready_replicas: 0, desired_replicas: 0, runtime_class: "gvisor" } } };
  page.on("pageerror", error => errors.push(error.message));
  await page.route("**/api/v1/**", async route => {
    const request = route.request();
    const path = new URL(request.url()).pathname;
    let body: unknown;
    if (path.endsWith("/auth/session")) body = { user: { id: "user-1", email: "test@example.test", display_name: "Test", status: "active", created_at: time }, memberships: [{ organization_id: "org-1", organization_slug: "test", organization_name: "Test", principal_id: "principal-1", role: "owner" }], csrf_token: "backup-test-csrf" };
    else if (path.endsWith("/projects")) body = { items: [{ id: "project-1", organization_id: "org-1", name: "Test", slug: "test", created_at: time }] };
    else if (path.endsWith("/flash/backups/backup-1/restore")) {
      expect(request.method()).toBe("POST");
      expect(request.headers()["x-heterocloud-csrf"]).toBe("backup-test-csrf");
      expect(request.postDataJSON()).toEqual({ name: "recovered", project_id: "project-1", request_id: expect.stringMatching(/^[0-9a-f-]{36}$/) });
      actions.push("restore-new-service"); body = service;
    } else if (path.endsWith("/flash/backups")) body = { items: [backup] };
    else if (path.endsWith("/flash/services/restored-1/backups")) body = { items: [] };
    else if (path.endsWith("/flash/services/restored-1")) body = service;
    else if (path.endsWith("/flash/quota")) body = {};
    else if (path.endsWith("/containers") || path.endsWith("/domains") || path.endsWith("/vpc/networks")) body = { items: [] };
    else return route.fulfill({ status: 404, json: { error: { code: "not_found", message: path } } });
    return route.fulfill({ json: body });
  });
  await page.goto("/flash/backups");
  await expect(page.getByText("before-update")).toBeVisible({ timeout: 3000 });
  await page.getByRole("button", { name: "復元", exact: true }).click();
  await expect(page.getByText(/停止状態の非公開サービスを作成/)).toBeVisible();
  expect(actions).toEqual([]);
  await page.getByRole("textbox", { name: "復元先サービス名" }).fill("recovered");
  await page.getByRole("button", { name: "新しいサービスへ復元" }).click();
  await expect(page).toHaveURL(/\/flash\/services\/restored-1$/);
  await expect(page.getByRole("button", { name: "開始", exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Web Shell" })).toBeDisabled();
  expect(actions).toEqual(["restore-new-service"]);
  expect(errors).toEqual([]);
});
