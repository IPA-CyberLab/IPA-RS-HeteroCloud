// Real console/API controls; uses a fixture session, not a simulated IdP login.
import { createRequire } from "node:module";
import { readFile, writeFile } from "node:fs/promises";
const require = createRequire(new URL("../../apps/console/package.json", import.meta.url));
const { chromium, devices, expect } = require("@playwright/test");
const [fixturePath, reportPath] = process.argv.slice(2);
if (!fixturePath || !reportPath) throw new Error("usage: node task_iam_oidc_console.mjs PRIVATE_FIXTURE REPORT");
const fixture = JSON.parse(await readFile(fixturePath, "utf8"));
const tenant = fixture.tenants[0], test = fixture.task_iam;
if (!fixture.browser_session || !tenant.slug.startsWith("vpc-e2e-")) throw new Error("owned fixture session required");
const endpoint = new URL(test.endpoint).origin;
const browser = await chromium.launch();
const report = { schema_version: 1, passed: false, records: [] };
let stage = "fixture_session";
let dialogForDiagnostics;
async function chooseAuthentication(page, dialog, label) {
  const button = dialog.getByRole("button", { name: label, exact: true });
  if (await button.isVisible()) {
    await button.click();
  } else {
    // Cloudscape presents the same segmented control as a select on mobile.
    await dialog.getByRole("button", { name: /^ロードバランサー認証/ }).click();
    await page.getByRole("option", { name: label, exact: true }).click();
  }
}
try {
  for (const [name, profile] of [["desktop", devices["Desktop Chrome"]], ["mobile", devices["Pixel 7"]]]) {
    const context = await browser.newContext(profile);
    await context.addCookies([{ name: "hc_session", value: fixture.browser_session, url: endpoint, secure: true, httpOnly: true, sameSite: "Lax" }]);
    const sessionResponse = await context.request.get(`${endpoint}/api/v1/auth/session`);
    if (!sessionResponse.ok()) throw new Error("fixture session is unavailable");
    const session = await sessionResponse.json();
    if (!session.memberships.some(m => m.organization_id === tenant.organization_id)) throw new Error("unrelated organization");
    const page = await context.newPage();
    const errors = [];
    page.on("pageerror", () => errors.push("JavaScript error"));
    const started = Date.now();
    stage = `${name}_parent_console_load`;
    await page.goto(`${endpoint}/flash/services/${test.parent_id}`, { waitUntil: "domcontentloaded" });
    await expect(page.getByRole("heading", { name: `task-iam-e2e-parent-${test.nonce}`, exact: true })).toBeVisible();
    const readyMs = Date.now() - started;
    if (readyMs >= 3000) throw new Error(`${name}: console load ${readyMs}ms exceeds 3000ms`);
    await page.getByRole("button", { name: "編集", exact: true }).click();
    stage = `${name}_task_role_selection`;
    let dialog = page.getByRole("dialog");
    await expect(dialog.getByRole("button", { name: /^タスクIAM/ })).toContainText(`task-iam-e2e-controller-${test.nonce}`);
    await dialog.getByRole("button", { name: "キャンセル", exact: true }).click();
    await page.goto(`${endpoint}/flash/services/${test.web_id}`, { waitUntil: "domcontentloaded" });
    await page.getByRole("button", { name: "編集", exact: true }).click();
    dialog = page.getByRole("dialog");
    dialogForDiagnostics = dialog;
    stage = `${name}_optional_oidc_and_callback`;
    await chooseAuthentication(page, dialog, "OIDC");
    await expect(dialog.getByLabel("Issuer URL", { exact: true })).toBeVisible();
    await expect(dialog.getByLabel("Client ID", { exact: true })).toBeVisible();
    await expect(dialog.getByLabel("Client Secret", { exact: true })).toHaveValue("");
    await expect(dialog.getByText(test.callback_url, { exact: true })).toBeVisible();
    await chooseAuthentication(page, dialog, "認証なし");
    await expect(dialog.getByLabel("Client Secret", { exact: true })).toHaveCount(0);
    await dialog.getByRole("button", { name: "キャンセル", exact: true }).click();
    await page.goto(`${endpoint}/iam/principals`, { waitUntil: "domcontentloaded" });
    stage = `${name}_principal_controls`;
    await expect(page.getByRole("heading", { name: "IAMプリンシパル", exact: true })).toBeVisible();
    await expect(page.getByRole("row").filter({ hasText: `task-iam-e2e-controller-${test.nonce}` }).getByRole("button", { name: "無効化", exact: true })).toBeVisible();
    await page.goto(`${endpoint}/iam/bindings`, { waitUntil: "domcontentloaded" });
    stage = `${name}_binding_controls`;
    await expect(page.getByRole("heading", { name: "IAMバインディング", exact: true })).toBeVisible();
    await expect(page.getByText(`task-iam-e2e-controller-${test.nonce} / task-iam-e2e-policy-${test.nonce}`).first()).toBeVisible();
    await expect(page.getByRole("button", { name: "割り当てを解除", exact: true }).first()).toBeVisible();
    expect(errors).toHaveLength(0);
    await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1)).toBe(true);
    const record = { browser: name, ready_ms: readyMs, real_api: true, fixture_session: true,
                     task_role_selection: true, optional_oidc_controls: true, secret_not_prefilled: true,
                     principal_and_binding_controls: true, passed: true };
    report.records.push(record);
    await writeFile(reportPath, JSON.stringify(report, null, 2));
    console.log(JSON.stringify(record));
    await context.close();
  }
  report.passed = true;
  await writeFile(reportPath, JSON.stringify(report, null, 2));
} catch (error) {
  if (stage.endsWith("_optional_oidc_and_callback") && process.env.HETEROCLOUD_E2E_PRIVATE_DEBUG === "true") {
    // Only this read-only form stage: password fields are never populated.
    const snapshot = await dialogForDiagnostics.ariaSnapshot().catch(() => "dialog unavailable");
    await writeFile(`${reportPath}.private-error.txt`, String(error.message) + "\n" + snapshot, { mode: 0o600 });
  }
  report.failure_stage = stage;
  await writeFile(reportPath, JSON.stringify(report, null, 2));
  console.error(`Console E2E failed at ${stage}; session credentials are not included in diagnostics`);
  process.exitCode = 1;
} finally { await browser.close(); }
