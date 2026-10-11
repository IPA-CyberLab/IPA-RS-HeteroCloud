// Real authenticated console updates against a disposable fixture; no API mocks.
import { createRequire } from "node:module";
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { resolve } from "node:path";
const require = createRequire(new URL("../../apps/console/package.json", import.meta.url));
const { chromium, devices, expect } = require("@playwright/test");
const [endpoint, fixturePath, serviceId, outputDirectory] = process.argv.slice(2);
if (!outputDirectory) throw new Error("usage: flash_updates_browser.mjs ENDPOINT PRIVATE_FIXTURE SERVICE_ID OUTPUT_DIRECTORY");
const fixture = JSON.parse(await readFile(fixturePath, "utf8"));
const tenant = fixture.tenants[0];
if (!fixture.browser_session || !tenant.slug.startsWith("vpc-e2e-")) throw new Error("disposable browser fixture required");
await mkdir(outputDirectory, { recursive: true, mode: 0o700 });
const browser = await chromium.launch();
const records = [];
try {
  for (const [name, profile, rootfs, total] of [["desktop", devices["Desktop Chrome"], 3, 6], ["mobile", devices["Pixel 7"], 2, 5]]) {
    const context = await browser.newContext({ ...profile });
    await context.addCookies([{ name: "hc_session", value: fixture.browser_session, url: endpoint, secure: new URL(endpoint).protocol === "https:", httpOnly: true, sameSite: "Lax" }]);
    const url = new URL(`/api/v1/organizations/${tenant.organization_id}/flash/services/${serviceId}`, endpoint).href;
    async function service() {
      const response = await context.request.get(url);
      if (!response.ok()) throw new Error("Flash service API unavailable");
      const value = await response.json();
      if (value.organization_id !== tenant.organization_id || !value.name.startsWith("flash-updates-")) throw new Error("Refusing to operate an unrelated resource");
      return value;
    }
    const original = await service();
    const page = await context.newPage();
    const errors = [];
    page.on("pageerror", () => errors.push("JavaScript error"));
    const started = Date.now();
    await page.goto(new URL(`/flash/services/${serviceId}`, endpoint).href, { waitUntil: "domcontentloaded" });
    await expect(page.getByRole("button", { name: "編集", exact: true })).toBeVisible();
    const readyMs = Date.now() - started;
    if (readyMs >= 3000) throw new Error(`${name}: console readiness exceeded 3000ms (${readyMs}ms)`);
    await page.getByRole("button", { name: "編集", exact: true }).click();
    const dialog = page.getByRole("dialog", { name: "Flashサービスを編集" });
    await expect(dialog.getByRole("spinbutton", { name: "コンテナの書き込み領域" })).toBeEnabled();
    await expect(dialog.getByRole("spinbutton", { name: "永続ディスク" })).toBeEnabled();
    await dialog.getByRole("spinbutton", { name: "ディスク上限", exact: true }).fill(String(total));
    await dialog.getByRole("spinbutton", { name: "コンテナの書き込み領域" }).fill(String(rootfs));
    await dialog.getByRole("spinbutton", { name: "永続ディスク" }).fill("2");
    await dialog.getByRole("button", { name: "変更を保存", exact: true }).click();
    await expect(dialog).not.toBeVisible();
    await expect.poll(async () => {
      const value = await service();
      return value.generation > original.generation && value.state === "ready" && value.spec.rootfs_storage_gib === rootfs && value.spec.persistent_storage_gib === 2 && !value.status.status?.storage_resize_pending;
    }, { timeout: 240000 }).toBe(true);
    await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1)).toBe(true);
    if (errors.length) throw new Error(`${name}: browser JavaScript errors`);
    await page.screenshot({ path: resolve(outputDirectory, `flash-storage-updated-${name}.png`), fullPage: true });
    records.push({ browser: name, ready_ms: readyMs, real_api: true, rootfs_gib: rootfs, persistent_gib: 2, passed: true });
    console.log(JSON.stringify(records.at(-1))); await context.close();
  }
  await writeFile(resolve(outputDirectory, "browser-report.json"), JSON.stringify({ records, passed: true }, null, 2), { mode: 0o600 });
} finally { await browser.close(); }
