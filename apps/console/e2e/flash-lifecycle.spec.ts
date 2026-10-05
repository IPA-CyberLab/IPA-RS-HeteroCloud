import { expect, test } from "@playwright/test";

test("private workspace can stop and resume without deletion on desktop and mobile", async ({ page }) => {
  const timestamp = "2026-10-05T00:00:00Z";
  const spec = {
    region: "test", image: "example/workspace:v1", replicas: 1, stopped: false,
    cpu_millis: 100, memory_mib: 128, ephemeral_storage_gib: 1,
    ports: [], exposure: { type: "internal", traffic_mode: "forwarded" },
    env: { HOME: "/root" }, command: ["sleep"], args: ["infinity"], metadata: {},
  };
  let service = {
    id: "workspace", organization_id: "org-test", project_id: "project-test", provider: "flash",
    name: "private-workspace", generation: 1, state: "ready", spec,
    status: { status: { stopped: false, ready_replicas: 1, desired_replicas: 1, runtime_class: "gvisor" } },
    created_at: timestamp, updated_at: timestamp,
  };
  const actions: string[] = [];
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.route("**/api/v1/**", async (route) => {
    const request = route.request();
    const path = new URL(request.url()).pathname;
    let body: unknown;
    if (path.endsWith("/auth/session")) body = {
      user: { id: "user", email: "test@example.test", display_name: "Test", status: "active", created_at: timestamp },
      memberships: [{ organization_id: "org-test", organization_slug: "test", organization_name: "Test", principal_id: "principal", role: "owner" }],
      csrf_token: "test-csrf",
    };
    else if (path.endsWith("/projects")) body = { items: [{ id: "project-test", organization_id: "org-test", slug: "test", name: "Test", created_at: timestamp }] };
    else if (path.endsWith("/vpc/networks")) body = { items: [] };
    else if (path.endsWith("/flash/quota")) body = { max_services: 100, max_replicas_per_service: 100, max_cpu_millis_per_vm: 4000, max_memory_mib_per_vm: 8192, max_disk_gib_per_vm: 10, max_total_replicas: 100, max_total_cpu_millis: 20000, max_total_memory_mib: 32768, max_total_disk_gib: 100 };
    else if (/\/flash\/services\/workspace\/(stop|start)$/.test(path)) {
      expect(request.method()).toBe("POST");
      expect(request.headers()["x-heterocloud-csrf"]).toBe("test-csrf");
      const stopped = path.endsWith("/stop");
      actions.push(stopped ? "stop" : "start");
      service = { ...service, generation: service.generation + 1, spec: { ...service.spec, stopped },
        status: { status: { stopped, ready_replicas: stopped ? 0 : 1, desired_replicas: stopped ? 0 : 1, runtime_class: "gvisor" } } };
      body = service;
    } else if (path.endsWith("/flash/services/workspace")) {
      expect(request.method()).toBe("GET");
      body = service;
    } else return route.fulfill({ status: 404, json: { error: { code: "unexpected_request", message: path } } });
    return route.fulfill({ json: body });
  });
  await page.goto("/flash/services/workspace");
  await page.getByRole("button", { name: "停止", exact: true }).click();
  await expect(page.getByRole("button", { name: "開始", exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Web Shell" })).toBeDisabled();
  await expect(page.getByText(/ホーム領域・シークレット・設定は保持/)).toBeVisible();
  await page.getByRole("button", { name: "開始", exact: true }).click();
  await expect(page.getByRole("button", { name: "停止", exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Web Shell" })).toBeEnabled();
  expect(actions).toEqual(["stop", "start"]);
  expect(errors).toEqual([]);
});
