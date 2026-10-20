import {expect,test} from "@playwright/test";

test("diagnostics, revision rollback and scheduled backups share the service console",async({page})=>{
  const time="2026-10-20T00:00:00Z";
  const errors:string[]=[];
  const mutations:string[]=[];
  const service={id:"service-1",organization_id:"org-1",project_id:"project-1",provider:"flash",name:"workspace",generation:3,state:"updating",created_at:time,updated_at:time,
    spec:{region:"test",image:"example/workspace:bad",replicas:1,cpu_millis:100,memory_mib:128,ephemeral_storage_gib:2,env:{},command:[],args:[],metadata:{},ports:[],exposure:{type:"internal",traffic_mode:"forwarded"}},
    status:{status:{runtime_class:"gvisor",ready_replicas:0,desired_replicas:1,diagnostics:[{code:"disk_usage_critical",severity:"error",message:"disk usage"}],disk_usage:{capacity_bytes:100*2**30,used_bytes:90*2**30,available_bytes:10*2**30,measured_at:time}}}};
  let policy:unknown=null;
  page.on("pageerror",error=>errors.push(error.message));
  await page.route("**/api/v1/**",async route=>{
    const request=route.request(),path=new URL(request.url()).pathname;
    let body:unknown;
    if(request.method()!=="GET")expect(request.headers()["x-heterocloud-csrf"]).toBe("operations-test-csrf");
    if(path.endsWith("/auth/session"))body={user:{id:"user-1",email:"test@example.test",display_name:"Test",status:"active",created_at:time},memberships:[{organization_id:"org-1",organization_slug:"test",organization_name:"Test",principal_id:"principal-1",role:"owner"}],csrf_token:"operations-test-csrf"};
    else if(path.endsWith("/projects"))body={items:[{id:"project-1",organization_id:"org-1",name:"Test",slug:"test",created_at:time}]};
    else if(path.endsWith("/revisions"))body={items:[{generation:1,name:"workspace",phase:"ready",created_at:time,ready_at:time},{generation:3,name:"workspace",phase:"pending",created_at:time,ready_at:null}]};
    else if(path.endsWith("/rollback")){
      expect(request.postDataJSON()).toEqual({generation:1,expected_generation:3});
      mutations.push("rollback");
      body={...service,generation:4,state:"updating"};
    }
    else if(path.endsWith("/backup-policy")){
      if(request.method()==="PUT"){expect(request.postDataJSON()).toEqual({enabled:true,interval_seconds:86400,retention_count:2,verify_restore:true});mutations.push("backup-policy");policy={...request.postDataJSON(),next_run_at:time};}
      body=policy;
    }
    else if(path.endsWith("/flash/services/service-1"))body=service;
    else if(path.endsWith("/flash/quota"))body={};
    else if(path.endsWith("/domains")||path.endsWith("/flash/backups")||path.endsWith("/vpc/networks")||path.endsWith("/events"))body={items:[]};
    else return route.fulfill({status:404,json:{error:{code:"not_found",message:path}}});
    return route.fulfill({contentType:"application/json",body:JSON.stringify(body)});
  });
  const start=Date.now();
  await page.goto("/flash/services/service-1");
  await expect(page.getByText("永続ディスクの使用率が90%以上です。")).toBeVisible({timeout:3000});
  expect(Date.now()-start).toBeLessThan(3000);
  await expect(page.getByText(/90.00 \/ 100.00 GiB/)).toBeVisible();
  await page.getByRole("tab",{name:"更新履歴",exact:true}).click();
  await page.getByRole("button",{name:"世代 1 に切り戻す"}).click();
  expect(mutations).toEqual([]);
  await expect(page.getByText(/永続データは過去の状態に戻りません/)).toBeVisible();
  await page.getByRole("button",{name:"切り戻す",exact:true}).click();
  await expect.poll(()=>mutations).toEqual(["rollback"]);
  await page.getByRole("tab",{name:"定期バックアップ",exact:true}).click();
  await page.getByRole("checkbox",{name:"定期バックアップを有効にする"}).check();
  await page.getByRole("button",{name:"バックアップ設定を保存"}).click();
  await expect(page.getByText("バックアップ設定を保存しました。")).toBeVisible();
  expect(mutations).toEqual(["rollback","backup-policy"]);
  expect(errors).toEqual([]);
});
