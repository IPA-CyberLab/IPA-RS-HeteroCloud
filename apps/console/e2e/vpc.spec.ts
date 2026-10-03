import { expect, test } from "@playwright/test";

test("VPC private defaults, NAT, rules and deletion work in the console", async ({page}, testInfo) => {
  const timestamp = "2026-10-03T00:00:00Z";
  const id = "00000000-0000-0000-0000-000000000010";
  let network: Record<string, any> | null = null;
  const writes: any[] = [];
  await page.route("**/api/v1/**", async route => {
    const path = new URL(route.request().url()).pathname;
    let body: unknown;
    if (path.endsWith("/auth/session")) body = {csrf_token:"test-csrf",user:{id:"user-test",email:"test@example.test",display_name:"Test"},memberships:[{organization_id:"org-test",organization_slug:"example",organization_name:"Example",principal_id:"principal-test",role:"owner"}]};
    else if (path.endsWith("/projects")) body = {items:[{id:"project-test",organization_id:"org-test",slug:"test",name:"Example Project",created_at:timestamp}]};
    else if (path.endsWith("/flash/services")) body = {items:[]};
    else if (path.endsWith("/vpc/networks")) {
      if (route.request().method() === "POST") {
        const input=route.request().postDataJSON(); writes.push(input);
        network={...input,id,organization_id:"org-test",provider:"vpc",state:"ready",generation:1,status:{observation:"current",status:{phase:"ready",dns_suffix:"hc-vpc-example.svc.cluster.local"}},created_at:timestamp,updated_at:timestamp};
        body=network;
      } else body={items:network ? [network] : []};
    } else if (path.endsWith(`/vpc/networks/${id}`)) {
      if (route.request().method() === "PUT") {const input=route.request().postDataJSON();writes.push(input);network={...network,...input};body=network;}
      else if (route.request().method() === "DELETE") {body={...network,state:"deleting"};network=null;}
      else body=network;
    } else return route.fulfill({status:404,json:{error:{code:"not_found",message:path}}});
    return route.fulfill({json:body});
  });
  const started = Date.now();
  await page.goto("/vpc/networks");
  await expect(page.getByRole("heading",{name:"VPC",exact:true})).toBeVisible();
  expect(Date.now() - started).toBeLessThan(3000);
  await page.getByRole("button",{name:"VPCを作成",exact:true}).click();
  const dialog=page.getByRole("dialog",{name:"VPCを作成"});
  await expect(dialog.getByRole("checkbox",{name:"有効にする"})).not.toBeChecked();
  await dialog.getByRole("textbox",{name:"名前",exact:true}).fill("Coder network");
  await dialog.getByRole("button",{name:/プロジェクト/}).click();
  await page.getByRole("option",{name:"Example Project"}).click();
  await dialog.getByRole("textbox",{name:"セキュリティグループ",exact:true}).fill("coder\nworkspaces");
  await dialog.getByRole("button",{name:"保存",exact:true}).click();
  await expect.poll(() => writes.length).toBe(1);
  expect(writes[0].spec.nat.enabled).toBe(false);
  expect(writes[0].spec.rules).toEqual([]);
  await expect(page.getByRole("heading",{name:"Coder network",exact:true})).toBeVisible();
  await page.getByRole("button",{name:"編集",exact:true}).click();
  const edit=page.getByRole("dialog",{name:"VPCを編集"});
  await edit.getByRole("checkbox",{name:"有効にする"}).check();
  await edit.getByRole("button",{name:"ルールを追加",exact:true}).click();
  await edit.getByRole("button",{name:/接続先/}).click();
  await page.getByRole("option",{name:"グループ: workspaces"}).click();
  await edit.getByRole("spinbutton",{name:"開始ポート"}).fill("22");
  await edit.getByRole("button",{name:"保存",exact:true}).click();
  await expect.poll(() => writes.length).toBe(2);
  expect(writes[1].spec.nat.enabled).toBe(true);
  expect(writes[1].spec.rules[0]).toMatchObject({source:{type:"security_group",name:"coder"},destination:{type:"security_group",name:"workspaces"},protocol:"tcp",port:22});
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth+1)).toBe(true);
  await page.screenshot({path:testInfo.outputPath("vpc.png"),fullPage:true});
  await page.getByRole("button",{name:"削除",exact:true}).click();
  await page.getByRole("dialog",{name:"VPCを削除"}).getByRole("button",{name:"削除する",exact:true}).click();
  await expect(page.getByText("VPCがありません。",{exact:true})).toBeVisible();
});
