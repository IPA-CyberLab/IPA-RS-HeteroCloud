// Real console/API lifecycle test with an isolated, expiring fixture session.
// Identity-provider sign-in is outside this test. No requests are mocked.
import { createRequire } from "node:module";
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { resolve } from "node:path";
const require = createRequire(new URL("../../apps/console/package.json", import.meta.url));
const { chromium, devices, expect } = require("@playwright/test");
const [endpoint, fixturePath, serviceId, outputDirectory] = process.argv.slice(2);
if (!endpoint || !fixturePath || !serviceId || !outputDirectory) throw new Error("usage: node flash_lifecycle_browser.mjs ENDPOINT PRIVATE_FIXTURE SERVICE_ID OUTPUT_DIRECTORY");
const fixture = JSON.parse(await readFile(fixturePath, "utf8"));
const tenant = fixture.tenants[0];
if (!fixture.browser_session || !tenant.slug.startsWith("vpc-e2e-")) throw new Error("disposable browser fixture required");
await mkdir(outputDirectory, { recursive: true, mode: 0o700 });
const browser = await chromium.launch();
const records = [];
try {
  for (const [name, profile] of [["desktop", devices["Desktop Chrome"]], ["mobile", devices["Pixel 7"]]]) {
    const context = await browser.newContext({ ...profile });
    await context.addCookies([{ name: "hc_session", value: fixture.browser_session, url: endpoint, secure: new URL(endpoint).protocol === "https:", httpOnly: true, sameSite: "Lax" }]);
    const session = await (await context.request.get(new URL("/api/v1/auth/session", endpoint).href)).json();
    if (!session.memberships.some(m => m.organization_id === tenant.organization_id)) throw new Error("Browser is outside the disposable tenant");
    const apiUrl = new URL(`/api/v1/organizations/${tenant.organization_id}/flash/services/${serviceId}`, endpoint).href;
    async function service() {
      const response = await context.request.get(apiUrl);
      if (!response.ok()) throw new Error("Lifecycle service API unavailable");
      const value = await response.json();
      if (value.id !== serviceId || value.organization_id !== tenant.organization_id || !value.name.startsWith("flash-lifecycle-")) throw new Error("Refusing to operate an unrelated service");
      return value;
    }
    await service();
    const page = await context.newPage();
    const failures = [];
    page.on("pageerror", error => failures.push(error.message));
    const started = Date.now();
    await page.goto(new URL(`/flash/services/${serviceId}`, endpoint).href, { waitUntil: "domcontentloaded" });
    await expect(page.getByRole("button", { name: "停止", exact: true })).toBeVisible();
    const readyMs = Date.now() - started;
    if (readyMs >= 3000) throw new Error(`${name}: Flash console ready in ${readyMs}ms (limit 3000ms)`);
    await page.getByRole("button", { name: "停止", exact: true }).click();
    await expect(page.getByRole("button", { name: "開始", exact: true })).toBeVisible();
    await expect(page.getByRole("button", { name: "Web Shell" })).toBeDisabled();
    await expect.poll(async () => {
      const value = await service();
      return value.state === "ready" && value.status.status?.stopped === true;
    }, { timeout: 180000 }).toBe(true);
    await page.screenshot({ path: resolve(outputDirectory, `flash-stopped-${name}.png`), fullPage: true });
    await page.getByRole("button", { name: "開始", exact: true }).click();
    await expect.poll(async () => {
      const value = await service();
      return value.state === "ready" && value.spec.stopped !== true && value.status.status?.ready_replicas === 1;
    }, { timeout: 180000 }).toBe(true);
    await expect(page.getByRole("button", { name: "Web Shell" })).toBeEnabled();
    await expect(page.getByRole("button", { name: "停止", exact: true })).toBeVisible();
    await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1)).toBe(true);
    if (failures.length) throw new Error(`${name}: browser JavaScript error`);
    records.push({ browser: name, ready_ms: readyMs, real_api: true, stopped_and_resumed: true, passed: true });
    console.log(JSON.stringify(records.at(-1)));
    await context.close();
  }
  await writeFile(resolve(outputDirectory, "browser-report.json"), JSON.stringify({ records, passed: true }, null, 2), { mode: 0o600 });
} finally { await browser.close(); }
