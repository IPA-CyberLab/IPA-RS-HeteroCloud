import { expect, test } from "@playwright/test";

for (const endpointMode of ["load_balancer", "web"] as const) {
test(`Flash autoscaling and ${endpointMode} edit on desktop and mobile`, async ({ page, isMobile }, testInfo) => {
  const web = endpointMode === "web";
  const timestamp = "2026-09-08T00:00:00Z";
  const spec = {
    region: "heteronet-global", image: "example/server:v1", replicas: 2,
    autoscaling: { min_replicas: 2, max_replicas: 8, target_cpu_utilization_percent: 70 },
    cpu_millis: 500, memory_mib: 512, ephemeral_storage_gib: 10,
    ports: [{ name: "game", protocol: web ? "tcp" : "udp", container_port: web ? 8080 : 7777, service_port: 30001 }],
    exposure: { type: "public", traffic_mode: "forwarded", endpoint_mode: endpointMode, allowed_source_cidrs: web ? [] : ["203.0.113.0/24"], denied_source_cidrs: [] },
    env: { MODE: "production" }, command: ["/app/server"], args: ["--listen"], metadata: { keep: true },
  };
  const service = {
    id: "flash-test", organization_id: "org-test", project_id: "project-test", provider: "flash",
    name: "flash-autoscale", generation: 1, state: "ready", spec,
    status: { status: { ready_replicas: 3, desired_replicas: 4, endpoints: [
      { name: "game", protocol: web ? "tcp" : "udp", host: "lb.example.test", port: 30001 },
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
    else if (path.endsWith("/flash/gpu-types")) body = { items: [{ gpu_type: "nvidia-geforce-gtx-1080-ti", display_name: "NVIDIA GeForce GTX 1080 Ti", access: "open", total: 2, available: 2 }] };
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
  if (web) {
    await expect(page.getByRole("link", { name: /https:\/\/lb.example.test/ })).toHaveAttribute("href", "https://lb.example.test");
    await expect(page.getByText(/:443|:30001/)).toHaveCount(0);
    await expect(page.getByText("サービスポート", { exact: true })).toHaveCount(0);
  } else {
    await expect(page.getByText("lb.example.test:30001", { exact: true })).toBeVisible();
    await expect(page.getByRole("link", { name: /lb.example.test/ })).toHaveCount(0);
  }
  await page.screenshot({ path: testInfo.outputPath(web ? "flash-web-detail.png" : "flash-detail.png"), fullPage: true });
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
  if (web) {
    await expect(dialog.getByRole("button", { name: /gameのプロトコル/ })).toHaveText("TCP");
    await expect(dialog.getByRole("button", { name: "エンドポイントを追加" })).toBeDisabled();
    await expect(dialog.getByRole("spinbutton", { name: "サービスポート" })).toHaveCount(0);
    await dialog.getByRole("spinbutton", { name: "コンテナポート" }).fill("8081");
    for (const label of ["受信許可元IP / CIDR", "受信拒否元IP / CIDR"]) {
      const cidrs = dialog.getByRole("textbox", { name: label, exact: true });
      await cidrs.fill("203.0.113.0/24");
      await expect(cidrs).toHaveValue("203.0.113.0/24");
      await expect(dialog.getByRole("button", { name: "変更を保存" })).toBeDisabled();
      await expect(dialog.getByText(/設定を削除するか/).first()).toBeVisible();
      await cidrs.fill("");
    }
    await expect(dialog.getByRole("button", { name: "変更を保存" })).toBeEnabled();
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
  await dialog.getByRole("button", { name: isMobile ? /公開アドレス.*ドメイン/ : web ? "HTTP/HTTPS ドメイン" : "ドメイン (LB)" })
    .evaluate((element) => element.scrollIntoView({ block: "center" }));
  await page.screenshot({ path: testInfo.outputPath(web ? "flash-web-edit.png" : "flash-publishing.png") });
  await dialog.getByRole("button", { name: "変更を保存" }).click();
  await expect(dialog).not.toBeVisible();
  expect(saved?.spec).toMatchObject({ ...spec, ports: [{ name: "game", protocol: web ? "tcp" : "udp", container_port: web ? 8081 : 7777 }],
    autoscaling: { ...spec.autoscaling, max_replicas: 9, target_memory_utilization_percent: 65 } });
  if (web) await expect(page.getByRole("link", { name: /https:\/\/lb.example.test/ })).toHaveAttribute("href", "https://lb.example.test");
  expect(errors).toEqual([]);
});
}


test("Flash作成ではGPU種類だけを選び、レプリカを1に固定する", async ({ page, isMobile }, testInfo) => {
  test.skip(Boolean(isMobile), "デスクトップのGPU選択を検証");
  const timestamp = "2026-09-18T00:00:00Z";
  await page.route("**/api/v1/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    let body: unknown;
    if (path.endsWith("/auth/session")) body = {
      user: { id: "user-test", email: "test@example.test", display_name: "Test", status: "active", created_at: timestamp },
      memberships: [{ organization_id: "org-test", organization_slug: "test", organization_name: "Test", principal_id: "principal-test", role: "owner" }], csrf_token: "test-token",
    };
    else if (path.endsWith("/projects")) body = { items: [{ id: "project-test", organization_id: "org-test", slug: "test", name: "Test", created_at: timestamp }] };
    else if (path.endsWith("/flash/quota")) body = { max_services: 100, max_replicas_per_service: 100, max_cpu_millis_per_vm: 4000, max_memory_mib_per_vm: 8128, max_disk_gib_per_vm: 10, max_total_replicas: 100, max_total_cpu_millis: 20000, max_total_memory_mib: 32768, max_total_disk_gib: 100, max_weekly_gpu_seconds: 40320 };
    else if (path.endsWith("/flash/services")) body = { items: [] };
    else if (path.endsWith("/registry/images")) body = { items: [] };
    else if (path.endsWith("/flash/gpu-types")) body = { items: [
      { gpu_type: "nvidia-geforce-gtx-1080-ti", display_name: "NVIDIA GeForce GTX 1080 Ti", access: "open", total: 2, available: 1 },
      { gpu_type: "nvidia-a100", display_name: "NVIDIA A100", access: "private", total: 1, available: 0 },
    ] };
    else return route.fulfill({ status: 404, json: { error: { code: "unhandled", message: path } } });
    return route.fulfill({ json: body });
  });

  await page.goto("/flash/services");
  await page.getByRole("button", { name: "サービスを作成" }).click();
  const dialog = page.getByRole("dialog", { name: "Flashサービスを作成" });
  await dialog.getByRole("button", { name: /GPU種類.*GPUなし/ }).click();
  const queuedGpu = page.getByRole("option", {
    name: /NVIDIA A100.*Private.*空き 0 \/ 1.*開始まで待機/,
  });
  await expect(queuedGpu).not.toHaveAttribute("aria-disabled", "true");
  await queuedGpu.click();

  await expect(dialog.getByRole("button", { name: /GPU種類.*NVIDIA A100/ })).toBeVisible();
  await expect(
    dialog.getByText("現在空きはありません。リクエストは受け付けられ、GPUが空き次第自動で開始します。"),
  ).toBeVisible();
  await expect(dialog.getByRole("spinbutton", { name: "レプリカ" })).toHaveValue("1");
  await expect(dialog.getByRole("spinbutton", { name: "レプリカ" })).toBeDisabled();
  await dialog.getByRole("button", { name: "自動" }).click();
  await expect(dialog.getByRole("spinbutton", { name: "最大レプリカ" })).toHaveValue("1");
  await expect(dialog.getByRole("spinbutton", { name: "最大レプリカ" })).toBeDisabled();
  await page.screenshot({ path: testInfo.outputPath("flash-gpu-type.png"), fullPage: true });
});
