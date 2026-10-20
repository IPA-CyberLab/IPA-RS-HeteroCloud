import {QueryClient,QueryClientProvider} from "@tanstack/react-query";
import {render,screen,waitFor} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import {beforeEach,describe,expect,it,vi} from "vitest";
import {api} from "@/lib/api-client";
import type {FlashService} from "@/lib/api-types";
import {FlashOperationsPanel} from "./flash-operations-panel";
const service={id:"service-1",generation:3,spec:{},status:{status:{diagnostics:[{code:"disk_usage_critical",severity:"error",message:"backend"}],disk_usage:{capacity_bytes:100*2**30,used_bytes:90*2**30,available_bytes:10*2**30,measured_at:"2026-10-20T00:00:00Z"}}}} as unknown as FlashService;
function show(){return render(<QueryClientProvider client={new QueryClient({defaultOptions:{queries:{retry:false},mutations:{retry:false}}})}><FlashOperationsPanel organizationId="org-1" service={service}/></QueryClientProvider>);}
describe("FlashOperationsPanel",()=>{
 beforeEach(()=>{
   vi.spyOn(api.flash.operations,"backupPolicy").mockResolvedValue(null);
   vi.spyOn(api.flash.operations,"events").mockResolvedValue({items:[]});
   vi.spyOn(api.flash.operations,"revisions").mockResolvedValue({items:[{generation:1,service_instance_id:"service-1",name:"first",phase:"ready",created_at:"2026-10-19T00:00:00Z",ready_at:"2026-10-19T00:00:01Z"},{generation:3,service_instance_id:"service-1",name:"latest",phase:"pending",created_at:"2026-10-20T00:00:00Z",ready_at:null}]});
 });
 it("shows measured storage and warnings immediately without fetching hidden panels",()=>{
   show();
   expect(screen.getByText(/永続ディスクの使用率が90%以上/)).toBeInTheDocument();
   expect(screen.getByText(/90.00 \/ 100.00 GiB/)).toBeInTheDocument();
   expect(api.flash.operations.revisions).not.toHaveBeenCalled();
   expect(api.flash.operations.backupPolicy).not.toHaveBeenCalled();
 });
 it("only rolls back a confirmed healthy generation and fences concurrent edits",async()=>{
   const user=userEvent.setup();
   const rollback=vi.spyOn(api.flash.operations,"rollback").mockResolvedValue({...service,generation:4});
   show();
   await user.click(screen.getByRole("tab",{name:"更新履歴"}));
   await user.click(await screen.findByRole("button",{name:"世代 1 に切り戻す"}));
   expect(rollback).not.toHaveBeenCalled();
   expect(screen.getByText(/永続データは過去の状態に戻りません/)).toBeInTheDocument();
   await user.click(screen.getByRole("button",{name:"切り戻す"}));
   await waitFor(()=>expect(rollback).toHaveBeenCalledWith("org-1","service-1",1,3));
 });
 it("persists scheduling and verified retention settings",async()=>{
   const user=userEvent.setup();
   const save=vi.spyOn(api.flash.operations,"putBackupPolicy").mockResolvedValue({service_instance_id:"service-1",enabled:true,interval_seconds:86400,retention_count:2,verify_restore:true,next_run_at:"2026-10-21T00:00:00Z",last_run_at:null,last_error:null,last_backup_id:null});
   show();
   await user.click(screen.getByRole("tab",{name:"定期バックアップ"}));
   const enabled=await screen.findByRole("checkbox",{name:"定期バックアップを有効にする"});
   await waitFor(()=>expect(enabled).not.toBeDisabled());
   await user.click(enabled);
   await user.click(screen.getByRole("button",{name:"バックアップ設定を保存"}));
   await waitFor(()=>expect(save).toHaveBeenCalledWith("org-1","service-1",expect.objectContaining({enabled:true,interval_seconds:86400,retention_count:2,verify_restore:true})));
 });
});
