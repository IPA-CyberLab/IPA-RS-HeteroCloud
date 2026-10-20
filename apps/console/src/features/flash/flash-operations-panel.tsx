import Alert from "@cloudscape-design/components/alert";
import Box from "@cloudscape-design/components/box";
import Button from "@cloudscape-design/components/button";
import Container from "@cloudscape-design/components/container";
import FormField from "@cloudscape-design/components/form-field";
import Header from "@cloudscape-design/components/header";
import Input from "@cloudscape-design/components/input";
import Modal from "@cloudscape-design/components/modal";
import SpaceBetween from "@cloudscape-design/components/space-between";
import Tabs from "@cloudscape-design/components/tabs";
import Toggle from "@cloudscape-design/components/toggle";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { api, getApiErrorMessage } from "@/lib/api-client";
import type { FlashBackupPolicyInput, FlashService } from "@/lib/api-types";
import { formatDateTime } from "@/lib/utils";
import { flashProviderStatus } from "./flash-service-utils";

const reasonLabels: Record<string,string> = {
  insufficient_disk_capacity:"ホストの空きディスクが不足しています。",
  insufficient_replicated_storage:"永続ディスクの全レプリカを保存する空き容量が不足しています。",
  insufficient_compute_capacity:"CPUまたはメモリに空きのある配置先を待っています。",
  insufficient_inodes:"ホスト上で作成できるファイル数が不足しています。",
  capacity_metrics_unavailable:"容量を確認できないため、起動を再試行しています。",
  image_capacity_unknown:"イメージの容量を確認しています。",
  oom_killed:"メモリ上限を超えてコンテナが終了しました。",
  oom_recovered:"メモリ上限による終了後、コンテナが再起動しました。",
  image_pull_failed:"イメージを取得できません。指定とレジストリへのアクセスを確認してください。",
  container_start_failed:"コンテナの起動に失敗しているか、終了を繰り返しています。",
  vpc_pending:"VPCの通信ルールと名前解決の準備を待っています。",
  disk_full:"永続ディスクの使用率が95%以上です。不要なファイルの削除かディスク拡張が必要です。",
  disk_usage_critical:"永続ディスクの使用率が90%以上です。",
  disk_usage_high:"永続ディスクの使用率が80%以上です。",
  deployment_failed:"更新を完了できませんでした。更新履歴とコンテナの状態を確認してください。",
  deployment_rolled_back:"新版が正常になるまでの制限時間を超えたため、直前の正常な実行設定へ戻しました。",
  provider_unavailable:"現在の状態を取得できません。最後に取得した状態を表示しています。",
};
export const diagnosticText = (code: string, fallback: string) => reasonLabels[code] ?? fallback;

