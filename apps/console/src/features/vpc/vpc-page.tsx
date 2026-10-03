import Alert from "@cloudscape-design/components/alert";
import Box from "@cloudscape-design/components/box";
import Button from "@cloudscape-design/components/button";
import ColumnLayout from "@cloudscape-design/components/column-layout";
import Container from "@cloudscape-design/components/container";
import FormField from "@cloudscape-design/components/form-field";
import Header from "@cloudscape-design/components/header";
import Input from "@cloudscape-design/components/input";
import Modal from "@cloudscape-design/components/modal";
import Select from "@cloudscape-design/components/select";
import SpaceBetween from "@cloudscape-design/components/space-between";
import Textarea from "@cloudscape-design/components/textarea";
import Toggle from "@cloudscape-design/components/toggle";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { PageHeader } from "@/components/shared/page-header";
import { PageLoading } from "@/components/shared/page-loading";
import { ErrorState } from "@/components/shared/error-state";
import { ProjectSelector } from "@/components/shared/resource-selectors";
import { useActiveOrganization } from "@/features/organizations/organization-context";
import { api, getApiErrorMessage } from "@/lib/api-client";
import type { VpcNetwork, VpcPeer, VpcRule, VpcSpec } from "@/lib/api-types";
import { vpcsQueryOptions, flashServicesQueryOptions } from "@/lib/queries";

