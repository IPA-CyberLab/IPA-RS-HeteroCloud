import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { Button, ColumnLayout, FormField, Header, Input, Modal, Select, SpaceBetween, TagInput, Textarea } from "@/components/ui";
import { FormError } from "@/components/shared/form-error";
import { PageHeader } from "@/components/shared/page-header";
import { PageLoading } from "@/components/shared/page-loading";
import { useActiveOrganization } from "@/features/organizations/organization-context";
import { api, getApiErrorMessage } from "@/lib/api-client";
import type { IdentityCenterPermissionSet } from "@/lib/api-types";

const readActions = ["project:List", "flash:ListInstances", "flash:GetInstance", "registry:GetRegistry",
  "realtime:ListServices", "realtime:GetService", "realtime:GetMetrics", "syouyu:ListBuckets", "syouyu:GetBucket", "syouyu:GetBucketUsage", "vpc:ListNetworks", "vpc:GetNetwork"];
const presets = [{ value: "read", label: "閲覧専用" }, { value: "admin", label: "管理者（すべて）" }, { value: "custom", label: "アクションを指定" }];
const subjectOptions = [{ value: "user", label: "ユーザー" }, { value: "group", label: "グループ" }];
const durations = [{ value: "3600", label: "1 時間" }, { value: "14400", label: "4 時間" }, { value: "28800", label: "8 時間" }, { value: "43200", label: "12 時間" }];
const visualPolicy = (preset: string, actions: string[], resources: string[]) => ({
  Version: "2012-10-17", Statement: [{ Effect: "Allow", Action: preset === "read" ? readActions : preset === "admin" ? ["*"] : actions, Resource: resources }],
});

