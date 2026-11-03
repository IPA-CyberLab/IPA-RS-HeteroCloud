import { expect, test } from "@playwright/test";

test("表示名、組織、アカウント、組織単位とプロジェクトの名前を変更する", async ({ page }) => {
  const createdAt = "2026-10-30T00:00:00Z";
  const accountId = "management";
  const user = { id: "user", email: "user@example.test", display_name: "User", status: "active", created_at: createdAt };
  const membership = { organization_id: accountId, organization_name: "Account", organization_slug: "account", principal_id: "principal", role: "owner" };
  const summary = { id: "cloud-org", name: "Organization", management_account_id: accountId, management_account_number: "100000000000", current_account_id: accountId, current_account_number: "100000000000", root_id: "root" };
  const units = [{ id: "root", parent_id: null as string | null, name: "Root", created_at: createdAt }, { id: "unit", parent_id: "root", name: "Workloads", created_at: createdAt }];
  const accounts = [{ id: accountId, account_number: "100000000000", name: "Account", slug: "account", parent_id: "root", is_management_account: true, created_at: createdAt }, { id: "member", account_number: "100000000001", name: "Member", slug: "member", parent_id: "unit", is_management_account: false, created_at: createdAt }];
  const project = { id: "project", organization_id: accountId, name: "Project", slug: "project", created_at: createdAt };
  const principal = { id: "principal", organization_id: accountId, name: "IAM User", kind: "user", user_id: "user", enabled: true, created_at: createdAt };
  const changed: string[] = [];

  await page.route("**/api/v1/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    const method = route.request().method();
    if (path === "/api/v1/auth/session") return route.fulfill({ json: { user, memberships: [membership], csrf_token: "csrf", owner_console: false } });
    if (path === "/api/v1/auth/profile" && method === "PATCH") {
      user.display_name = route.request().postDataJSON().name;
      changed.push("profile");
      return route.fulfill({ status: 204 });
    }
    if (path === "/api/v1/organizations") return route.fulfill({ json: { items: [accounts[0]] } });
    if (path === `/api/v1/organizations/${accountId}` && method === "PATCH") {
      accounts[0].name = membership.organization_name = route.request().postDataJSON().name;
      changed.push("account");
      return route.fulfill({ json: accounts[0] });
    }
    if (path === `/api/v1/organizations/${accountId}/organization`) {
      if (method === "PATCH") { summary.name = route.request().postDataJSON().name; changed.push("organization"); return route.fulfill({ status: 204 }); }
      return route.fulfill({ json: summary });
    }
    if (path.endsWith("/organization/tree")) return route.fulfill({ json: { organization: summary, units, accounts } });
    if (path.endsWith("/organization/policies")) return route.fulfill({ json: { enabled: true, policies: [], attachments: [] } });
    if (path.endsWith("/organization/units/unit") && method === "PATCH") { units[1].name = route.request().postDataJSON().name; changed.push("unit"); return route.fulfill({ status: 204 }); }
    if (path.endsWith("/organization/accounts/member") && method === "PATCH") { accounts[1].name = route.request().postDataJSON().name; changed.push("member"); return route.fulfill({ status: 204 }); }
    if (path.endsWith("/projects/project") && method === "PATCH") { project.name = route.request().postDataJSON().name; changed.push("project"); return route.fulfill({ json: project }); }
    if (path.endsWith("/projects")) return route.fulfill({ json: { items: [project] } });
    if (path.endsWith("/iam/users/principal") && method === "PATCH") { principal.name = route.request().postDataJSON().name; changed.push("iam-user"); return route.fulfill({ json: principal }); }
    if (path.endsWith("/iam/principals")) return route.fulfill({ json: { items: [principal] } });
    return route.fulfill({ status: 404, json: { error: { code: "unhandled", message: path } } });
  });

  await page.goto("/settings");
  await page.getByRole("button", { name: "表示名を変更" }).click();
  let dialog = page.getByRole("dialog", { name: "表示名を変更" });
  await dialog.getByLabel("表示名").fill("New User");
  await dialog.getByRole("button", { name: "保存" }).click();
  await expect(dialog).not.toBeVisible();
  await expect(page.getByRole("main").getByText("New User")).toBeVisible();

  await page.goto("/organizations");
  await page.getByRole("button", { name: "組織名を変更" }).click();
  dialog = page.getByRole("dialog", { name: "名前を変更" });
  await dialog.getByLabel("名前").fill("New Organization");
  await dialog.getByRole("button", { name: "保存" }).click();
  await expect(page.getByRole("heading", { name: "組織: New Organization" })).toBeVisible();
  await page.getByRole("button", { name: "Accountのアカウント名を変更" }).click();
  await dialog.getByLabel("名前").fill("New Account");
  await dialog.getByRole("button", { name: "保存" }).click();
  await expect(page.getByRole("button", { name: "New Accountのアカウント名を変更" })).toBeVisible();
  await page.getByRole("button", { name: "Workloadsの組織単位名を変更" }).click();
  await dialog.getByLabel("名前").fill("New Unit");
  await dialog.getByRole("button", { name: "保存" }).click();
  await expect(page.getByRole("button", { name: "New Unitの組織単位名を変更" })).toBeVisible();
  await page.getByRole("button", { name: "Memberのメンバーアカウント名を変更" }).click();
  await dialog.getByLabel("名前").fill("New Member");
  await dialog.getByRole("button", { name: "保存" }).click();
  await expect(page.getByRole("button", { name: "New Memberのメンバーアカウント名を変更" })).toBeVisible();

  await page.goto("/projects");
  await page.getByRole("button", { name: "Projectのプロジェクト名を変更" }).click();
  dialog = page.getByRole("dialog", { name: "プロジェクト名を変更" });
  await dialog.getByLabel("プロジェクト名").fill("New Project");
  await dialog.getByRole("button", { name: "保存" }).click();
  await expect(page.getByRole("button", { name: "New Projectのプロジェクト名を変更" })).toBeVisible();
  await page.goto("/iam/principals");
  await page.getByRole("button", { name: "IAM UserのIAMユーザー名を変更" }).click();
  dialog = page.getByRole("dialog", { name: "IAMユーザー名を変更" });
  await dialog.getByLabel("IAMユーザー名").fill("New IAM User");
  await dialog.getByRole("button", { name: "保存" }).click();
  await expect(page.getByRole("button", { name: "New IAM UserのIAMユーザー名を変更" })).toBeVisible();
  expect(changed).toEqual(["profile", "organization", "account", "unit", "member", "project", "iam-user"]);
});