const initial: VpcSpec = {region: "heteronet-global", description: "", nat: {enabled: false}, security_groups: ["default"], rules: []};
const peerValue = (p: VpcPeer) => p.type === "all" ? "all" : p.type === "service" ? `service:${p.service_id}` : `group:${p.name}`;
const parsePeer = (value: string): VpcPeer => value === "all" ? {type: "all"} : value.startsWith("service:") ? {type: "service", service_id: value.slice(8)} : {type: "security_group", name: value.slice(6)};
export function VpcPage() {
  const {activeOrganization} = useActiveOrganization();
  const org = activeOrganization.organization_id;
  const networks = useQuery(vpcsQueryOptions(org));
  const services = useQuery(flashServicesQueryOptions(org));
  const cache = useQueryClient();
  const [edit, setEdit] = useState<VpcNetwork | "new" | null>(null);
  const [deleting, setDeleting] = useState<VpcNetwork | null>(null);
  const [name, setName] = useState("");
  const [project, setProject] = useState("");
  const [spec, setSpec] = useState<VpcSpec>(initial);
  const [groups, setGroups] = useState("default");
  const [validation, setValidation] = useState("");
  const invalidate = () => cache.invalidateQueries({queryKey: ["organizations",org,"vpc"]});
  const save = useMutation({mutationFn: () => {
    const next = {...spec, security_groups: groups.split(/[\s,]+/).filter(Boolean)};
    return edit && edit !== "new" ? api.vpc.update(org, edit.id, {name, spec: next}) : api.vpc.create(org, {project_id: project, name, spec: next});
  }, onSuccess: async () => {setEdit(null); await invalidate();}});
  const remove = useMutation({mutationFn: (id: string) => api.vpc.delete(org,id), onSuccess: async () => {setDeleting(null); await invalidate();}});
  function open(v: VpcNetwork | "new") {save.reset();setValidation("");setEdit(v);setName(v === "new" ? "" : v.name);setProject(v === "new" ? "" : v.project_id);setSpec(v === "new" ? structuredClone(initial) : structuredClone(v.spec));setGroups(v === "new" ? "default" : v.spec.security_groups.join("\n"));}
  function changeRule(index: number, patch: Partial<VpcRule>) {setSpec({...spec, rules: spec.rules.map((r,i) => i === index ? {...r,...patch} : r)});}
  const peerOptions = [
    ...groups.split(/[\s,]+/).filter(Boolean).map(g => ({value: `group:${g}`, label: `グループ: ${g}`})),
    ...(services.data?.items ?? []).filter(s => edit && edit !== "new" && s.spec.network?.vpc_id === edit.id).map(s => ({value: `service:${s.id}`, label: `サービス: ${s.name}`})),
    {value: "all", label: "このVPCの全サービス"},
  ];
  if (networks.isPending) return <PageLoading />;
  if (networks.isError) return <ErrorState description={getApiErrorMessage(networks.error)} />;
  return <SpaceBetween size="l">
    <PageHeader title="VPC" description="Flashサービス間のプライベート通信と、外部へのNATを管理します。" actions={<Button formAction="none" variant="primary" onClick={() => open("new")}>VPCを作成</Button>} />
    <Alert type="info">VPC内の通信は許可ルールが必要です。NATは外向きの接続専用で、サービスを外部へ公開しません。</Alert>
    {networks.data.items.length === 0 && <Box>VPCがありません。</Box>}
    {networks.data.items.map(v => {
      const status = v.status.status;
      const attached = (services.data?.items ?? []).filter(s => s.spec.network?.vpc_id === v.id);
      return <Container key={v.id} header={<Header actions={<SpaceBetween direction="horizontal" size="xs"><Button formAction="none" onClick={() => open(v)}>編集</Button><Button formAction="none" disabled={attached.length > 0 || v.state === "deleting"} onClick={() => {remove.reset();setDeleting(v);}}>削除</Button></SpaceBetween>}>{v.name}</Header>}>
        <SpaceBetween size="m">
          <Box variant="code">{v.id}</Box>
          {v.status.observation === "unavailable" && <Alert type="warning">ネットワークの現在の状態を取得できません。</Alert>}
          <ColumnLayout columns={3}><div>状態: {status?.phase ?? v.state}</div><div>NAT: {v.spec.nat.enabled ? "有効" : "無効"}</div><div>接続サービス: {attached.length}</div></ColumnLayout>
          {status?.dns_suffix && <div>内部DNS: <code>名前.{status.dns_suffix}</code></div>}
          {status?.nat_gateway_node && <div>NATゲートウェイ: {status.nat_gateway_node}（切替時に送信元IPが変わる場合があります）</div>}
          <div>セキュリティグループ: {v.spec.security_groups.join(", ")}</div>
          <div>通信ルール: {v.spec.rules.length}</div>
          {attached.map(s => <div key={s.id}><a href={`/flash/services/${s.id}`}>{s.name}</a> — {s.spec.network?.private_name ?? `f-${s.id.replaceAll("-","")}`} / {s.spec.network?.security_groups.join(", ")}</div>)}
          {attached.length > 0 && <Box variant="small">削除するには、接続しているサービスを先に削除またはVPCから切り離してください。</Box>}
        </SpaceBetween>
      </Container>;
    })}
    <Modal visible={edit !== null} onDismiss={() => setEdit(null)} size="large" header={edit === "new" ? "VPCを作成" : "VPCを編集"}>
      <form onSubmit={e => {e.preventDefault();setValidation("");const groupList = groups.split(/[\s,]+/).filter(Boolean);if(!name.trim() || !project || !groupList.length) {setValidation("名前、プロジェクト、セキュリティグループを指定してください。");return;}save.mutate();}}>
        <SpaceBetween size="m">
          {(validation || save.isError) && <Alert type="error">{validation || getApiErrorMessage(save.error)}</Alert>}
          <FormField label="名前"><Input value={name} onChange={({detail}) => setName(detail.value)} disabled={save.isPending} /></FormField>
          <FormField label="プロジェクト"><ProjectSelector value={project} onValueChange={setProject} disabled={edit !== "new" || save.isPending} /></FormField>
          <FormField label="リージョン"><Input value={spec.region} onChange={({detail}) => setSpec({...spec,region: detail.value})} disabled={save.isPending} /></FormField>
          <FormField label="説明"><Textarea value={spec.description} onChange={({detail}) => setSpec({...spec,description: detail.value})} /></FormField>
          <FormField label="外向きNAT" description="Flash側の送信アクセス設定も適用されます。外部からの接続を許可する設定ではありません。"><Toggle checked={spec.nat.enabled} onChange={({detail}) => setSpec({...spec,nat:{enabled:detail.checked}})}>有効にする</Toggle></FormField>
          <FormField label="セキュリティグループ" description="1行に1つ。英小文字・数字・ハイフンを使用してください。"><Textarea value={groups} onChange={({detail}) => setGroups(detail.value)} /></FormField>
          <Header variant="h3">通信ルール</Header>
          {spec.rules.map((rule,index) => <Container key={index}>
            <SpaceBetween size="s">
              <ColumnLayout columns={2}>{(["source","destination"] as const).map(key => <FormField key={key} label={key === "source" ? "接続元" : "接続先"}><Select selectedOption={peerOptions.find(o => o.value === peerValue(rule[key])) ?? {value:peerValue(rule[key]),label:peerValue(rule[key])}} options={peerOptions} onChange={({detail}) => changeRule(index,{[key]:parsePeer(detail.selectedOption.value ?? "all")})} /></FormField>)}</ColumnLayout>
              <ColumnLayout columns={3}>
                <FormField label="プロトコル"><Select options={[{value:"tcp",label:"TCP"},{value:"udp",label:"UDP"}]} selectedOption={{value:rule.protocol,label:rule.protocol.toUpperCase()}} onChange={({detail}) => changeRule(index,{protocol:detail.selectedOption.value === "udp" ? "udp" : "tcp"})} /></FormField>
                <FormField label="開始ポート"><Input type="number" value={String(rule.port)} onChange={({detail}) => changeRule(index,{port:Number(detail.value)})} /></FormField>
                <FormField label="終了ポート（省略可）"><Input type="number" value={rule.end_port === undefined ? "" : String(rule.end_port)} onChange={({detail}) => changeRule(index,{end_port:detail.value ? Number(detail.value) : undefined})} /></FormField>
              </ColumnLayout>
              <Box variant="small">接続先サービスで宣言されているポートだけに適用されます。応答通信は自動で許可されます。</Box>
              <Button formAction="none" onClick={() => setSpec({...spec,rules:spec.rules.filter((_,i) => i !== index)})}>ルールを削除</Button>
            </SpaceBetween>
          </Container>)}
          <Button formAction="none" disabled={spec.rules.length >= 128} onClick={() => setSpec({...spec,rules:[...spec.rules,{description:"",source:parsePeer(peerOptions[0]?.value ?? "all"),destination:parsePeer(peerOptions[0]?.value ?? "all"),protocol:"tcp",port:443}]})}>ルールを追加</Button>
          <SpaceBetween direction="horizontal" size="xs"><Button formAction="none" onClick={() => setEdit(null)}>キャンセル</Button><Button variant="primary" formAction="submit" loading={save.isPending}>保存</Button></SpaceBetween>
        </SpaceBetween>
      </form>
    </Modal>
    <Modal visible={deleting !== null} onDismiss={() => setDeleting(null)} header="VPCを削除">
      <SpaceBetween size="m"><Box>{deleting?.name} を削除します。</Box>{remove.isError && <Alert type="error">{getApiErrorMessage(remove.error)}</Alert>}<Button formAction="none" variant="primary" loading={remove.isPending} onClick={() => {if(deleting) remove.mutate(deleting.id);}}>削除する</Button></SpaceBetween>
    </Modal>
  </SpaceBetween>;
}
