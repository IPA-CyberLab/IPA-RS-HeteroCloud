import { Alert } from "@/components/ui";
import { Box } from "@/components/ui";
import { Button } from "@/components/ui";
import { SpaceBetween } from "@/components/ui";
import { Toggle } from "@/components/ui";
import {useQuery} from "@tanstack/react-query";
import {useState} from "react";
import {PageHeader} from "@/components/shared/page-header";
import {RouterLink} from "@/components/shared/router-link";
import {useActiveOrganization} from "@/features/organizations/organization-context";
import {api,getApiErrorMessage} from "@/lib/api-client";
import {formatDateTime} from "@/lib/utils";
import {diagnosticText} from "./flash-operations-panel";

export function FlashAlertsPage({owner=false}:{owner?:boolean}) {
  const {activeOrganization}=useActiveOrganization();
  const org=activeOrganization.organization_id;
  const [includeResolved,setIncludeResolved]=useState(false);
  const query=useQuery({queryKey:owner?["owner","flash","events"]:["organizations",org,"flash","events"],
    queryFn:({signal})=>owner?api.flash.operations.ownerAlerts(signal):api.flash.operations.alerts(org,signal),refetchInterval:15000});
  const items=query.data?.items.filter(e=>includeResolved||!e.resolved_at)??[];
  return <SpaceBetween size="m">
    <PageHeader description="リソース不足、起動失敗、更新と復旧の状態を確認できます。" title={owner?"全サービスの通知":"Flashの通知"} actions={<Button loading={query.isFetching} onClick={()=>void query.refetch()}>更新</Button>}/>
    <Toggle checked={includeResolved} onChange={({detail})=>setIncludeResolved(detail.checked)}>解消済みの通知も表示する</Toggle>
    {query.isError&&<Alert type="error">{getApiErrorMessage(query.error)}</Alert>}
    {query.isPending&&<Box>通知を取得しています。</Box>}
    {query.isSuccess&&items.length===0&&<Box>現在の通知はありません。</Box>}
    {items.map(e=><Alert key={e.id} type={e.resolved_at?"info":e.severity} header={formatDateTime(e.created_at)}>
      {diagnosticText(e.code,e.message)}<br/>
      {owner?<Box>サービスID: {e.service_instance_id}</Box>:<RouterLink to={"/flash/services/"+e.service_instance_id}>サービスの状態を開く</RouterLink>}
    </Alert>)}
  </SpaceBetween>;
}
