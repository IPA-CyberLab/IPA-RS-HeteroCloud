import {expect,test} from "@playwright/test";

test("diagnostics, revision rollback and scheduled backups share the service console",async({page})=>{
  const time="2026-10-20T00:00:00Z";
  const errors:string[]=[];
  const mutations:string[]=[];
  let service={id:"service-1",organization_id:"org-1",project_id:"project-1",provider:"flash",name:"workspace",generation:3,state:"updating",created_at:time,updated_at:time,
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
    else if(path.endsWith("/logs")){
      const query = new URL(request.url()).searchParams;
      const pod = query.get("pod");
      body = {items:[{name:"pod-a",phase:"Running",ready:true},{name:"pod-b",phase:"Failed",ready:false}],pod,previous:query.get("previous")==="true",text:pod?`<script>${pod}</script>`:null,truncated:query.get("previous")==="true"};
    }
    else if(path.endsWith("/flash/services/service-1/ssh-keys"))body={key_ids:[]};
    else if(path.endsWith("/flash/ssh-keys"))body={items:[]};
    else if(path.endsWith("/flash/ssh-info"))body={enabled:true,host_key_fingerprint:"SHA256:test",tcp:{host:"ssh.example.test",port:2222}};
    else if(path.endsWith("/flash/services/service-1")){
      if(request.method()==="PUT"){
        const input=request.postDataJSON();
        expect(input.spec.ssh_enabled).toBe(true);
        mutations.push("ssh-enabled");
        service={...service,spec:input.spec};
      }
      body=service;
    }
    else if(path.endsWith("/flash/quota"))body={max_services:100,max_replicas_per_service:100,max_cpu_millis_per_vm:4000,max_memory_mib_per_vm:8128,max_disk_gib_per_vm:10,max_total_replicas:100,max_total_cpu_millis:20000,max_total_memory_mib:32768,max_total_disk_gib:100,max_weekly_cpu_millicore_seconds:3245760000,max_weekly_memory_mib_seconds:2836280317,max_weekly_gpu_seconds:40320};
    else if(path.endsWith("/domains")||path.endsWith("/flash/backups")||path.endsWith("/flash/backup-destinations")||path.endsWith("/vpc/networks")||path.endsWith("/events"))body={items:[]};
    else return route.fulfill({status:404,json:{error:{code:"not_found",message:path}}});
    return route.fulfill({contentType:"application/json",body:JSON.stringify(body)});
  });
  const start=Date.now();
  await page.goto("/flash/services/service-1");
  await expect(page.getByText("永続ディスクの使用率が90%以上です。")).toBeVisible({timeout:3000});
  expect(Date.now()-start).toBeLessThan(3000);
  await expect(page.getByText(/90.00 \/ 100.00 GiB/)).toBeVisible();
  await page.getByRole("tab",{name:"コンテナログ",exact:true}).click();
  await expect(page.getByText("<script>pod-a</script>")).toBeVisible();
  await page.getByLabel("ログを表示するコンテナ").selectOption("pod-b");
  await expect(page.getByText("<script>pod-b</script>")).toBeVisible();
  await page.getByLabel("前回終了したコンテナのログ").check();
  await expect(page.getByText("ログの一部のみを表示しています。")).toBeVisible();
  await page.getByRole("tab",{name:"更新履歴",exact:true}).click();
  await expect(page.getByRole("button",{name:"世代 1 に切り戻す"})).toHaveCount(0);
  await expect(page.getByRole("button",{name:"公開鍵を登録"})).toHaveCount(0);
  await page.getByRole("button",{name:"編集",exact:true}).click();
  let editor=page.getByRole("dialog",{name:"Flashサービスを編集"});
  await expect(editor.getByRole("button",{name:"公開鍵を登録"})).toBeVisible();
  await editor.getByRole("checkbox",{name:"SSH を有効にする"}).check();
  await editor.getByRole("button",{name:"変更を保存"}).click();
  await expect.poll(()=>mutations).toEqual(["ssh-enabled"]);
  await expect(page.getByText(/ssh -p 2222 .*ssh.example.test/)).toBeVisible();
  await page.getByRole("button",{name:"編集",exact:true}).click();
  editor=page.getByRole("dialog",{name:"Flashサービスを編集"});
  await editor.getByRole("tab",{name:"更新履歴",exact:true}).click();
  await editor.getByRole("button",{name:"世代 1 に切り戻す"}).click();
  expect(mutations).toEqual(["ssh-enabled"]);
  await expect(page.getByText(/永続データは過去の状態に戻りません/)).toBeVisible();
  await page.getByRole("button",{name:"切り戻す",exact:true}).click();
  await expect.poll(()=>mutations).toEqual(["ssh-enabled","rollback"]);
  await editor.getByRole("tab",{name:"定期バックアップ",exact:true}).click();
  await editor.getByRole("checkbox",{name:"定期バックアップを有効にする"}).check();
  await editor.getByRole("button",{name:"バックアップ設定を保存"}).click();
  await expect(page.getByText("バックアップ設定を保存しました。")).toBeVisible();
  expect(mutations).toEqual(["ssh-enabled","rollback","backup-policy"]);
  expect(errors).toEqual([]);
});
