import { expect, test } from "@playwright/test";

test("a user registers and attaches a secret from the Secret Manager page", async ({ page }) => {
  const organizationId = "org-test";
  const serviceId = "flash-test";
  const timestamp = "2026-09-28T00:00:00Z";
  const names: string[] = [];
  const writes: Array<{ value: string }> = [];
  let secretFiles: Record<string, string> = {};
  const service = () => ({
    id: serviceId,
    organization_id: organizationId,
    project_id: "project-test",
    provider: "flash",
    name: "my-container",
    generation: 1,
    state: "ready",
    spec: { secret_files: secretFiles },
    status: {},
    created_at: timestamp,
    updated_at: timestamp,
  });

  await page.route("**/api/v1/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/api/v1/auth/session") {
      return route.fulfill({ json: {
        user: { id: "user-test", email: "user@example.test", display_name: "Test", status: "active", created_at: timestamp },
        memberships: [{ organization_id: organizationId, organization_slug: "test", organization_name: "Test", principal_id: "principal-test", role: "owner" }],
        csrf_token: "test-csrf",
      } });
    }
    if (path === `/api/v1/organizations/${organizationId}/flash/services`) {
      return route.fulfill({ json: { items: [service()] } });
    }
    if (path === `/api/v1/organizations/${organizationId}/flash/services/${serviceId}/secrets`) {
      return route.fulfill({ json: { items: names } });
    }
    if (path === `/api/v1/organizations/${organizationId}/flash/services/${serviceId}/secrets/api-key`
      && route.request().method() === "PUT") {
      writes.push(route.request().postDataJSON());
      names.push("api-key");
      return route.fulfill({ status: 204, body: "" });
    }
    if (path === `/api/v1/organizations/${organizationId}/flash/services/${serviceId}`
      && route.request().method() === "PUT") {
      const request = route.request().postDataJSON();
      secretFiles = request.spec.secret_files;
      return route.fulfill({ json: { ...service(), spec: request.spec } });
    }
    if (path === "/api/v1/services/secret-manager") {
      return route.fulfill({ json: {
        url: "http://secrets.heteronetwork.internal:21444/ui/vault/auth?with=oidc/",
      } });
    }
    return route.fulfill({ status: 404, json: { error: { code: "unhandled", message: path } } });
  });

  await page.goto("/secrets");
  await expect(page.getByRole("heading", { name: "Hetero Secret Manager" })).toBeVisible();
  await expect(page.getByText("my-container")).toBeVisible();
  await page.getByRole("textbox", { name: "名前" }).fill("api-key");
  await page.getByLabel("値").fill("e2e-value");
  await page.getByRole("button", { name: "登録・更新" }).click();
  await expect.poll(() => writes).toEqual([{ value: "e2e-value" }]);
  await expect(page.getByLabel("値")).toHaveValue("");
  await page.getByRole("button", { name: "コンテナへ接続" }).click();
  await expect.poll(() => secretFiles).toEqual({ "api-key": "api-key" });
  await expect(page.getByText("/vault/secrets/api-key")).toBeVisible();
  await expect(page.getByRole("link", { name: "OpenBaoにOIDCでサインイン" }))
    .toHaveAttribute("href", /with=oidc\/$/);
});
