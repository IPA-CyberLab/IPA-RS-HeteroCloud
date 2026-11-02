// Real HeteroCloud API and disposable browser session; no API mocks.
// The fixture intentionally does not simulate the external identity-provider login.
import { createRequire } from "node:module";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { randomBytes } from "node:crypto";
const require = createRequire(new URL("../../apps/console/package.json", import.meta.url));
const { chromium, devices, expect } = require("@playwright/test");
const [endpoint, fixturePath, outputDirectory, mode = "full"] = process.argv.slice(2);
if (!endpoint || !fixturePath || !outputDirectory || new URL(endpoint).protocol !== "https:") {
  throw new Error("usage: node organization_iam_browser.mjs HTTPS_ENDPOINT PRIVATE_FIXTURE OUTPUT_DIRECTORY");
}
if (!["full", "iam-only"].includes(mode)) throw new Error("mode must be full or iam-only");
const fixture = JSON.parse(await readFile(fixturePath, "utf8"));
const tenant = fixture.tenants?.[0];
if (!fixture.browser_session || !tenant?.slug?.startsWith("vpc-e2e-") || !tenant.organization_id) {
  throw new Error("a disposable browser fixture is required");
}
await mkdir(outputDirectory, { recursive: true, mode: 0o700 });
const records = [];
const browser = await chromium.launch();
try {
  for (const [name, profile] of [["desktop", devices["Desktop Chrome"]], ["mobile", devices["Pixel 7"]]]) {
    const context = await browser.newContext({ ...profile });
    try {
      await context.addCookies([{ name: "hc_session", value: fixture.browser_session, url: endpoint,
        secure: true, httpOnly: true, sameSite: "Lax" }]);
      await context.addInitScript((id) => localStorage.setItem("heterocloud.active-organization", id), tenant.organization_id);
      const page = await context.newPage();
      const errors = [];
      page.on("pageerror", () => errors.push("JavaScript error"));
      const sessionResponse = await context.request.get(new URL("/api/v1/auth/session", endpoint).href);
      if (!sessionResponse.ok()) throw new Error("fixture session is not authenticated");
      const session = await sessionResponse.json();
      if (!session.memberships?.some((m) => m.organization_id === tenant.organization_id && m.role === "owner")) {
        throw new Error("fixture session is outside its disposable account");
      }
      if (mode === "full") {
        const url = new URL(`/api/v1/organizations/${tenant.organization_id}/quotas`, endpoint).href;
        const response = await context.request.get(url);
        if (!response.ok()) throw new Error(`quota API returned ${response.status()}`);
        const quotas = await response.json();
        if (!quotas.cloud_organization?.service_quotas_enabled || !quotas.cloud_organization?.flash_weekly_quotas_enabled ||
            quotas.member?.user_id !== fixture.browser_user_id) throw new Error("shared quota state or owner scope is incorrect");
        await page.goto(new URL("/organizations/quotas", endpoint).href, { waitUntil: "domcontentloaded" });
        await expect(page.getByRole("heading", { name: "自分の上限" })).toBeVisible({ timeout: 15000 });
        await expect(page.getByRole("heading", { name: "組織全体の共有上限" })).toBeVisible({ timeout: 15000 });
        await expect(page.getByText("サービスの割当上限: 適用中 · Flash 週次上限: 適用中")).toBeVisible();
        await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1)).toBe(true);
      }
      if (name === "desktop") {
        const policyName = `e2e-timed-${randomBytes(6).toString("hex")}`;
        await page.goto(new URL("/iam/policies", endpoint).href, { waitUntil: "domcontentloaded" });
        await page.getByRole("button", { name: "ポリシーを作成" }).click();
        const dialog = page.getByRole("dialog", { name: "IAMポリシーを作成" });
        await dialog.getByLabel("ポリシー名").fill(policyName);
        await dialog.getByLabel("アクションを追加").fill("project:List");
        await dialog.getByLabel("アクションを追加").press("Enter");
        await dialog.getByLabel("リソースを追加").fill("*");
        await dialog.getByLabel("リソースを追加").press("Enter");
        await dialog.getByLabel("条件の開始日時（任意）").fill("2027-01-02T09:00");
        await dialog.getByLabel("条件の終了日時（任意）").fill("2027-01-03T09:00");
        await dialog.getByRole("button", { name: "作成", exact: true }).click();
        await expect(dialog).not.toBeVisible({ timeout: 15000 });
        const policiesResponse = await context.request.get(new URL(`/api/v1/organizations/${tenant.organization_id}/iam/policies`, endpoint).href);
        if (!policiesResponse.ok()) throw new Error(`policy list returned ${policiesResponse.status()}`);
        const policies = await policiesResponse.json();
        const policy = policies.items.find((item) => item.name === policyName);
        if (!policy?.document?.statements?.[0]?.conditions?.DateGreaterThanEquals ||
            !policy.document.statements[0].conditions.DateLessThan) throw new Error("timed policy did not persist");

        // Exercise the real authorization path with a scoped, disposable
        // identity. A nonmatching Deny must not apply; a matching Deny must
        // override the account administrator's implicit grant.
        const base = new URL(`/api/v1/organizations/${tenant.organization_id}/`, endpoint);
        const headers = { origin: new URL(endpoint).origin, "x-heterocloud-csrf": session.csrf_token };
        const principal = session.memberships.find((m) => m.organization_id === tenant.organization_id).principal_id;
        let bindingId;
        try {
          for (const [operator, expected] of [["DateLessThan", 200], ["DateGreaterThan", 403]]) {
            const created = await context.request.post(new URL("iam/policies", base).href, {
              headers, data: { name: `e2e-deny-${randomBytes(6).toString("hex")}`,
                document: { Version: "2012-10-17", Statement: { Effect: "Deny", Action: "project:List", Resource: "*",
                  Condition: { [operator]: { "aws:CurrentTime": "2000-01-01T00:00:00Z" } } } } },
            });
            if (created.status() !== 201) throw new Error(`conditional policy create returned ${created.status()}`);
            const policyId = (await created.json()).id;
            const attached = await context.request.post(new URL("iam/bindings", base).href, {
              headers, data: { principal_id: principal, policy_id: policyId },
            });
            if (attached.status() !== 201) throw new Error(`policy binding returned ${attached.status()}`);
            bindingId = (await attached.json()).id;
            const projects = await context.request.get(new URL("projects", base).href);
            if (projects.status() !== expected) throw new Error(`conditional authorization returned ${projects.status()}, expected ${expected}`);
            const removed = await context.request.delete(new URL(`iam/bindings/${bindingId}`, base).href, { headers });
            if (removed.status() !== 204) throw new Error(`policy binding removal returned ${removed.status()}`);
            bindingId = undefined;
          }
        } finally {
          if (bindingId) await context.request.delete(new URL(`iam/bindings/${bindingId}`, base).href, { headers });
        }
      } else if (mode === "iam-only") {
        await page.goto(new URL("/iam/policies", endpoint).href, { waitUntil: "domcontentloaded" });
        await expect(page.getByRole("heading", { name: "IAMポリシー", exact: true })).toBeVisible({ timeout: 15000 });
      }
      if (errors.length) throw new Error(`${name}: browser JavaScript error`);
      const result = { browser: name, mode, real_api: true, external_identity_provider_tested: false, passed: true };
      records.push(result);
      console.log(JSON.stringify(result));
    } finally { await context.close(); }
  }
  await writeFile(resolve(outputDirectory, "organization-iam-report.json"), JSON.stringify({ records, passed: true }, null, 2), { mode: 0o600 });
} finally { await browser.close(); }
