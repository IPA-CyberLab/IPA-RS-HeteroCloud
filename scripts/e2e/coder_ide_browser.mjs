// A visible shell alone is insufficient: load a folder, open a file and reload.
// Configuration and the temporary Coder session token arrive only through stdin.
import { createRequire } from 'node:module';
import { open } from 'node:fs/promises';
import { constants } from 'node:fs';
const require = createRequire(new URL('../../apps/console/package.json', import.meta.url));
const { chromium, expect } = require('@playwright/test');
let input = '';
for await (const chunk of process.stdin) {
  input += chunk;
  if (input.length > 65536) throw new Error('Configuration too large');
}
let config, url;
try { config = JSON.parse(input); url = new URL(config.url); }
catch { throw new Error('Invalid JSON configuration or URL; input withheld'); }
const folder = url.searchParams.get('folder');
if (!['https:', 'http:'].includes(url.protocol) || url.username || url.password || url.hash
    || !folder?.startsWith('/') || !url.pathname.endsWith('/apps/code-server/')
    || [...url.searchParams.keys()].some(key => key !== 'folder')
    || typeof config.session_token !== 'string' || !config.session_token
    || typeof config.file !== 'string' || !/^[^/\\\0]+$/.test(config.file)
    || typeof config.report_path !== 'string' || !config.report_path) {
  throw new Error('Provide an explicit app URL, folder, existing root file, report path and stdin token');
}
const timeout = config.timeout_ms ?? 30000;
if (!Number.isInteger(timeout) || timeout < 1000 || timeout > 60000) {
  throw new Error('Readiness timeout must be 1000..60000 milliseconds');
}
const escape = value => value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
const folderName = folder.split('/').filter(Boolean).at(-1);
if (!folderName) throw new Error('Select a named project folder');
const report = { schema_version: 1, observed_at: new Date().toISOString(),
  passed: false, checks: [], server_errors: [] };
let stage = 'browser_start';
const browser = await chromium.launch();
try {
  const context = await browser.newContext();
  await context.addCookies([{ name: 'coder_session_token', value: config.session_token,
    url: url.origin, secure: url.protocol === 'https:', httpOnly: true, sameSite: 'Lax' }]);
  const page = await context.newPage();
  const sockets = [];
  page.on('response', response => {
    const target = new URL(response.url());
    if (target.origin === url.origin && target.pathname.startsWith(url.pathname)
        && response.status() >= 500) {
      report.server_errors.push({ status: response.status(), path: target.pathname });
    }
  });
  page.on('websocket', socket => {
    const state = { received: 0, sent: 0, closed: false };
    sockets.push(state);
    socket.on('framereceived', () => state.received++);
    socket.on('framesent', () => state.sent++);
    socket.on('close', () => { state.closed = true; });
    socket.on('socketerror', () => { state.closed = true; });
  });
  for (const name of ['folder_and_file', 'reload_existing_folder']) {
    stage = name;
    const started = Date.now();
    const response = name === 'folder_and_file'
      ? await page.goto(url.href, { waitUntil: 'domcontentloaded', timeout })
      : await page.reload({ waitUntil: 'domcontentloaded', timeout });
    if (response.status() !== 200) throw new Error('App HTTP status');
    const row = page.locator('.explorer-viewlet .monaco-list-row')
      .filter({ hasText: new RegExp('^' + escape(config.file) + '$') });
    await expect(row).toBeVisible({ timeout });
    await row.dblclick();
    await expect(page).toHaveTitle(new RegExp(escape(config.file) + '.*' + escape(folderName)), { timeout });
    await expect(page.locator('.editor-group-container.active .view-lines .view-line').first())
      .toBeVisible({ timeout });
    await expect.poll(() => sockets.filter(s => !s.closed && s.received > 0 && s.sent > 0).length,
      { timeout }).toBeGreaterThan(0);
    await page.waitForTimeout(5000);
    if (!sockets.some(s => !s.closed && s.received > 0 && s.sent > 0)) {
      throw new Error('App connection lost after rendering');
    }
    if (report.server_errors.length) throw new Error('App server errors');
    report.checks.push({ name, passed: true, status: response.status(),
      folder_loaded: true, file_opened: true, editor_rendered: true,
      websocket_bidirectional: true, stable_connection_ms: 5000,
      elapsed_ms: Date.now() - started });
    console.log(JSON.stringify(report.checks.at(-1)));
  }
  await context.close();
  report.passed = true;
} catch (error) {
  // Exception text can contain authenticated URLs. Retain bounded metadata only.
  report.failure_stage = stage;
  report.failure_kind = error.name === 'TimeoutError' ? 'timeout' : 'readiness_failed';
  process.exitCode = 1;
} finally {
  await browser.close();
  const file = await open(config.report_path,
    constants.O_WRONLY | constants.O_CREAT | constants.O_TRUNC | constants.O_NOFOLLOW, 0o600);
  try { await file.chmod(0o600); await file.writeFile(JSON.stringify(report, null, 2) + '\n'); }
  finally { await file.close(); }
  console.log(JSON.stringify({ passed: report.passed, checks: report.checks.length,
    failure_stage: report.failure_stage }));
}
