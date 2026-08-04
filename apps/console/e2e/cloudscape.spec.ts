import { expect, type Page, type Route, test } from "@playwright/test";

const organizationId = "org-heterocloud";
const projectId = "project-flow";
const serviceId = "flow-production";
const timestamp = "2026-08-04T07:00:00Z";

const service = {
  id: serviceId,
  organization_id: organizationId,
  project_id: projectId,
  provider: "flow",
  name: "flow-production",
  generation: 3,
  state: "ready",
  spec: {
    region: "heteronet-global",
    max_participants: 500,
    max_rooms: 1_000,
    rate_limit: { requests_per_second: 40, burst: 80 },
    metadata: {},
  },
  status: {},
  created_at: timestamp,
  updated_at: timestamp,
};

const endpoints = {
  api: ["https://flow.heterocloud.mizuame.app"],
  signaling: ["wss://flow.heterocloud.mizuame.app"],
  livekit: ["wss://flow.heterocloud.mizuame.app"],
  stun: ["stun:flow.heterocloud.mizuame.app:3478"],
  turn: ["turn:flow.heterocloud.mizuame.app:3478?transport=udp"],
};

const metrics = {
  active_rooms: 12,
  concurrent_connections: 48,
  ingress_bytes: 8_940_000,
  egress_bytes: 8_830_000,
  transferred_bytes: 17_770_000,
  measured_at: timestamp,
  sfu_participants: 40,
  p2p_connections: 8,
  room_limit: 1_000,
  endpoints,
};

function json(route: Route, body: unknown) {
  return route.fulfill({
    status: 200,
    contentType: "application/json",
    body: JSON.stringify(body),
  });
}

async function mockApi(page: Page) {
  await page.route("**/api/v1/**", async (route) => {
    const path = new URL(route.request().url()).pathname.replace("/api/v1", "");

    if (path === "/auth/session") {
      return json(route, {
        user: {
          id: "user-owner",
          email: "owner@heterocloud.example",
          display_name: "Cloud Owner",
          status: "active",
          created_at: timestamp,
        },
        memberships: [
          {
            organization_id: organizationId,
            organization_slug: "heterocloud",
            organization_name: "HeteroCloud Lab",
            principal_id: "principal-owner",
            role: "owner",
          },
        ],
        csrf_token: "e2e-csrf-token",
      });
    }
    if (path === "/organizations") {
      return json(route, {
        items: [{ id: organizationId, slug: "heterocloud", name: "HeteroCloud Lab", created_at: timestamp }],
      });
    }
    if (path === `/organizations/${organizationId}/projects`) {
      return json(route, {
        items: [{ id: projectId, organization_id: organizationId, slug: "flow", name: "Flow", created_at: timestamp }],
      });
    }
    if (path === `/organizations/${organizationId}/iam/principals`) {
      return json(route, {
        items: [{
          id: "principal-owner",
          organization_id: organizationId,
          kind: "user",
          name: "Cloud Owner",
          user_id: "user-owner",
          enabled: true,
          created_at: timestamp,
        }],
      });
    }
    if (path === `/organizations/${organizationId}/iam/policies`) {
      return json(route, { items: [] });
    }
    if (path === `/organizations/${organizationId}/audit-events`) {
      return json(route, {
        items: [{
          id: 1,
          occurred_at: timestamp,
          organization_id: organizationId,
          principal_id: "principal-owner",
          user_id: "user-owner",
          request_id: "request-e2e",
          source_ip: "10.250.0.4",
          action: "flow.service.read",
          resource: `flow-service:${serviceId}`,
          decision: "allow",
          reason: "policy matched",
          metadata: {},
        }],
      });
    }
    if (path === `/organizations/${organizationId}/realtime/services`) {
      return json(route, { items: [service] });
    }
    if (path === `/organizations/${organizationId}/realtime/services/${serviceId}/metrics`) {
      return json(route, metrics);
    }
    if (path === `/organizations/${organizationId}/projects/${projectId}/realtime/services/${serviceId}/metrics/history`) {
      return json(route, {
        range: "24h",
        step_seconds: 900,
        samples: [
          { sampled_at: "2026-08-04T06:30:00Z", active_rooms: 8, concurrent_connections: 31, ingress_bytes: 7_800_000, egress_bytes: 7_700_000, transferred_bytes: 15_500_000 },
          { sampled_at: "2026-08-04T06:45:00Z", active_rooms: 10, concurrent_connections: 40, ingress_bytes: 8_200_000, egress_bytes: 8_100_000, transferred_bytes: 16_300_000 },
          { sampled_at: timestamp, active_rooms: 12, concurrent_connections: 48, ingress_bytes: 8_940_000, egress_bytes: 8_830_000, transferred_bytes: 17_770_000 },
        ],
      });
    }
    if (path === `/organizations/${organizationId}/realtime/services/${serviceId}/developer-credentials`) {
      return json(route, { items: [] });
    }
    if (path === `/organizations/${organizationId}/realtime/services/${serviceId}/access-contexts`) {
      return json(route, { items: [] });
    }
    if (path === `/organizations/${organizationId}/realtime/services/${serviceId}`) {
      return json(route, service);
    }

    return route.fulfill({
      status: 404,
      contentType: "application/json",
      body: JSON.stringify({ error: { code: "e2e_unhandled", message: `Unhandled ${path}` } }),
    });
  });
}

