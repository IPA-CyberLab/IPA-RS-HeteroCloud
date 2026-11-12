import { expect, test } from "@playwright/test";

test("権限セットとグループをアカウントに割り当てて切り替える", async ({ page }) => {
  const date = "2026-11-02T00:00:00Z";
  const summary = { id: "organization", name: "Example", management_account_id: "management", management_account_number: "100000000000", current_account_id: "management", current_account_number: "100000000000", root_id: "root" };
  const accounts = [{ id: "target", name: "Development", slug: "development", account_number: "100000000001", parent_id: "root", is_management_account: false, created_at: date }];
  const sets: Record<string, unknown>[] = [];
  const groups: { id: string; name: string; users: string[]; created_at: string }[] = [];
  const assignments: Record<string, unknown>[] = [];
  let activeRole: Record<string, unknown> | undefined;
  await page.route("**/api/v1/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    const method = route.request().method();
    const items = (value: unknown[]) => route.fulfill({ json: { items: value } });
    if (path.endsWith("/auth/session")) return route.fulfill({ json: { user: { id: "user", email: "user@example.test", display_name: "Member" }, memberships: [{ organization_id: "management", organization_name: "Example", organization_slug: "example", principal_id: "principal", role: "owner" }], csrf_token: "test", owner_console: false, ...(activeRole ? { active_role: activeRole } : {}) } });
    if (path.endsWith("/auth/role/exit") && method === "POST") { activeRole = undefined; return route.fulfill({ status: 204 }); }
    if (path === "/api/v1/organizations") return items([{ id: "management", name: "Example", slug: "example", created_at: date }]);
    if (path.endsWith("/organization")) return route.fulfill({ json: summary });
    if (path.endsWith("/organization/tree")) return route.fulfill({ json: { organization: summary, units: [], accounts } });
    if (path.endsWith("/identity-center/users")) return items([{ id: "user", email: "user@example.test", display_name: "Member" }]);
    if (path.endsWith("/identity-center/permission-sets") && method === "POST") {
      const body = route.request().postDataJSON();
      const set = { id: "set", ...body, created_at: date, updated_at: date };
      sets.push(set); return route.fulfill({ status: 201, json: set });
    }
    if (path.endsWith("/identity-center/permission-sets")) return items(sets);
    if (path.endsWith("/identity-center/groups") && method === "POST") {
      const group = { id: "group", name: route.request().postDataJSON().name, users: [], created_at: date };
      groups.push(group); return route.fulfill({ status: 201, json: group });
    }
    if (path.endsWith("/identity-center/groups/group/users/user") && method === "PUT") { groups[0].users.push("user"); return route.fulfill({ status: 204 }); }
    if (path.endsWith("/identity-center/groups")) return items(groups);
    if (path.endsWith("/identity-center/assignments") && method === "POST") {
      const body = route.request().postDataJSON();
      const assignment = { id: "assignment", ...body, permission_set_name: "Readers", account_name: "Development", account_number: "100000000001", role_principal_id: "role", role_arn: "arn:heterocloud:iam::100000000001:role/Readers", session_duration_seconds: 3600, created_at: date };
      assignments.push(assignment); return route.fulfill({ status: 201, json: assignment });
    }
    if (path.endsWith("/identity-center/my-assignments/assignment/session") && method === "POST") {
      activeRole = { session_id: "session", organization_id: "target", account_number: "100000000001", account_name: "Development", principal_id: "role", role_arn: "arn:heterocloud:iam::100000000001:role/Readers", session_name: "identity-center", expires_at: new Date(Date.now() + 3_600_000).toISOString(), active: true };
      return route.fulfill({ json: { switched: true } });
    }
    if (path.endsWith("/identity-center/assignments") || path.endsWith("/identity-center/my-assignments")) return items(assignments);
    return route.fulfill({ status: 404, json: { error: { code: "not_found", message: path } } });
  });
  await page.goto("/identity-center");
  await page.getByRole("button", { name: "権限セットを作成" }).click();
  let dialog = page.getByRole("dialog", { name: "権限セットを作成" });
  await dialog.getByLabel("名前").fill("Readers");
  await dialog.getByRole("button", { name: "保存" }).click();
  await expect(page.getByRole("cell", { name: "Readers" })).toBeVisible();
  await page.getByLabel("グループ名").fill("Developers");
  await page.getByRole("button", { name: "作成", exact: true }).click();
  await page.getByLabel("グループ", { exact: true }).selectOption("group");
  await page.getByLabel("ユーザー", { exact: true }).selectOption("user");
  await page.getByRole("button", { name: "追加", exact: true }).click();
  await expect(page.getByText("メンバー: Member")).toBeVisible();
  await page.getByRole("button", { name: "アカウントに割り当て" }).click();
  dialog = page.getByRole("dialog", { name: "アカウントに割り当て" });
  await dialog.getByLabel("権限セット").selectOption("set");
  await dialog.getByLabel("アカウント", { exact: true }).selectOption("target");
  await dialog.getByLabel("対象の種類").selectOption("group");
  await dialog.getByLabel("対象", { exact: true }).selectOption("group");
  await dialog.getByRole("button", { name: "割り当て", exact: true }).click();
  await expect(page.getByRole("button", { name: "切り替え", exact: true })).toBeVisible();
  await page.getByRole("button", { name: "切り替え", exact: true }).click();
  await expect(page).toHaveURL(/\/overview$/);
  await page.goto("/identity-center");
  await expect(page.getByText("現在はロールを使用しています。", { exact: false })).toBeVisible();
  await page.getByRole("button", { name: "ロールを終了" }).click();
  await expect(page.getByRole("button", { name: "切り替え", exact: true })).toBeVisible();
});