export function FlashOperationsPanel({organizationId,service}:{organizationId:string;service:FlashService}) {
  const [tab,setTab]=useState("health");
  const [rollback,setRollback]=useState<number|null>(null);
  const [draft,setDraft]=useState<FlashBackupPolicyInput|null>(null);
  const client=useQueryClient();
  const base=["organizations",organizationId,"flash","services",service.id];
  const history=useQuery({queryKey:[...base,"revisions"],queryFn:({signal})=>api.flash.operations.revisions(organizationId,service.id,signal),enabled:tab==="revisions",refetchInterval:tab==="revisions"?15000:false});
  const events=useQuery({queryKey:[...base,"events"],queryFn:({signal})=>api.flash.operations.events(organizationId,service.id,signal),enabled:tab==="events",refetchInterval:tab==="events"?15000:false});
  const policy=useQuery({queryKey:[...base,"backup-policy"],queryFn:({signal})=>api.flash.operations.backupPolicy(organizationId,service.id,signal),enabled:tab==="backup-policy",refetchInterval:tab==="backup-policy"?15000:false});
  const save=useMutation({mutationFn:(input:FlashBackupPolicyInput)=>api.flash.operations.putBackupPolicy(organizationId,service.id,input),onSuccess:()=>{setDraft(null);void client.invalidateQueries({queryKey:[...base,"backup-policy"]});}});
  const revert=useMutation({mutationFn:(generation:number)=>api.flash.operations.rollback(organizationId,service.id,generation,service.generation),onSuccess:(value)=>{client.setQueryData(base,value);setRollback(null);void client.invalidateQueries({queryKey:base});}});
  const status=flashProviderStatus(service.status);
  const disk=status.disk_usage;
  const diagnostics=status.diagnostics??[];
  const settings=draft??policy.data??{enabled:false,interval_seconds:86400,retention_count:2,verify_restore:true};
  const change=(key:keyof FlashBackupPolicyInput,value:number|boolean)=>setDraft({...settings,[key]:value});
  return <Container header={<Header variant="h2">状態・更新・バックアップ設定</Header>}>
    <Tabs activeTabId={tab} onChange={({detail})=>setTab(detail.activeTabId)} tabs={[
      {id:"health",label:"状態と使用量",content:<SpaceBetween size="m">
        {diagnostics.map((d,index)=><Alert key={d.code+"/"+index} type={d.severity}>{diagnosticText(d.code,d.message)}</Alert>)}
        {!diagnostics.length&&<Box>現在報告されているリソース・起動の警告はありません。</Box>}
        {status.allocated_memory_mib!==undefined&&<Box>稼働中の全世代の割り当て: CPU {((status.allocated_cpu_millis??0)/1000).toFixed(2)} コア / メモリ {(status.allocated_memory_mib/1024).toFixed(2)} GiB / GPU {status.allocated_gpu_count??0} 個</Box>}
        {disk?<Box>永続ディスク実使用量: {(disk.used_bytes/2**30).toFixed(2)} / {(disk.capacity_bytes/2**30).toFixed(2)} GiB（{(disk.used_bytes/disk.capacity_bytes*100).toFixed(1)}%）<br/>空き: {(disk.available_bytes/2**30).toFixed(2)} GiB・取得日時: {formatDateTime(disk.measured_at)}</Box>:<Box>永続ディスク実使用量: 取得できません。停止中または測定情報がない場合は表示されません。</Box>}
        <Box>更新方式: {service.spec.deployment?.stateless?"永続ホームなし・段階的更新":"永続ホームを保持・停止して更新"} / 自動切り戻し: {service.spec.deployment?.auto_rollback===false?"無効":"有効"}</Box>
      </SpaceBetween>},
      {id:"events",label:"通知履歴",content:<SpaceBetween size="m">
        {events.isError&&<Alert type="error">{getApiErrorMessage(events.error)}</Alert>}
        {events.isPending&&<Box>通知を取得しています。</Box>}
        {events.data?.items.map(e=><Alert key={e.id} type={e.resolved_at?"info":e.severity} header={(e.resolved_at?"解消済み":"通知")+"・"+formatDateTime(e.created_at)}>{diagnosticText(e.code,e.message)}</Alert>)}
        {events.data?.items.length===0&&<Box>通知履歴はありません。</Box>}
      </SpaceBetween>},
      {id:"revisions",label:"更新履歴",content:<SpaceBetween size="m">
        {history.isError&&<Alert type="error">{getApiErrorMessage(history.error)}</Alert>}
        {history.isPending&&<Box>更新履歴を取得しています。</Box>}
        {history.data?.items.map(r=><Box key={r.generation}>世代 {r.generation}・{r.phase==="ready"?"正常確認済み":r.phase==="rolled_back"?"切り戻し済み":r.phase==="failed"?"失敗":"確認中"}・{formatDateTime(r.created_at)} {r.phase==="ready"&&r.generation<service.generation&&<Button onClick={()=>setRollback(r.generation)}>世代 {r.generation} に切り戻す</Button>}</Box>)}
        <Modal visible={rollback!==null} onDismiss={()=>!revert.isPending&&setRollback(null)} header="正常な世代へ切り戻す" footer={<Button variant="primary" loading={revert.isPending} onClick={()=>rollback!==null&&revert.mutate(rollback)}>切り戻す</Button>}>
          <SpaceBetween size="m"><Box>世代 {rollback} の実行設定へ戻します。現在のアクセス制御・シークレット設定と永続ディスクの内容は保持します。永続データは過去の状態に戻りません。</Box>{revert.isError&&<Alert type="error">{getApiErrorMessage(revert.error)}</Alert>}</SpaceBetween>
        </Modal>
      </SpaceBetween>},
      {id:"backup-policy",label:"定期バックアップ",content:<SpaceBetween size="m">
        {policy.isError&&<Alert type="error">{getApiErrorMessage(policy.error)}</Alert>}
        <Toggle checked={settings.enabled} disabled={policy.isPending||save.isPending} onChange={({detail})=>change("enabled",detail.checked)}>定期バックアップを有効にする</Toggle>
        <FormField label="取得間隔（時間）" description="1〜168時間"><Input ariaLabel="バックアップ取得間隔" type="number" value={String(settings.interval_seconds/3600)} onChange={({detail})=>change("interval_seconds",Number(detail.value)*3600)}/></FormField>
        <FormField label="保存世代数" description="1〜8世代。手動バックアップは世代整理の対象になりません。"><Input ariaLabel="バックアップ保存世代数" type="number" value={String(settings.retention_count)} onChange={({detail})=>change("retention_count",Number(detail.value))}/></FormField>
        <Toggle checked={settings.verify_restore} onChange={({detail})=>change("verify_restore",detail.checked)}>保存後に別ディスクへ復元して読み取りを検証する</Toggle>
        {policy.data?.last_error&&<Alert type="warning">定期バックアップを完了できませんでした。権限、バックアップの容量上限、保存元の状態を確認してください。</Alert>}
        {policy.data?.enabled&&<Box>次回予定: {formatDateTime(policy.data.next_run_at)}</Box>}
        <Box>古い世代は新しいバックアップの保存・検証が完了してから整理します。バックアップの容量上限は手動分と共有します。</Box>
        <Button variant="primary" loading={save.isPending} disabled={policy.isPending||policy.isError||settings.interval_seconds<3600||settings.interval_seconds>604800||!Number.isInteger(settings.interval_seconds)||settings.retention_count<1||settings.retention_count>8||!Number.isInteger(settings.retention_count)} onClick={()=>save.mutate(settings)}>バックアップ設定を保存</Button>
        {save.isSuccess&&<Alert type="success">バックアップ設定を保存しました。</Alert>}{save.isError&&<Alert type="error">{getApiErrorMessage(save.error)}</Alert>}
      </SpaceBetween>},
    ]}/>
  </Container>;
}