function collectBrowserErrors(page: Page) {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("console", (message) => {
    if (message.type() === "error") errors.push(message.text());
  });
  return errors;
}

async function expectNoPageOverflow(page: Page) {
  await expect
    .poll(() =>
      page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1),
    )
    .toBe(true);
}

test.beforeEach(async ({ page }) => {
  await mockApi(page);
});

test("概要から Flow 詳細まで操作でき、グラフと接続先を表示する", async ({ page, isMobile }, testInfo) => {
  test.skip(Boolean(isMobile), "デスクトップ操作の検証");
  const browserErrors = collectBrowserErrors(page);
  await page.goto("/overview");

  await expect(page.getByRole("heading", { name: "コンソールホーム" })).toBeVisible();
  await expect(page.getByText("HeteroCloud Lab のリソース、稼働状況、最近の操作です。")).toBeVisible();
  await page.getByRole("link", { name: "Flow", exact: true }).first().click();
  await expect(page).toHaveURL(/\/flow\/services$/);
  await expect(page.getByRole("heading", { name: "Flow", exact: true })).toBeVisible();

  const serviceRow = page.getByRole("row", { name: /flow-production/ });
  await serviceRow.getByRole("link", { name: "flow-productionの詳細を開く" }).click();
  await expect(page).toHaveURL(new RegExp(`/flow/services/${serviceId}$`));
  await expect(page.getByRole("heading", { name: "flow-production" })).toBeVisible();
  await expect(page.getByRole("application", { name: "アクティブルームの推移" })).toBeVisible();
  await expect(page.getByRole("application", { name: "転送量 / 時間の推移" })).toBeVisible();
  await expect(page.getByText("stun:flow.heterocloud.mizuame.app:3478")).toBeVisible();
  await expect(page.getByText("turn:flow.heterocloud.mizuame.app:3478?transport=udp")).toBeVisible();
  await expectNoPageOverflow(page);

  await page.screenshot({
    path: testInfo.outputPath("flow-detail.png"),
    fullPage: true,
  });
  expect(browserErrors).toEqual([]);
});

test("モバイルでもナビゲーションと Flow 一覧を操作できる", async ({ page, isMobile }, testInfo) => {
  test.skip(!isMobile, "モバイル操作の検証");
  const browserErrors = collectBrowserErrors(page);
  await page.goto("/flow/services");

  await expect(page.getByRole("heading", { name: "Flow", exact: true })).toBeVisible();
  await expect(page.getByText("flow-production", { exact: true }).first()).toBeVisible();
  await expect(page.getByRole("columnheader", { name: "状態" })).toBeVisible();
  await expect(page.getByRole("columnheader", { name: "同時接続" })).toBeVisible();
  await expect(page.getByRole("columnheader", { name: "プロジェクト" })).toHaveCount(0);
  await expectNoPageOverflow(page);

  await page.screenshot({
    path: testInfo.outputPath("flow-mobile.png"),
    fullPage: true,
  });

  const navigationToggle = page.getByRole("button", { name: "ナビゲーションを開く" });
  if (await navigationToggle.isVisible()) {
    await navigationToggle.click();
    await expect(page.getByRole("link", { name: "プロジェクト" })).toBeVisible();
    await page.screenshot({
      path: testInfo.outputPath("navigation-mobile.png"),
      fullPage: true,
    });
  }
  expect(browserErrors).toEqual([]);
});
