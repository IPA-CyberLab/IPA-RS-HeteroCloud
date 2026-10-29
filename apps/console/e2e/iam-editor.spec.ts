import { expect, test } from "@playwright/test";

test("IAM policy actions and resources are added individually and saved", async ({
  page,
}) => {
  let submitted: any;
  const timestamp = "2026-10-01T00:00:00Z";
  await page.route("**/api/v1/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path.endsWith("/auth/session"))
      return route.fulfill({
        json: {
          csrf_token: "test",
          user: { id: "u", email: "test@example.test", display_name: "Test" },
          memberships: [
            {
              organization_id: "org",
              organization_slug: "org",
              organization_name: "Example",
              principal_id: "p",
              role: "owner",
            },
          ],
        },
      });
    if (path.endsWith("/iam/policies")) {
      if (route.request().method() === "POST") {
        submitted = route.request().postDataJSON();
        return route.fulfill({
          json: {
            ...submitted,
            id: "policy-1",
            organization_id: "org",
            semantics_digest: "0123456789abcdef",
            created_at: timestamp,
            updated_at: timestamp,
          },
        });
      }
      return route.fulfill({ json: { items: [] } });
    }
    return route.fulfill({
      status: 404,
      json: { error: { code: "not_found", message: path } },
    });
  });
  await page.goto("/iam/policies");
  await page.getByRole("button", { name: "ポリシーを作成" }).click();
  const dialog = page.getByRole("dialog", { name: "IAMポリシーを作成" });
  await dialog.getByLabel("ポリシー名").fill("FlowReadOnly");
  const action = dialog.getByLabel("アクションを追加");
  await action.fill("flow:ListInstances");
  await dialog.getByLabel("リソースを追加").focus();
  await expect(
    dialog.getByRole("button", { name: "flow:ListInstances を削除" }),
  ).toBeVisible();
  await action.fill("flow:GetInstance");
  await action.press("Enter");
  await dialog.getByRole("button", { name: "flow:GetInstance を削除" }).click();
  const resource = dialog.getByLabel("リソースを追加");
  await resource.fill("hc:org:*:flow/*");
  await resource.press("Enter");
  await dialog.getByRole("button", { name: "作成", exact: true }).click();
  await expect.poll(() => submitted?.name).toBe("FlowReadOnly");
  expect(submitted.document.statements).toEqual([
    {
      effect: "Allow",
      actions: ["flow:ListInstances"],
      resources: ["hc:org:*:flow/*"],
    },
  ]);
  await expect(dialog).not.toBeVisible();
});
