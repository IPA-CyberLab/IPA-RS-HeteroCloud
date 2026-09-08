import { expect, test } from "@playwright/test";

test("Flash autoscaling and LB edit on desktop and mobile", async ({ page, isMobile }, testInfo) => {
  const timestamp = "2026-09-08T00:00:00Z";
  const spec = {
    region: "heteronet-global", image: "example/server:v1", replicas: 2,
    autoscaling: { min_replicas: 2, max_replicas: 8, target_cpu_utilization_percent: 70 },
    cpu_millis: 500, memory_mib: 512, ephemeral_storage_gib: 10,
    ports: [{ name: "game", protocol: "udp", container_port: 7777, service_port: 30001 }],
    exposure: { type: "public", traffic_mode: "forwarded", endpoint_mode: "load_balancer", allowed_source_cidrs: ["203.0.113.0/24"], denied_source_cidrs: [] },
    env: { MODE: "production" }, command: ["/app/server"], args: ["--listen"], metadata: { keep: true },
  };
  const service = {
    id: "flash-test", organization_id: "org-test", project_id: "project-test", provider: "flash",
    name: "flash-autoscale", generation: 1, state: "ready", spec,
    status: { status: { ready_replicas: 3, desired_replicas: 4, endpoints: [
      { name: "game", protocol: "udp", host: "lb.example.test", port: 30001 },
    ] } }, created_at: timestamp, updated_at: timestamp,
  };
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  let saved: { spec: typeof spec; name: string } | undefined;
  // Every API request is fulfilled locally, including the edit submission.
  await page.route("**/api/v1/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    let body: unknown;
    if (path.endsWith("/auth/session")) body = {
      user: { id: "user-test", email: "test@example.test", display_name: "Test", status: "active", created_at: timestamp },
      memberships: [{ organization_id: "org-test", organization_slug: "test", organization_name: "Test", principal_id: "principal-test", role: "owner" }], csrf_token: "test-token",
    };
    else if (path.endsWith("/projects")) body = { items: [{ id: "project-test", organization_id: "org-test", slug: "test", name: "Test", created_at: timestamp }] };
    else if (path.endsWith("/flash/quota")) body = { max_services: 100, max_replicas_per_service: 100, max_cpu_millis_per_vm: 4000, max_memory_mib_per_vm: 8128, max_disk_gib_per_vm: 10, max_total_replicas: 100, max_total_cpu_millis: 20000, max_total_memory_mib: 32768, max_total_disk_gib: 100 };
    else if (path.endsWith("/flash/services/flash-test")) {
      if (route.request().method() !== "GET") saved = route.request().postDataJSON();
      body = saved ? { ...service, ...saved } : service;
    } else if (path.endsWith("/registry/images")) body = { items: [] };
    else return route.fulfill({ status: 404, json: { error: { code: "unhandled", message: path } } });
    return route.fulfill({ json: body });
  });
  await page.goto("/flash/services/flash-test");
  await expect(page.getByText("自動・2〜8")).toBeVisible();
  await expect(page.getByText("要求レプリカ", { exact: true }).locator("..")).toContainText("4");
  await expect(page.getByText("稼働レプリカ", { exact: true }).locator("..")).toContainText("3");
  await expect(page.getByText("lb.example.test:30001", { exact: true })).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath("flash-detail.png"), fullPage: true });
  await page.getByRole("button", { name: "編集", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Flashサービスを編集" });
  await expect(dialog.getByRole("spinbutton", { name: "最小レプリカ" })).toHaveValue("2");
  if (isMobile) {
    await dialog.getByRole("button", { name: /通信モード.*転送/ }).click();
    await expect(page.getByRole("option", { name: "ダイレクト" })).toHaveAttribute("aria-disabled", "true");
    await page.keyboard.press("Escape");
  } else {
    await expect(dialog.getByRole("button", { name: "ダイレクト" })).toBeDisabled();
  }
  await dialog.getByRole("spinbutton", { name: "最大レプリカ" }).fill("9");
  await dialog.getByRole("checkbox", { name: "メモリ目標使用率 (%)" }).check();
  await dialog.getByRole("spinbutton", { name: "メモリ目標使用率 (%)" }).fill("65");
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1)).toBe(true);
  await dialog.getByRole("spinbutton", { name: "最小レプリカ" }).scrollIntoViewIfNeeded();
  const positions = await dialog.locator(".flash-scale-controls input[type=number]").evaluateAll((inputs) =>
    inputs.map((input) => { const rect = input.getBoundingClientRect(); return { x: rect.x, y: rect.y, width: rect.width }; }),
  );
  expect(positions).toHaveLength(4);
  expect(positions[0].y).toBeCloseTo(positions[1].y, 0);
  expect(positions[2].y).toBeCloseTo(positions[3].y, 0);
  for (const position of positions) expect(position.width).toBeGreaterThan(80);
  if (!isMobile) expect(positions[0].y).toBeCloseTo(positions[2].y, 0);
  await page.screenshot({ path: testInfo.outputPath("flash-autoscale.png"), fullPage: true });
  await dialog.locator(".flash-scale-controls").screenshot({ path: testInfo.outputPath("flash-scale-controls.png") });
  await dialog.getByRole("button", { name: isMobile ? /公開アドレス.*ドメイン/ : "ドメイン (LB)" })
    .evaluate((element) => element.scrollIntoView({ block: "center" }));
  await page.screenshot({ path: testInfo.outputPath("flash-publishing.png") });
  await dialog.getByRole("button", { name: "変更を保存" }).click();
  await expect(dialog).not.toBeVisible();
  expect(saved?.spec).toMatchObject({ ...spec, ports: [{ name: "game", protocol: "udp", container_port: 7777 }],
    autoscaling: { ...spec.autoscaling, max_replicas: 9, target_memory_utilization_percent: 65 } });
  expect(errors).toEqual([]);
});
