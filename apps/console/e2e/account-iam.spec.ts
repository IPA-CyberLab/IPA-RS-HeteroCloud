import { expect, test } from "@playwright/test";

test("管理アカウントからOU、メンバーアカウントとSCPを設定する", async ({ page }) => {
  const createdAt = "2026-10-30T00:00:00Z";
  const accountId = "management";
  const summary = { id: "cloud-org", name: "Example", management_account_id: accountId,
    management_account_number: "100000000000", current_account_id: accountId,
    current_account_number: "100000000000", root_id: "root" };
  const units = [{ id: "root", parent_id: null as string | null, name: "Root", created_at: createdAt }];
  const accounts = [{ id: accountId, account_number: "100000000000", name: "Example", slug: "example", parent_id: "root", is_management_account: true, created_at: createdAt }];
  const policies = [{ id: "full", name: "FullAWSAccess", is_managed: true, document: { version: "2026-07-31", statements: [] }, created_at: createdAt }];
  const attachments: { target_type: string; target_id: string; policy_id: string }[] = [{ target_type: "unit", target_id: "root", policy_id: "full" }];
  let submittedPolicy: unknown;
  await page.route("**/api/v1/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    const method = route.request().method();
    if (path.endsWith("/auth/session")) return route.fulfill({ json: { user: { id: "user", email: "test@example.test", display_name: "Test" }, memberships: [{ organization_id: accountId, organization_name: "Example", organization_slug: "example", principal_id: "principal", role: "owner" }], csrf_token: "test", owner_console: false } });
    if (path === "/api/v1/organizations") return route.fulfill({ json: { items: [{ id: accountId, name: "Example", slug: "example", created_at: createdAt }] } });
    if (path.endsWith("/organization")) return route.fulfill({ json: summary });
    if (path.endsWith("/organization/tree")) return route.fulfill({ json: { organization: summary, units, accounts } });
    if (path.endsWith("/organization/units") && method === "POST") {
      const body = route.request().postDataJSON();
      units.push({ id: "ou", name: body.name, parent_id: body.parent_id, created_at: createdAt });
      return route.fulfill({ status: 201, json: units[1] });
    }
    if (path.endsWith("/organization/accounts") && method === "POST") {
      const body = route.request().postDataJSON();
      const account = { id: "member", account_number: "100000000001", name: body.name, slug: body.slug, parent_id: body.parent_id, is_management_account: false, created_at: createdAt };
      accounts.push(account);
      return route.fulfill({ status: 201, json: account });
    }
    if (path.endsWith("/organization/policies") && method === "POST") {
      const body = route.request().postDataJSON();
      submittedPolicy = body.document;
      policies.push({ id: "deny", name: body.name, is_managed: false, document: { version: "2026-07-31", statements: [] }, created_at: createdAt });
      return route.fulfill({ status: 201, json: policies[1] });
    }
    if (path.endsWith("/organization/policies")) return route.fulfill({ json: { enabled: true, policies, attachments } });
    if (path.endsWith("/organization/policies/deny/targets/unit/ou") && method === "PUT") {
      attachments.push({ target_type: "unit", target_id: "ou", policy_id: "deny" });
      return route.fulfill({ status: 204 });
    }
    return route.fulfill({ status: 404, json: { error: { code: "not_found", message: path } } });
  });
  await page.goto("/organizations");
  await page.getByRole("button", { name: "OUを作成" }).click();
  let dialog = page.getByRole("dialog", { name: "組織単位を作成" });
  await dialog.getByLabel("名前", { exact: true }).fill("Workloads");
  await dialog.getByRole("button", { name: "作成", exact: true }).click();
  await expect(page.getByText("Workloads（親: Root）")).toBeVisible();
  await page.getByRole("button", { name: "メンバーアカウントを作成" }).click();
  dialog = page.getByRole("dialog", { name: "メンバーアカウントを作成" });
  await dialog.getByLabel("名前", { exact: true }).fill("Development");
  await dialog.getByLabel("アカウントの識別子").fill("development");
  await dialog.getByLabel("親の組織単位").selectOption("ou");
  await dialog.getByRole("button", { name: "作成", exact: true }).click();
  await expect(page.getByRole("cell", { name: "100000000001", exact: true })).toBeVisible();
  await page.getByRole("button", { name: "ポリシーを作成", exact: true }).click();
  dialog = page.getByRole("dialog", { name: "SCPを作成" });
  await dialog.getByLabel("ポリシー名").fill("DenyDelete");
  await dialog.getByRole("button", { name: "作成", exact: true }).click();
  await expect(page.getByRole("cell", { name: "DenyDelete", exact: true })).toBeVisible();
  expect(submittedPolicy).toMatchObject({ Version: "2012-10-17" });
  await page.getByLabel("SCPポリシー").selectOption("deny");
  await page.getByLabel("SCP適用先").selectOption("unit:ou");
  await page.getByRole("button", { name: "適用", exact: true }).click();
  await expect(page.getByRole("row").filter({ hasText: "DenyDelete" }).getByText("OU: Workloads", { exact: false })).toBeVisible();
});

