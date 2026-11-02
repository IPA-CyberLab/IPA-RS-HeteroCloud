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

        // Create/reuse only an explicitly named test member account and OU.
        // Record the slug before creation so cleanup can find it even if the
        // browser loses the response after the server commits the request.
        if (!fixture.iam_child_slug) {
          fixture.iam_child_slug = `vpc-e2e-child-${randomBytes(6).toString("hex")}`;
          await writeFile(fixturePath, JSON.stringify(fixture), { mode: 0o600 });
        }
        if (!fixture.iam_child_slug.startsWith("vpc-e2e-child-")) throw new Error("invalid disposable child account");
        const treeResponse = await context.request.get(new URL("organization/tree", base).href);
        if (!treeResponse.ok()) throw new Error(`organization tree returned ${treeResponse.status()}`);
        const tree = await treeResponse.json();
        let child = tree.accounts.find((account) => account.slug === fixture.iam_child_slug);
        let unitId;
        if (child) {
          if (fixture.iam_child_id && fixture.iam_child_id !== child.id) throw new Error("test account identity changed");
          unitId = child.parent_id;
        } else {
          const unit = await context.request.post(new URL("organization/units", base).href,
            { headers, data: { name: `IAM E2E ${randomBytes(4).toString("hex")}`, parent_id: tree.organization.root_id } });
          if (unit.status() !== 201) throw new Error(`OU creation returned ${unit.status()}`);
          unitId = (await unit.json()).id;
          const account = await context.request.post(new URL("organization/accounts", base).href,
            { headers, data: { name: "IAM E2E member", slug: fixture.iam_child_slug, parent_id: unitId } });
          if (account.status() !== 201) throw new Error(`member account creation returned ${account.status()}`);
          child = await account.json();
          fixture.iam_child_id = child.id;
          await writeFile(fixturePath, JSON.stringify(fixture), { mode: 0o600 });
        }
        const childProjects = new URL(`/api/v1/organizations/${child.id}/projects`, endpoint).href;
        if ((await context.request.get(childProjects)).status() !== 200) throw new Error("test child account is not accessible");
        const scp = await context.request.post(new URL("organization/policies", base).href, {
          headers, data: { name: `e2e-scp-${randomBytes(6).toString("hex")}`,
            document: { Version: "2012-10-17", Statement: { Effect: "Deny", Action: "project:List", Resource: "*",
              Condition: { DateGreaterThan: { "aws:CurrentTime": "2000-01-01T00:00:00Z" } } } } },
        });
        if (scp.status() !== 201) throw new Error(`SCP creation returned ${scp.status()}`);
        const scpId = (await scp.json()).id;
        const attachment = new URL(`organization/policies/${scpId}/targets/unit/${unitId}`, base).href;
        let attached = false;
        try {
          const result = await context.request.put(attachment, { headers });
          if (result.status() !== 204) throw new Error(`SCP attachment returned ${result.status()}`);
          attached = true;
          if ((await context.request.get(childProjects)).status() !== 403) throw new Error("conditional SCP did not deny the child account");
        } finally {
          if (attached) {
            const result = await context.request.delete(attachment, { headers });
            if (result.status() !== 204) throw new Error(`SCP detachment returned ${result.status()}`);
          }
        }
        if ((await context.request.get(childProjects)).status() !== 200) throw new Error("child access did not recover after SCP removal");

        // Assume a role across the test accounts and check a time-limited
        // session policy against the real bearer-token authorization path.
        const childBase = new URL(`/api/v1/organizations/${child.id}/`, endpoint);
        const roleName = `e2e-role-${randomBytes(6).toString("hex")}`;
        const createdRole = await context.request.post(new URL("iam/roles", childBase).href, {
          headers, data: { name: roleName, trust_policy: { Version: "2012-10-17", Statement: [{ Effect: "Allow",
            Action: "sts:AssumeRole", Principal: { AWS: `arn:aws:iam::${tree.organization.management_account_number}:root` } }] } },
        });
        if (createdRole.status() !== 201) throw new Error(`role creation returned ${createdRole.status()}`);
        const role = await createdRole.json();
        const rolePolicy = await context.request.post(new URL("iam/policies", childBase).href, {
          headers, data: { name: `e2e-role-policy-${randomBytes(6).toString("hex")}`,
            document: { Version: "2012-10-17", Statement: { Effect: "Allow", Action: "project:List", Resource: "*" } } },
        });
        if (rolePolicy.status() !== 201) throw new Error(`role policy creation returned ${rolePolicy.status()}`);
        const roleBinding = await context.request.post(new URL("iam/bindings", childBase).href, {
          headers, data: { principal_id: role.principal_id, policy_id: (await rolePolicy.json()).id },
        });
        if (roleBinding.status() !== 201) throw new Error(`role policy binding returned ${roleBinding.status()}`);
        for (const [operator, expected] of [["DateLessThan", 200], ["DateGreaterThan", 403]]) {
          const assumed = await context.request.post(new URL("sts/assume-role", base).href, {
            headers, data: { role_arn: role.arn, session_name: `e2e-${operator}`, session_policy: {
              Version: "2012-10-17", Statement: [
                { Effect: "Allow", Action: "project:List", Resource: "*" },
                { Effect: "Deny", Action: "project:List", Resource: "*", Condition: { [operator]: { "aws:CurrentTime": "2000-01-01T00:00:00Z" } } },
              ] } },
          });
          if (!assumed.ok()) throw new Error(`role assumption returned ${assumed.status()}`);
          const roleSession = await assumed.json();
          try {
            const authorized = await context.request.get(childProjects, { headers: { authorization: `Bearer ${roleSession.access_token}` } });
            if (authorized.status() !== expected) throw new Error(`role session returned ${authorized.status()}, expected ${expected}`);
          } finally {
            const revoked = await context.request.delete(new URL(`iam/roles/${role.principal_id}/sessions/${roleSession.session_id}`, childBase).href, { headers });
            if (revoked.status() !== 204) throw new Error(`role session revocation returned ${revoked.status()}`);
          }
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