export function IdentityCenterPage() {
  const { activeOrganization, activeRole, exitRole, switchIdentityCenter } = useActiveOrganization();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const current = activeOrganization.organization_id;
  const organization = useQuery({ queryKey: ["identity-center", current, "organization"], queryFn: ({ signal }) => api.organizations.describeCloudOrganization(current, signal), enabled: !activeRole });
  const management = organization.data?.management_account_id ?? "";
  const queryEnabled = Boolean(management) && !activeRole;
  const mine = useQuery({ queryKey: ["identity-center", management, "mine"], queryFn: ({ signal }) => api.identityCenter.myAssignments(management, signal), enabled: queryEnabled });
  const sets = useQuery({ queryKey: ["identity-center", management, "sets"], queryFn: ({ signal }) => api.identityCenter.permissionSets(management, signal), enabled: queryEnabled, retry: false });
  const canManage = sets.isSuccess;
  const groups = useQuery({ queryKey: ["identity-center", management, "groups"], queryFn: ({ signal }) => api.identityCenter.groups(management, signal), enabled: canManage });
  const users = useQuery({ queryKey: ["identity-center", management, "users"], queryFn: ({ signal }) => api.identityCenter.users(management, signal), enabled: canManage });
  const assignments = useQuery({ queryKey: ["identity-center", management, "assignments"], queryFn: ({ signal }) => api.identityCenter.assignments(management, signal), enabled: canManage });
  const tree = useQuery({ queryKey: ["identity-center", management, "tree"], queryFn: ({ signal }) => api.organizations.tree(management, signal), enabled: canManage });
  const [setOpen, setSetOpen] = useState(false);
  const [editing, setEditing] = useState<IdentityCenterPermissionSet | null>(null);
  const [setName, setSetName] = useState("");
  const [preset, setPreset] = useState("read");
  const [actions, setActions] = useState<string[]>([]);
  const [resources, setResources] = useState<string[]>(["*"]);
  const [duration, setDuration] = useState("3600");
  const [jsonMode, setJsonMode] = useState(false);
  const [documentJson, setDocumentJson] = useState("");
  const [formError, setFormError] = useState("");
  const [groupName, setGroupName] = useState("");
  const [selectedGroup, setSelectedGroup] = useState("");
  const [selectedUser, setSelectedUser] = useState("");
  const [assignmentOpen, setAssignmentOpen] = useState(false);
  const [setId, setSetId] = useState("");
  const [accountId, setAccountId] = useState("");
  const [subjectType, setSubjectType] = useState<"user" | "group">("user");
  const [subjectId, setSubjectId] = useState("");
  const mutation = useMutation({ mutationFn: (operation: () => Promise<unknown>) => operation(), onSuccess: async () => {
    await queryClient.invalidateQueries({ queryKey: ["identity-center", management] });
    await queryClient.invalidateQueries({ queryKey: ["auth", "session"] });
  } });
  const execute = (operation: () => Promise<unknown>, after?: () => void) => mutation.mutate(operation, { onSuccess: after });
  const openCreate = () => { setEditing(null); setSetName(""); setPreset("read"); setActions([]); setResources(["*"]); setDuration("3600"); setJsonMode(false); setFormError(""); setSetOpen(true); };
  const loadVisual = (document: Record<string, unknown>) => {
    const entries = document.statements ?? document.Statement;
    const list = Array.isArray(entries) ? entries : entries ? [entries] : [];
    if (list.length !== 1 || !list[0] || typeof list[0] !== "object" || Array.isArray(list[0])) return false;
    const statement = list[0] as Record<string, unknown>;
    const selected = statement.actions ?? statement.Action;
    const scope = statement.resources ?? statement.Resource;
    const selectedActions = typeof selected === "string" ? [selected] : selected;
    const selectedResources = typeof scope === "string" ? [scope] : scope;
    if ((statement.effect ?? statement.Effect) !== "Allow" || statement.sid || statement.Sid || statement.not_actions || statement.NotAction || statement.not_resources || statement.NotResource || statement.conditions || statement.Condition ||
      !Array.isArray(selectedActions) || !selectedActions.every((item) => typeof item === "string") || !Array.isArray(selectedResources) || !selectedResources.every((item) => typeof item === "string")) return false;
    setActions(selectedActions); setResources(selectedResources);
    setPreset(selectedActions.length === 1 && selectedActions[0] === "*" ? "admin" : JSON.stringify([...selectedActions].sort()) === JSON.stringify([...readActions].sort()) ? "read" : "custom");
    return true;
  };
  const openEdit = (set: IdentityCenterPermissionSet) => { setEditing(set); setSetName(set.name); setDuration(String(set.session_duration_seconds)); setDocumentJson(JSON.stringify(set.document, null, 2)); setJsonMode(!loadVisual(set.document)); setFormError(""); setSetOpen(true); };
  const saveSet = () => {
    try {
      let document: Record<string, unknown>;
      if (jsonMode) {
        const value: unknown = JSON.parse(documentJson);
        if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("ポリシーには JSON オブジェクトを指定してください。");
        document = value as Record<string, unknown>;
      } else {
        const selected = preset === "read" ? readActions : preset === "admin" ? ["*"] : actions;
        if (!selected.length || !resources.length) throw new Error("アクションとリソースを指定してください。");
        document = visualPolicy(preset, actions, resources);
      }
      const value = { document, session_duration_seconds: Number(duration) };
      setFormError("");
      execute(() => editing ? api.identityCenter.updatePermissionSet(management, editing.id, value) : api.identityCenter.createPermissionSet(management, { ...value, name: setName.trim() }), () => setSetOpen(false));
    } catch (error) { setFormError(error instanceof Error ? error.message : "ポリシーを確認してください。"); }
  };
  if (activeRole) return <SpaceBetween size="l"><PageHeader title="Identity Center" description="アカウントアクセスを管理します。" /><section className="card border border-base-300 bg-base-100 p-5"><p>現在はロールを使用しています。割り当て一覧に戻るには元のアカウントへ戻ってください。</p><Button onClick={() => void exitRole()}>ロールを終了</Button></section></SpaceBetween>;
  if (organization.isPending) return <PageLoading label="組織を読み込んでいます" />;
  if (organization.isError || !management) return <FormError message="組織情報を取得できませんでした。" />;
  const groupOptions = (groups.data?.items ?? []).map((group) => ({ value: group.id, label: group.name }));
  const userOptions = (users.data?.items ?? []).map((user) => ({ value: user.id, label: `${user.display_name} (${user.email})` }));
  const selected = groups.data?.items.find((group) => group.id === selectedGroup);
  const targetOptions = (tree.data?.accounts ?? []).map((account) => ({ value: account.id, label: `${account.name} (${account.account_number})` }));
  const setOptions = (sets.data?.items ?? []).map((set) => ({ value: set.id, label: set.name }));
  const actorOptions = subjectType === "user" ? userOptions : groupOptions;
  return <SpaceBetween size="l">
    <PageHeader title="Identity Center" description="組織内のアカウントに、ユーザーやグループごとの権限セットを割り当てます。" />
    {activeRole ? <section className="card border border-base-300 bg-base-100 p-5"><p>現在はロールを使用しています。割り当て一覧に戻るには元のアカウントへ戻ってください。</p><Button onClick={() => void exitRole()}>ロールを終了</Button></section> : <>
      <section className="card border border-base-300 bg-base-100 p-5"><SpaceBetween size="m">
        <Header variant="h2" description="利用できる権限セットを選ぶと、対象アカウントへ切り替わります。">自分のアカウントアクセス</Header>
        {mine.isPending ? <PageLoading label="割り当てを確認しています" /> : mine.isError ? <FormError message={getApiErrorMessage(mine.error)} /> : mine.data.items.length === 0 ? <p>割り当てはありません。</p> : <div className="overflow-x-auto"><table className="table"><thead><tr><th>アカウント</th><th>権限セット</th><th>セッション</th><th>操作</th></tr></thead><tbody>{mine.data.items.map((item) => <tr key={item.id}><td>{item.account_name} · {item.account_number}</td><td>{item.permission_set_name}</td><td>{item.session_duration_seconds / 3600} 時間</td><td><Button onClick={() => void switchIdentityCenter(management, item.id).then(() => navigate("/overview"))}>切り替え</Button></td></tr>)}</tbody></table></div>}
      </SpaceBetween></section>
      {canManage ? <>
        <section className="card border border-base-300 bg-base-100 p-5"><SpaceBetween size="m">
          <Header variant="h2" actions={<Button variant="primary" onClick={openCreate}>権限セットを作成</Button>}>権限セット</Header>
          <div className="overflow-x-auto"><table className="table"><thead><tr><th>名前</th><th>セッション</th><th>操作</th></tr></thead><tbody>{sets.data.items.map((set) => <tr key={set.id}><td>{set.name}</td><td>{set.session_duration_seconds / 3600} 時間</td><td><SpaceBetween direction="horizontal" size="xs"><Button onClick={() => openEdit(set)}>編集</Button><Button onClick={() => { if (window.confirm(`${set.name} を削除しますか？`)) execute(() => api.identityCenter.deletePermissionSet(management, set.id)); }}>削除</Button></SpaceBetween></td></tr>)}</tbody></table></div>
        </SpaceBetween></section>
        <section className="card border border-base-300 bg-base-100 p-5"><SpaceBetween size="m">
          <Header variant="h2" description="管理アカウントのメンバーをグループに追加できます。">グループ</Header>
          <ColumnLayout columns={2}><FormField label="グループ名"><Input value={groupName} onChange={({ detail }) => setGroupName(detail.value)} /></FormField><div className="self-end"><Button disabled={!groupName.trim()} onClick={() => execute(() => api.identityCenter.createGroup(management, groupName.trim()), () => setGroupName(""))}>作成</Button></div></ColumnLayout>
          <ColumnLayout columns={3}><FormField label="グループ"><Select options={groupOptions} selectedOption={groupOptions.find((item) => item.value === selectedGroup)} onChange={({ detail }) => setSelectedGroup(detail.selectedOption.value ?? "")} /></FormField><FormField label="ユーザー"><Select options={userOptions} selectedOption={userOptions.find((item) => item.value === selectedUser)} onChange={({ detail }) => setSelectedUser(detail.selectedOption.value ?? "")} /></FormField><div className="self-end"><SpaceBetween direction="horizontal" size="xs"><Button disabled={!selectedGroup || !selectedUser || Boolean(selected?.users.includes(selectedUser))} onClick={() => execute(() => api.identityCenter.groupUser(management, selectedGroup, selectedUser, true))}>追加</Button><Button disabled={!selectedGroup || !selectedUser || !selected?.users.includes(selectedUser)} onClick={() => execute(() => api.identityCenter.groupUser(management, selectedGroup, selectedUser, false))}>削除</Button></SpaceBetween></div></ColumnLayout>
          {selected ? <><p>メンバー: {selected.users.map((id) => users.data?.items.find((user) => user.id === id)?.display_name ?? id).join("、") || "なし"}</p><Button onClick={() => { if (window.confirm(`${selected.name} を削除しますか？`)) execute(() => api.identityCenter.deleteGroup(management, selected.id), () => setSelectedGroup("")); }}>グループを削除</Button></> : null}
        </SpaceBetween></section>
        <section className="card border border-base-300 bg-base-100 p-5"><SpaceBetween size="m">
          <Header variant="h2" actions={<Button variant="primary" onClick={() => setAssignmentOpen(true)}>アカウントに割り当て</Button>}>アカウント割り当て</Header>
          {[groups.error, users.error, assignments.error, tree.error].filter(Boolean).map((error, index) => <FormError key={index} message={getApiErrorMessage(error)} />)}
          <div className="overflow-x-auto"><table className="table"><thead><tr><th>アカウント</th><th>権限セット</th><th>対象</th><th>操作</th></tr></thead><tbody>{assignments.data?.items.map((item) => <tr key={item.id}><td>{item.account_name} · {item.account_number}</td><td>{item.permission_set_name}</td><td>{item.subject_type === "group" ? groups.data?.items.find((group) => group.id === item.subject_id)?.name : users.data?.items.find((user) => user.id === item.subject_id)?.display_name}</td><td><Button onClick={() => { if (window.confirm("この割り当てを解除しますか？")) execute(() => api.identityCenter.revoke(management, item.id)); }}>解除</Button></td></tr>)}</tbody></table></div>
        </SpaceBetween></section>
      </> : sets.isError && (sets.error as { status?: number }).status !== 403 ? <FormError message={getApiErrorMessage(sets.error)} /> : null}
    </>}
    {mutation.isError ? <FormError message={getApiErrorMessage(mutation.error)} /> : null}
    <Modal visible={setOpen} onDismiss={() => setSetOpen(false)} header={editing ? "権限セットを編集" : "権限セットを作成"} footer={<SpaceBetween direction="horizontal" size="xs"><Button onClick={() => setSetOpen(false)}>キャンセル</Button><Button variant="primary" disabled={!editing && !setName.trim()} loading={mutation.isPending} onClick={saveSet}>保存</Button></SpaceBetween>}>
      <SpaceBetween size="m"><FormField label="名前"><Input value={setName} disabled={Boolean(editing)} onChange={({ detail }) => setSetName(detail.value)} /></FormField><FormField label="セッション時間"><Select options={durations} selectedOption={durations.find((item) => item.value === duration)} onChange={({ detail }) => setDuration(detail.selectedOption.value ?? "3600")} /></FormField>
        <SpaceBetween direction="horizontal" size="xs"><Button variant={!jsonMode ? "primary" : "normal"} onClick={() => { try { const value: unknown = JSON.parse(documentJson); if (!value || typeof value !== "object" || Array.isArray(value) || !loadVisual(value as Record<string, unknown>)) throw new Error("このポリシーは JSON で編集してください。"); setFormError(""); setJsonMode(false); } catch (error) { if (jsonMode) setFormError(error instanceof Error ? error.message : "ポリシーを確認してください。"); } }}>ビジュアル</Button><Button variant={jsonMode ? "primary" : "normal"} onClick={() => { if (!jsonMode) setDocumentJson(JSON.stringify(visualPolicy(preset, actions, resources), null, 2)); setFormError(""); setJsonMode(true); }}>JSON</Button></SpaceBetween>
        {jsonMode ? <FormField label="ポリシー JSON"><Textarea rows={14} value={documentJson} onChange={({ detail }) => setDocumentJson(detail.value)} /></FormField> : <><FormField label="権限"><Select options={presets} selectedOption={presets.find((item) => item.value === preset)} onChange={({ detail }) => setPreset(detail.selectedOption.value ?? "read")} /></FormField>{preset === "custom" ? <FormField label="アクション"><TagInput values={actions} onChange={setActions} ariaLabel="アクションを追加" /></FormField> : null}<FormField label="リソース"><TagInput values={resources} onChange={setResources} ariaLabel="リソースを追加" /></FormField></>}
        <FormError message={formError || (mutation.isError ? getApiErrorMessage(mutation.error) : null)} />
      </SpaceBetween>
    </Modal>
    <Modal visible={assignmentOpen} onDismiss={() => setAssignmentOpen(false)} header="アカウントに割り当て" footer={<SpaceBetween direction="horizontal" size="xs"><Button onClick={() => setAssignmentOpen(false)}>キャンセル</Button><Button variant="primary" disabled={!setId || !accountId || !subjectId} loading={mutation.isPending} onClick={() => execute(() => api.identityCenter.assign(management, { permission_set_id: setId, account_id: accountId, subject_type: subjectType, subject_id: subjectId }), () => setAssignmentOpen(false))}>割り当て</Button></SpaceBetween>}>
      <SpaceBetween size="m"><FormField label="権限セット"><Select options={setOptions} selectedOption={setOptions.find((item) => item.value === setId)} onChange={({ detail }) => setSetId(detail.selectedOption.value ?? "")} /></FormField><FormField label="アカウント"><Select options={targetOptions} selectedOption={targetOptions.find((item) => item.value === accountId)} onChange={({ detail }) => setAccountId(detail.selectedOption.value ?? "")} /></FormField><FormField label="対象の種類"><Select options={subjectOptions} selectedOption={subjectOptions.find((item) => item.value === subjectType)} onChange={({ detail }) => { setSubjectType(detail.selectedOption.value as "user" | "group"); setSubjectId(""); }} /></FormField><FormField label="対象"><Select options={actorOptions} selectedOption={actorOptions.find((item) => item.value === subjectId)} onChange={({ detail }) => setSubjectId(detail.selectedOption.value ?? "")} /></FormField><FormError message={mutation.isError ? getApiErrorMessage(mutation.error) : null} /></SpaceBetween>
    </Modal>
  </SpaceBetween>;
}
