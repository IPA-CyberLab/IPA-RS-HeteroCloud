// Real OIDC on an owned disposable Flash service; no requests or IdP are mocked.
import { createRequire } from "node:module";
import { readFile, writeFile } from "node:fs/promises";
const require = createRequire(new URL("../../apps/console/package.json", import.meta.url));
const { chromium, devices, expect, request } = require("@playwright/test");
const [fixturePath, reportPath] = process.argv.slice(2);
if (!fixturePath || !reportPath) throw new Error("usage: node load_balancer_oidc_browser.mjs PRIVATE_FIXTURE REPORT");
const fixture = JSON.parse(await readFile(fixturePath, "utf8"));
const tenant = fixture.tenants[0], test = fixture.task_iam, oidc = fixture.oidc_test;
if (!tenant.slug.startsWith("vpc-e2e-") || !oidc.client_id.startsWith("hc-oidc-e2e-")) throw new Error("owned fixture required");
const endpoint = new URL(test.endpoint).origin;
const base = `${endpoint}/api/v1/organizations/${tenant.organization_id}/flash/services/${test.web_id}`;
const admin = await request.newContext({ extraHTTPHeaders: { Authorization: `Bearer ${tenant.api_key}` }, userAgent: "HeteroCloud-VPC-E2E/1.0" });
const browser = await chromium.launch();
const report = { schema_version: 1, checks: [], passed: false, service_id: test.web_id };
let stage = "fixture_reset";
async function record(name, details = {}) {
  report.checks.push({ name, passed: true, ...details });
  await writeFile(reportPath, JSON.stringify(report, null, 2));
  console.log(`PASS ${name}`);
}
async function service() {
  let response;
  for (let attempt = 0; attempt < 3; attempt++) {
    response = await admin.get(base, { maxRedirects: 0 });
    if (response.status() !== 503) break;
    await new Promise(resolve => setTimeout(resolve, 1000));
  }
  if (!response.ok()) throw new Error(`service API returned HTTP ${response.status()}`);
  const value = await response.json();
  if (value.organization_id !== tenant.organization_id || !value.name.startsWith("task-iam-e2e-")) throw new Error("unrelated service");
  return value;
}
async function waitReady() {
  await expect.poll(async () => (await service()).state, { timeout: 180_000, intervals: [2000] }).toBe("ready");
}
async function auth(config) {
  const value = await service();
  const response = await admin.put(base, { data: { name: value.name, spec: { ...value.spec, exposure: { ...value.spec.exposure, authentication: config } } }, maxRedirects: 0 });
  if (!response.ok()) throw new Error(`authentication update returned HTTP ${response.status()}`);
}
async function secret(value) {
  // The signed credential transfer rejects a generation that the asynchronous
  // provider has not observed yet; bounded retries preserve that check.
  await expect.poll(async () => {
    const response = await admin.put(`${base}/load-balancer/secrets/oidc-client-secret`, { data: { value }, maxRedirects: 0 });
    if (![204, 503].includes(response.status())) throw new Error(`credential write returned HTTP ${response.status()}`);
    return response.status();
  }, { timeout: 30_000, intervals: [1000] }).toBe(204);
}
const config = { issuer_url: oidc.issuer_url, client_id: oidc.client_id, client_secret_ref: "oidc-client-secret", scopes: ["openid", "profile", "email"] };
try {
  if ((await service()).spec.exposure.authentication) {
    await auth(null);
    await waitReady();
  }
  const existingSecrets = await admin.get(`${base}/secrets`);
  if (!existingSecrets.ok()) throw new Error("test credential inventory unavailable");
  if ((await existingSecrets.json()).items.includes("oidc-client-secret")) {
    await expect.poll(async () => {
      const response = await admin.delete(`${base}/load-balancer/secrets/oidc-client-secret`, { maxRedirects: 0 });
      if (![204, 503].includes(response.status())) throw new Error(`credential reset returned HTTP ${response.status()}`);
      return response.status();
    }, { timeout: 30_000, intervals: [1000] }).toBe(204);
  }
  let context = await browser.newContext();
  stage = "anonymous_application";
  const anonymous = await context.request.get(test.web_url, { maxRedirects: 0 });
  expect(anonymous.status()).toBe(200);
  await record("default_no_authentication_returns_application");
  await auth(config);
  stage = "missing_credential";
  await expect.poll(async () => (await context.request.get(test.web_url, { maxRedirects: 0 })).status(), { timeout: 90_000, intervals: [2000] }).toBeGreaterThanOrEqual(500);
  await record("missing_client_secret_blocks_application");
  await secret(oidc.client_secret);
  stage = "policy_acceptance";
  await waitReady();
  const blocked = await admin.delete(`${base}/load-balancer/secrets/oidc-client-secret`, { maxRedirects: 0 });
  expect(blocked.status()).toBe(409);
  await record("attached_client_secret_cannot_be_deleted");
  await context.close();
  for (const [name, profile] of [["desktop", devices["Desktop Chrome"]], ["mobile", devices["Pixel 7"]]]) {
    stage = `real_oidc_login_${name}`;
    context = await browser.newContext(profile);
    const redirect = await context.request.get(test.web_url, { maxRedirects: 0 });
    expect(redirect.status()).toBe(302);
    expect(new URL(redirect.headers().location).pathname).toContain(`/realms/${oidc.realm}/`);
    const page = await context.newPage();
    const destination = `${test.web_url}/index.html?oidc_e2e=${name}`;
    await page.goto(destination);
    await page.locator("#username").fill(oidc.username);
    await page.locator("#password").fill(oidc.password);
    await page.locator("#kc-login").click();
    await expect(page.getByRole("heading", { name: "HeteroCloud OIDC E2E" })).toBeVisible({ timeout: 30_000 });
    expect(page.url() === destination).toBe(true);
    const cookies = (await context.cookies(test.web_url)).filter(c => /^Hc(Access|Id)Token-/.test(c.name));
    expect(cookies.length).toBeGreaterThan(0);
    expect(cookies.every(c => c.secure && c.httpOnly && c.sameSite === "Lax" && !c.domain.startsWith("."))).toBe(true);
    await record(`real_oidc_login_returns_original_url_${name}`);
    await context.request.get(`${test.web_url}/_heterocloud/oidc/logout`, { maxRedirects: 0 });
    expect((await context.cookies(test.web_url)).filter(c => /^Hc(Access|Id)Token-/.test(c.name)).length).toBe(0);
    await record(`load_balancer_session_logout_${name}`);
    await context.close();
  }
  await auth({ ...config, issuer_url: `${endpoint}/id/realms/hc-oidc-e2e-does-not-exist-${test.nonce}` });
  stage = "invalid_identity_provider";
  context = await browser.newContext();
  await expect.poll(async () => (await context.request.get(test.web_url, { maxRedirects: 0 })).status(), { timeout: 90_000, intervals: [2000] }).toBeGreaterThanOrEqual(500);
  await record("invalid_identity_provider_blocks_application");
  await auth(config);
  stage = "policy_recovery";
  await waitReady();
  await secret(oidc.client_secret);
  await waitReady();
  await record("credential_rewrite_and_policy_recovery_restore_oidc");
  await auth(null);
  stage = "explicit_no_authentication";
  await waitReady();
  await expect.poll(async () => (await context.request.get(test.web_url, { maxRedirects: 0 })).status(), { timeout: 60_000, intervals: [1000] }).toBe(200);
  await record("explicit_no_authentication_restores_public_application");
  await context.close();
  report.passed = true;
  await writeFile(reportPath, JSON.stringify(report, null, 2));
} catch {
  // Playwright diagnostics may contain Authorization headers, cookie values,
  // password fill arguments or an OAuth callback code. Keep only the stage.
  report.failure_stage = stage;
  await writeFile(reportPath, JSON.stringify(report, null, 2));
  console.error(`OIDC E2E failed at ${stage}; credentials are not included in diagnostics`);
  process.exitCode = 1;
} finally {
  await browser.close();
  await admin.dispose();
}
