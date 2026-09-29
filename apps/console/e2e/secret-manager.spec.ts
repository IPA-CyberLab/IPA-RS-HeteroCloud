import { expect, test } from "@playwright/test";

test("a user configures container secrets in the Flash service edit screen", async ({ page }) => {
  const organizationId = "org-test";
  const serviceId = "flash-test";
  const timestamp = "2026-09-28T00:00:00Z";
  const names: string[] = [];
  const writes: Array<{ value: string }> = [];
  let secretEnv: Record<string, string> = {};
  const service = () => ({
    id: serviceId,
    organization_id: organizationId,
    project_id: "project-test",
    provider: "flash",
    name: "my-container",
    generation: 1,
    state: "ready",
    spec: {
      region: "heteronet-global",
      image: "example/nginx:latest",
      replicas: 1,
      cpu_millis: 500,
      memory_mib: 512,
      ephemeral_storage_gib: 10,
      ports: [],
      exposure: { type: "internal", traffic_mode: "forwarded", endpoint_mode: "ip" },
      env: {},
      command: [],
      args: [],
      metadata: {},
      secret_env: secretEnv,
    },
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
    if (path === `/api/v1/organizations/${organizationId}/projects`) {
      return route.fulfill({ json: { items: [{
        id: "project-test", organization_id: organizationId, slug: "test", name: "Test", created_at: timestamp,
      }] } });
    }
    if (path === `/api/v1/organizations/${organizationId}/flash/quota`) {
      return route.fulfill({ json: {
        max_services: 100, max_replicas_per_service: 100,
        max_cpu_millis_per_vm: 4000, max_memory_mib_per_vm: 8128, max_disk_gib_per_vm: 10,
        max_total_replicas: 100, max_total_cpu_millis: 20000,
        max_total_memory_mib: 32768, max_total_disk_gib: 100,
        max_weekly_cpu_millicore_seconds: 3245760000,
        max_weekly_memory_mib_seconds: 2836280317, max_weekly_gpu_seconds: 40320,
      } });
    }
    if (path === `/api/v1/organizations/${organizationId}/flash/services`) {
      return route.fulfill({ json: { items: [service()] } });
    }
    if (path === `/api/v1/organizations/${organizationId}/flash/services/${serviceId}`) {
      if (route.request().method() === "PUT") {
        secretEnv = route.request().postDataJSON().spec.secret_env;
      }
      return route.fulfill({ json: service() });
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
    return route.fulfill({ status: 404, json: { error: { code: "unhandled", message: path } } });
  });

  await page.goto(`/flash/services/${serviceId}`);
  await expect(page.getByRole("heading", { name: "my-container" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "コンテナのシークレット" })).toHaveCount(0);
  await expect(page.getByText("Hetero Secret Manager", { exact: true })).toHaveCount(0);
  await page.getByRole("button", { name: "編集" }).click();
  const editor = page.getByRole("dialog", { name: "Flashサービスを編集" });
  await expect(editor.getByRole("heading", { name: "コンテナのシークレット" })).toBeVisible();
  await editor.getByRole("textbox", { name: "名前" }).fill("api-key");
  await editor.getByLabel("値").fill("e2e-value");
  await editor.getByRole("button", { name: "登録・更新" }).click();
  await expect.poll(() => writes).toEqual([{ value: "e2e-value" }]);
  await expect(editor.getByLabel("値")).toHaveValue("");
  await editor.getByRole("button", { name: "コンテナへ接続" }).click();
  expect(secretEnv).toEqual({});
  await expect(editor.getByText("API_KEY")).toBeVisible();
  await editor.getByRole("button", { name: "キャンセル" }).click();
  await expect(page.getByRole("heading", { name: "コンテナのシークレット" })).toHaveCount(0);
  await page.getByRole("button", { name: "編集" }).click();
  await expect(editor.getByRole("button", { name: "コンテナへ接続" })).toBeVisible();
  await editor.getByRole("button", { name: "コンテナへ接続" }).click();
  await editor.getByRole("button", { name: "変更を保存" }).click();
  await expect.poll(() => secretEnv).toEqual({ API_KEY: "api-key" });
  await expect(editor).toHaveCount(0);
  await expect(page.getByRole("heading", { name: "コンテナのシークレット" })).toHaveCount(0);
});
