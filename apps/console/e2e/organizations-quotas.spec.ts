import { expect, test } from "@playwright/test";

test("組織を追加して切り替え、メンバーの上限を設定する", async ({ page }) => {
  const createdAt = "2026-10-30T00:00:00Z";
  const organizations = [{ id: "org-1", slug: "first-org", name: "First org", created_at: createdAt }];
  const memberships = [{ organization_id: "org-1", organization_slug: "first-org", organization_name: "First org", principal_id: "principal-1", role: "owner" }];
  const limits = {
    flow: { max_services: 5, max_rooms_per_service: 10, max_total_rooms: 50, max_participants_per_service: 20, max_rate_limit_requests_per_second: 1, max_rate_limit_burst: 1, max_developer_credentials_per_service: 3 },
    flash: { max_services: 5, max_replicas_per_service: 1, max_total_replicas: 5, max_cpu_millis_per_vm: 1000, max_total_cpu_millis: 5000, max_memory_mib_per_vm: 512, max_total_memory_mib: 2560, max_disk_gib_per_vm: 1, max_total_disk_gib: 5, max_weekly_cpu_millicore_seconds: 10000, max_weekly_memory_mib_seconds: 20000, max_weekly_gpu_seconds: 300 },
    registry: { storage_gib: 10, max_credentials: 5 },
    syouyu: { max_buckets: 5, max_bytes_per_bucket: 1048576, max_total_bytes: 5242880, max_objects_per_bucket: 100, max_credentials_per_bucket: 2, max_total_credentials: 10 },
  };
  let updated = false;
  await page.route("**/api/v1/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path.endsWith("/auth/session")) return route.fulfill({ json: {
      user: { id: "user-1", email: "user@example.test", display_name: "User", status: "active", created_at: createdAt },
      memberships, csrf_token: "test-token", owner_console: false,
    } });
    if (path === "/api/v1/organizations" && route.request().method() === "POST") {
      const input = route.request().postDataJSON();
      const item = { id: "org-2", slug: input.slug, name: input.name, created_at: createdAt };
      organizations.push(item);
      memberships.push({ organization_id: item.id, organization_slug: item.slug, organization_name: item.name, principal_id: "principal-2", role: "owner" });
      return route.fulfill({ status: 201, json: item });
    }
    if (path === "/api/v1/organizations") return route.fulfill({ json: { items: organizations } });
    if (path.endsWith("/quota-unassigned")) return route.fulfill({ json: { items: [] } });
    if (path.endsWith("/members/user-1/quota") && route.request().method() === "PUT") {
      updated = true;
      return route.fulfill({ json: route.request().postDataJSON() });
    }
    if (path.endsWith("/quotas")) {
      const orgId = path.split("/")[4];
      const item = organizations.find((org) => org.id === orgId)!;
      const member = { user_id: "user-1", principal_id: "principal-2", display_name: "User", email: "user@example.test", role: "owner", override_limits: null, effective_limits: limits, usage: {} };
      return route.fulfill({ json: { organization: { organization: item, effective_limits: limits, override_limits: null, usage: {} }, member, members: [member] } });
    }
    return route.fulfill({ status: 404, json: { error: { code: "unhandled", message: path } } });
  });
  await page.goto("/organizations");
  await page.getByRole("button", { name: "組織を作成" }).click();
  const dialog = page.getByRole("dialog", { name: "組織を作成" });
  await dialog.getByLabel("組織名").fill("Second org");
  await dialog.getByLabel("組織の識別子").fill("second-org");
  await dialog.getByRole("button", { name: "作成", exact: true }).click();
  await expect(dialog).not.toBeVisible();
  await expect.poll(() => page.evaluate(() => localStorage.getItem("heterocloud.active-organization"))).toBe("org-2");
  await page.locator('summary[aria-label="操作対象の組織"]').click();
  await page.getByRole("menuitem", { name: "First org" }).click();
  await expect.poll(() => page.evaluate(() => localStorage.getItem("heterocloud.active-organization"))).toBe("org-1");
  await page.locator('summary[aria-label="操作対象の組織"]').click();
  await page.getByRole("menuitem", { name: "Second org" }).click();
  await expect.poll(() => page.evaluate(() => localStorage.getItem("heterocloud.active-organization"))).toBe("org-2");
  await page.goto("/organizations/quotas");
  await expect(page.getByRole("heading", { name: "自分の上限" })).toBeVisible();
  await page.getByText(/User · user@example.test/).click();
  await page.getByRole("button", { name: "上限を変更" }).click();
  const editor = page.getByRole("dialog", { name: "User の上限を変更" });
  await editor.getByRole("button", { name: "保存" }).click();
  await expect.poll(() => updated).toBe(true);
  await expect(editor).not.toBeVisible();
});
