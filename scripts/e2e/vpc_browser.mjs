// Live console smoke test: real API and a disposable fixture session.
// It does not mock API routes or claim to test the identity-provider login flow.
import { createRequire } from "node:module";
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { resolve } from "node:path";
const require = createRequire(new URL("../../apps/console/package.json", import.meta.url));
const { chromium, devices, expect } = require("@playwright/test");
const [endpoint, fixturePath, outputDirectory] = process.argv.slice(2);
if (!endpoint || !fixturePath || !outputDirectory) throw new Error("usage: node vpc_browser.mjs ENDPOINT PRIVATE_FIXTURE OUTPUT_DIRECTORY");
const fixture = JSON.parse(await readFile(fixturePath, "utf8"));
if (!fixture.browser_session || !fixture.tenants[0].slug.startsWith("vpc-e2e-")) throw new Error("disposable browser fixture required");
await mkdir(outputDirectory, { recursive: true, mode: 0o700 });
const browser = await chromium.launch();
const records = [];
try {
  for (const [name, profile] of [["desktop", devices["Desktop Chrome"]], ["mobile", devices["Pixel 7"]]]) {
    const context = await browser.newContext({ ...profile });
    await context.addCookies([{ name: "hc_session", value: fixture.browser_session, url: endpoint, secure: true, httpOnly: true, sameSite: "Lax" }]);
    const page = await context.newPage();
    const failures = [];
    page.on("pageerror", error => failures.push(error.message));
    const started = Date.now();
    await page.goto(new URL("/vpc/networks", endpoint).href, { waitUntil: "domcontentloaded" });
    await expect(page.getByRole("heading", { name: "VPC", exact: true })).toBeVisible();
    const readyMs = Date.now() - started;
    await expect(page.getByRole("heading", { name: "vpc-e2e-main", exact: true })).toBeVisible();
    const session = await (await context.request.get(new URL("/api/v1/auth/session", endpoint).href)).json();
    if (!session.memberships.some(m => m.organization_id === fixture.tenants[0].organization_id)) throw new Error("Browser is outside the disposable tenant");
    await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1)).toBe(true);
    await page.screenshot({ path: resolve(outputDirectory, `vpc-${name}.png`), fullPage: true });
    if (failures.length) throw new Error(`${name}: browser JavaScript error`);
    if (readyMs >= 3000) throw new Error(`${name}: VPC console ready in ${readyMs}ms (limit 3000ms)`);
    records.push({ browser: name, ready_ms: readyMs, real_api: true, passed: true });
    console.log(JSON.stringify(records.at(-1)));
    await context.close();
  }
  await writeFile(resolve(outputDirectory, "browser-report.json"), JSON.stringify({ records, passed: true }, null, 2));
} finally { await browser.close(); }