test("IAMグループとアクセス許可の境界を設定する", async ({ page }) => {
  const createdAt = "2026-10-30T00:00:00Z";
  const groups: { id: string; name: string; created_at: string; members: string[]; policies: string[] }[] = [];
  const boundaries: { principal_id: string; policy_id: string }[] = [];
  await page.route("**/api/v1/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    const method = route.request().method();
    if (path.endsWith("/auth/session")) return route.fulfill({ json: { user: { id: "user", email: "test@example.test", display_name: "Test" }, memberships: [{ organization_id: "account", organization_name: "Example", organization_slug: "example", principal_id: "principal", role: "owner" }], csrf_token: "test", owner_console: false } });
    if (path.endsWith("/iam/principals")) return route.fulfill({ json: { items: [{ id: "principal", organization_id: "account", kind: "user", name: "Test User", user_id: "user", enabled: true, created_at: createdAt }] } });
    if (path.endsWith("/iam/policies")) return route.fulfill({ json: { items: [{ id: "policy", organization_id: "account", name: "ReadOnly", document: { version: "2026-07-31", statements: [] }, semantics_digest: "0".repeat(64), created_at: createdAt, updated_at: createdAt }] } });
    if (path.endsWith("/iam/bindings")) return route.fulfill({ json: { items: [] } });
    if (path.endsWith("/iam/groups") && method === "POST") {
      groups.push({ id: "group", name: route.request().postDataJSON().name, created_at: createdAt, members: [], policies: [] });
      return route.fulfill({ status: 201, json: { id: "group" } });
    }
    if (path.endsWith("/iam/groups")) return route.fulfill({ json: { enabled: true, items: groups } });
    if (path.endsWith("/iam/groups/group/users/principal") && method === "PUT") {
      groups[0].members.push("principal");
      return route.fulfill({ status: 204 });
    }
    if (path.endsWith("/iam/groups/group/policies/policy") && method === "PUT") {
      groups[0].policies.push("policy");
      return route.fulfill({ status: 204 });
    }
    if (path.endsWith("/iam/permissions-boundaries")) return route.fulfill({ json: { enabled: true, items: boundaries } });
    if (path.endsWith("/iam/principals/principal/permissions-boundary") && method === "PUT") {
      boundaries.push({ principal_id: "principal", policy_id: route.request().postDataJSON().policy_id });
      return route.fulfill({ status: 204 });
    }
    return route.fulfill({ status: 404, json: { error: { code: "not_found", message: path } } });
  });
  await page.goto("/iam/bindings");
  await page.getByLabel("グループ名").fill("Developers");
  await page.getByRole("button", { name: "グループを作成" }).click();
  const groupCard = page.locator("section.rounded-box").filter({ hasText: "Developers" });
  await expect(groupCard).toBeVisible();
  await page.getByLabel("IAMグループ").selectOption("group");
  await page.getByLabel("グループに追加するユーザー").selectOption("principal");
  await page.getByRole("button", { name: "ユーザーを追加" }).click();
  await expect(groupCard).toContainText("ユーザー: Test User");
  await page.getByLabel("グループに追加するポリシー").selectOption("policy");
  await page.getByRole("button", { name: "ポリシーを追加", exact: true }).click();
  await expect(groupCard).toContainText("ポリシー: ReadOnly");
  await page.getByLabel("境界を設定するプリンシパル").selectOption("principal");
  await page.getByLabel("境界ポリシー").selectOption("policy");
  await page.getByRole("button", { name: "境界を設定" }).click();
  await expect(page.getByText("Test User / ReadOnly", { exact: false })).toBeVisible();
});
