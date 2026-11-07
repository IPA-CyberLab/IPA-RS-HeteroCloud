// Real console and API, using only the disposable fixture session and workload.
// This checks the UI; the fixture session does not test the IdP login flow.
import { createRequire } from "node:module";
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { resolve } from "node:path";
const require = createRequire(new URL("../../apps/console/package.json", import.meta.url));
const { chromium, devices, expect } = require("@playwright/test");
const [endpoint, fixturePath, serviceId, marker, outputDirectory] = process.argv.slice(2);
if (!endpoint || !fixturePath || !serviceId || !marker || !outputDirectory) throw new Error("missing live test arguments");
const fixture = JSON.parse(await readFile(fixturePath, "utf8"));
const tenant = fixture.tenants[0];
if (!fixture.browser_session || !tenant.slug.startsWith("vpc-e2e-") || !marker.startsWith("flash-logs-e2e-marker-")) throw new Error("disposable test fixture required");
await mkdir(outputDirectory, { recursive: true, mode: 0o700 });
const browser = await chromium.launch();
const records = [];
try {
  for (const [name, profile] of [["desktop", devices["Desktop Chrome"]], ["mobile", devices["Pixel 7"]]]) {
    const context = await browser.newContext({ ...profile });
    await context.addCookies([{ name: "hc_session", value: fixture.browser_session, url: endpoint,
      secure: new URL(endpoint).protocol === "https:", httpOnly: true, sameSite: "Lax" }]);
    const session = await (await context.request.get(new URL("/api/v1/auth/session", endpoint).href)).json();
    if (!session.memberships.some(m => m.organization_id === tenant.organization_id)) throw new Error("browser is outside the disposable tenant");
    const response = await context.request.get(new URL(`/api/v1/organizations/${tenant.organization_id}/flash/services/${serviceId}`, endpoint).href);
    if (!response.ok()) throw new Error("disposable service is unavailable");
    const service = await response.json();
    if (service.id !== serviceId || service.organization_id !== tenant.organization_id || !service.name.startsWith("flash-logs-e2e-")) throw new Error("refusing an unrelated service");
    const page = await context.newPage();
    const errors = [];
    page.on("pageerror", () => errors.push("pageerror"));
    await page.goto(new URL(`/flash/services/${serviceId}`, endpoint).href, { waitUntil: "domcontentloaded" });
    await page.getByRole("tab", { name: "コンテナログ" }).click({ timeout: 30000 });
    await expect(page.locator("pre")).toContainText(marker, { timeout: 30000 });
    await expect(page.getByLabel("ログを表示するコンテナ")).toBeVisible();
    if (errors.length) throw new Error(`${name}: browser JavaScript error`);
    records.push({ browser: name, real_api: true, marker_visible: true, passed: true });
    console.log(JSON.stringify(records.at(-1)));
    await context.close();
  }
  await writeFile(resolve(outputDirectory, "browser-report.json"), JSON.stringify({ records, passed: true }, null, 2), { mode: 0o600 });
} finally { await browser.close(); }
