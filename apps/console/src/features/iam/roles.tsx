import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Box, Button, FormField, Input, Modal, Select, SpaceBetween, Textarea } from "@/components/ui";
import { FormError } from "@/components/shared/form-error";
import { api, getApiErrorMessage } from "@/lib/api-client";
import { useActiveOrganization } from "@/features/organizations/organization-context";
import type { IamPolicy, IamRole } from "@/lib/api-types";

export function IamRoles({ accountId, policies }: { accountId: string; policies: IamPolicy[] }) {
  const client = useQueryClient();
  const { switchRole } = useActiveOrganization();
  const key = ["organizations", accountId, "iam", "roles"];
  const roles = useQuery({ queryKey: key, queryFn: ({ signal }) => api.iam.roles.list(accountId, signal) });
  const caller = useQuery({ queryKey: ["organizations", accountId, "sts", "caller"], queryFn: ({ signal }) => api.iam.roles.caller(accountId, signal), enabled: roles.data?.enabled });
  const [name, setName] = useState("");
  const [trustedAccount, setTrustedAccount] = useState("");
  const [maxHours, setMaxHours] = useState("1");
  const [roleId, setRoleId] = useState("");
  const [policyId, setPolicyId] = useState("");
  const [editing, setEditing] = useState<IamRole | null>(null);
  const [trustText, setTrustText] = useState("");
  const [targetArn, setTargetArn] = useState("");
  const [sessionName, setSessionName] = useState("console-session");
  const [sessionMinutes, setSessionMinutes] = useState("60");
  const [sessionPolicy, setSessionPolicy] = useState("");
  const [credentialsOpen, setCredentialsOpen] = useState(false);
  const [copyDone, setCopyDone] = useState(false);
  const [inputError, setInputError] = useState("");
  const [switching, setSwitching] = useState(false);
  const sessions = useQuery({ queryKey: ["organizations", accountId, "iam", "roles", roleId, "sessions"],
    queryFn: ({ signal }) => api.iam.roles.sessions(accountId, roleId, signal), enabled: !!roleId && !!roles.data?.enabled });
  const refresh = () => Promise.all([
    client.invalidateQueries({ queryKey: key }),
    client.invalidateQueries({ queryKey: ["organizations", accountId, "iam", "principals"] }),
    client.invalidateQueries({ queryKey: ["organizations", accountId, "iam", "bindings"] }),
  ]);
  const create = useMutation({ mutationFn: () => api.iam.roles.create(accountId, { name: name.trim(), max_session_seconds: Number(maxHours) * 3600,
    trust_policy: { Version: "2012-10-17", Statement: [{ Effect: "Allow", Action: "sts:AssumeRole", Principal: { HeteroCloud: `arn:heterocloud:iam::${trustedAccount}:root` } }] } }),
    onSuccess: async () => { setName(""); setTrustedAccount(""); await refresh(); } });
  const parseJson = (text: string): Record<string, unknown> => {
    const value: unknown = JSON.parse(text);
    if (typeof value !== "object" || value === null || Array.isArray(value)) throw new Error("JSONオブジェクトを指定してください。");
    return value as Record<string, unknown>;
  };
  const update = useMutation({ mutationFn: () => {
    if (!editing) throw new Error("ロールを選択してください。");
    return api.iam.roles.updateTrust(accountId, editing.principal_id, parseJson(trustText));
  }, onSuccess: async () => { setEditing(null); await refresh(); } });
  const attach = useMutation({ mutationFn: () => api.iam.bindings.create(accountId, { principal_id: roleId, policy_id: policyId }), onSuccess: refresh });
  const remove = useMutation({ mutationFn: (id: string) => api.iam.roles.delete(accountId, id), onSuccess: refresh });
  const revoke = useMutation({ mutationFn: (id: string) => api.iam.roles.revokeSession(accountId, roleId, id), onSuccess: () => sessions.refetch() });
  const assume = useMutation({ mutationFn: () => api.iam.roles.assume(accountId, { role_arn: targetArn.trim(), session_name: sessionName.trim(), duration_seconds: Number(sessionMinutes) * 60,
    ...(sessionPolicy.trim() ? { session_policy: parseJson(sessionPolicy) } : {}) }),
    onSuccess: () => { setCredentialsOpen(true); setCopyDone(false); setInputError(""); if (roleId) void sessions.refetch(); } });
  const roleOptions = roles.data?.items.map((item) => ({ value: item.principal_id, label: item.name })) ?? [];
  const policyOptions = policies.map((item) => ({ value: item.id, label: item.name }));
  return <section className="card bg-base-100 border border-base-300 p-5"><SpaceBetween size="m">
    <div><h2 className="text-lg font-semibold">IAMロール</h2>
      <p>信頼するアカウントとロールのポリシーを設定し、期限付きのAPI認証情報を発行するか、コンソールをロールに切り替えます。条件付き信頼とサービスからの引き受けは現在利用できません。</p></div>
    {caller.data ? <p>現在のプリンシパル: <code>{caller.data.arn}</code></p> : null}
    {roles.data && !roles.data.enabled ? <p>APIの更新が完了するまで、ロールの操作は停止しています。</p> : null}
    {roles.isError ? <FormError message={getApiErrorMessage(roles.error)} /> : null}
    <SpaceBetween direction="horizontal" size="xs">
      <FormField label="ロール名"><Input value={name} onChange={({ detail }) => setName(detail.value)} /></FormField>
      <FormField label="信頼するアカウントID"><Input value={trustedAccount} placeholder="12桁" onChange={({ detail }) => setTrustedAccount(detail.value)} /></FormField>
      <FormField label="最大時間（1〜12時間）"><Input type="number" value={maxHours} onChange={({ detail }) => setMaxHours(detail.value)} /></FormField>
      <Button disabled={!roles.data?.enabled || !name.trim() || !/^\d{12}$/.test(trustedAccount) || !Number.isInteger(Number(maxHours)) || Number(maxHours) < 1 || Number(maxHours) > 12 || create.isPending} onClick={() => create.mutate()}>ロールを作成</Button>
    </SpaceBetween>
    <div className="overflow-x-auto"><table className="table"><thead><tr><th>ロール</th><th>ARN</th><th>操作</th></tr></thead><tbody>
      {roles.data?.items.map((role) => <tr key={role.principal_id}><td>{role.name}{role.managed_by_identity_center ? " · アカウントアクセス 管理" : ""}</td><td><code>{role.arn}</code></td><td><SpaceBetween direction="horizontal" size="xs"><Button disabled={!roles.data?.enabled || role.managed_by_identity_center} onClick={() => { update.reset(); setTrustText(JSON.stringify(role.trust_policy, null, 2)); setEditing(role); }}>信頼を編集</Button><Button disabled={!roles.data?.enabled || role.managed_by_identity_center || remove.isPending} onClick={() => remove.mutate(role.principal_id)}>削除</Button></SpaceBetween></td></tr>)}
    </tbody></table></div>
    <SpaceBetween direction="horizontal" size="xs">
      <Select ariaLabel="ポリシーを付与するロール" options={roleOptions} selectedOption={roleOptions.find((item) => item.value === roleId) ?? null} onChange={({ detail }) => setRoleId(detail.selectedOption.value ?? "")} />
      <Select ariaLabel="ロールに付与するポリシー" options={policyOptions} selectedOption={policyOptions.find((item) => item.value === policyId) ?? null} onChange={({ detail }) => setPolicyId(detail.selectedOption.value ?? "")} />
      <Button disabled={!roles.data?.enabled || !roleId || !policyId || attach.isPending || roles.data.items.find((role) => role.principal_id === roleId)?.managed_by_identity_center} onClick={() => attach.mutate()}>ロールにポリシーを付与</Button>
    </SpaceBetween>
    {roleId ? <div><h3 className="font-semibold">選択したロールのセッション</h3>
      {sessions.data?.items.map((session) => <div key={session.id}>{session.session_name} / {session.source_arn} / 有効期限: {session.expires_at}{" "}
        {session.revoked_at ? "失効済み" : <Button disabled={revoke.isPending} onClick={() => revoke.mutate(session.id)}>失効</Button>}</div>)}
      {sessions.isError ? <FormError message={getApiErrorMessage(sessions.error)} /> : null}{revoke.isError ? <FormError message={getApiErrorMessage(revoke.error)} /> : null}</div> : null}
    <h3 className="font-semibold">ロールの一時認証情報を発行</h3>
    <SpaceBetween direction="horizontal" size="xs"><FormField label="引き受けるロールARN"><Input value={targetArn} onChange={({ detail }) => setTargetArn(detail.value)} /></FormField>
      <FormField label="セッション名"><Input value={sessionName} onChange={({ detail }) => setSessionName(detail.value)} /></FormField>
      <FormField label="有効時間（分）"><Input type="number" value={sessionMinutes} onChange={({ detail }) => setSessionMinutes(detail.value)} /></FormField></SpaceBetween>
    <FormField label="セッションポリシー（任意）" description="指定すると、ロールの許可をこのポリシーでも制限します。"><Textarea rows={4} value={sessionPolicy} onChange={({ detail }) => setSessionPolicy(detail.value)} /></FormField>
    <SpaceBetween direction="horizontal" size="xs"><Button disabled={!roles.data?.enabled || !targetArn.trim() || !sessionName.trim() || !Number.isInteger(Number(sessionMinutes)) || Number(sessionMinutes) < 15 || Number(sessionMinutes) > 720 || assume.isPending} onClick={() => { setInputError(""); assume.mutate(); }}>一時認証情報を発行</Button>
      <Button variant="primary" loading={switching} disabled={!roles.data?.enabled || !roles.data?.browser_switch_enabled || !targetArn.trim() || !sessionName.trim() || !Number.isInteger(Number(sessionMinutes)) || Number(sessionMinutes) < 15 || Number(sessionMinutes) > 720} onClick={() => {
        let policy: Record<string, unknown> | undefined;
        try { policy = sessionPolicy.trim() ? parseJson(sessionPolicy) : undefined; } catch { setInputError("有効なセッションポリシーJSONを入力してください。"); return; }
        setSwitching(true);
        void switchRole({ role_arn: targetArn.trim(), session_name: sessionName.trim(), duration_seconds: Number(sessionMinutes) * 60, ...(policy ? { session_policy: policy } : {}) }).catch(() => undefined).finally(() => setSwitching(false));
      }}>コンソールをロールに切り替え</Button></SpaceBetween>
    {create.isError ? <FormError message={getApiErrorMessage(create.error)} /> : null}{attach.isError ? <FormError message={getApiErrorMessage(attach.error)} /> : null}
    {remove.isError ? <FormError message={getApiErrorMessage(remove.error)} /> : null}
    {assume.isError ? <FormError message={getApiErrorMessage(assume.error)} /> : null}{inputError ? <FormError message={inputError} /> : null}
    <Modal visible={editing !== null} onDismiss={() => setEditing(null)} size="large" header="ロールの信頼ポリシー"
      footer={<Box float="right"><SpaceBetween direction="horizontal" size="xs"><Button onClick={() => setEditing(null)}>キャンセル</Button><Button variant="primary" loading={update.isPending} onClick={() => update.mutate()}>保存</Button></SpaceBetween></Box>}>
      <SpaceBetween size="m"><FormField label="信頼ポリシーJSON"><Textarea rows={14} value={trustText} onChange={({ detail }) => setTrustText(detail.value)} /></FormField>
        {update.isError ? <FormError message={getApiErrorMessage(update.error)} /> : null}</SpaceBetween>
    </Modal>
    <Modal visible={credentialsOpen} onDismiss={() => { setCredentialsOpen(false); assume.reset(); }} header="一時認証情報"
      footer={<Button onClick={() => { setCredentialsOpen(false); assume.reset(); }}>閉じる</Button>}>
      <SpaceBetween size="m"><p>このトークンは再表示できません。APIのBearer認証に使用できます。有効期限: {assume.data?.expires_at}</p>
        <code className="break-all">{assume.data?.access_token}</code><div><Button onClick={() => {
          if (!assume.data?.access_token) return;
          void navigator.clipboard.writeText(assume.data.access_token).then(() => setCopyDone(true)).catch(() => setInputError("コピーできませんでした。"));
        }}>{copyDone ? "コピーしました" : "コピー"}</Button></div></SpaceBetween>
    </Modal>
  </SpaceBetween></section>;
}
