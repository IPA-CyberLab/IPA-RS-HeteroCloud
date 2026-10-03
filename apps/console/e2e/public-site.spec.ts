import { expect, test } from "@playwright/test";

test("the public homepage is readable without a session API", async ({ page }) => {
  const apiRequests: string[] = [];
  await page.route("**/api/v1/**", async (route) => {
    apiRequests.push(new URL(route.request().url()).pathname);
    await route.fulfill({ status: 503, contentType: "application/json", body: "{}" });
  });

  await page.goto("/");
  await expect(page.getByRole("heading", { level: 1 })).toContainText("分散したリソースを");
  await expect(page).toHaveURL(/\/$/);
  expect(await page.locator("script").count()).toBe(0);
  expect(apiRequests).toEqual([]);
});

test("restoring the console document at / loads the public homepage", async ({ page }) => {
  let sessionRequests = 0;
  await page.route("**/api/v1/auth/session", async (route) => {
    sessionRequests += 1;
    await route.fulfill({ status: 401, contentType: "application/json", body: JSON.stringify({ error: { code: "unauthorized", message: "Authentication is required." } }) });
  });
  await page.goto("/login");
  await expect(page.getByRole("heading", { name: "ログイン", exact: true })).toBeVisible();
  await expect.poll(() => sessionRequests).toBe(1);

  await page.evaluate(() => {
    history.pushState(null, "", "/");
    window.dispatchEvent(new PopStateEvent("popstate"));
  });

  await expect(page).toHaveURL(/\/\?site=public$/);
  await expect(page.getByRole("heading", { level: 1 })).toContainText("分散したリソースを");
  expect(sessionRequests).toBe(1);
});

test("the login page opens a fresh public document", async ({ page }) => {
  await page.route("**/api/v1/auth/session", async (route) => {
    await route.fulfill({ status: 401, contentType: "application/json", body: JSON.stringify({ error: { code: "unauthorized", message: "Authentication is required." } }) });
  });
  await page.goto("/login");
  await page.getByRole("link", { name: "HeteroCloud について" }).click();

  await expect(page).toHaveURL(/\/\?site=public$/);
  await expect(page.getByRole("heading", { level: 1 })).toContainText("分散したリソースを");
});
