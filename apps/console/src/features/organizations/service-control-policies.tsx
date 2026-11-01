import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { Box, Button, FormField, Input, Modal, Select, SpaceBetween, Textarea } from "@/components/ui";
import { FormError } from "@/components/shared/form-error";
import { api, getApiErrorMessage } from "@/lib/api-client";
import type { CloudOrganizationTree } from "@/lib/api-types";

const example = JSON.stringify({ Version: "2012-10-17", Statement: [{ Effect: "Deny", Action: ["flash:Delete*"], Resource: "*" }] }, null, 2);

export function ServiceControlPolicies({ accountId, tree }: { accountId: string; tree: CloudOrganizationTree }) {
  const client = useQueryClient();
  const key = ["cloud-organization", accountId, "policies"];
  const policies = useQuery({ queryKey: key, queryFn: ({ signal }) => api.organizations.serviceControlPolicies(accountId, signal) });
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [document, setDocument] = useState(example);
  const [policyId, setPolicyId] = useState("");
  const [target, setTarget] = useState("");
  const targets = useMemo(() => [
    ...tree.units.map((unit) => ({ value: `unit:${unit.id}`, label: unit.parent_id ? `OU: ${unit.name}` : "組織ルート" })),
    ...tree.accounts.filter((account) => !account.is_management_account).map((account) => ({ value: `account:${account.id}`, label: `アカウント: ${account.name} (${account.account_number})` })),
  ], [tree]);
  const refresh = () => client.invalidateQueries({ queryKey: key });
  const create = useMutation({
    mutationFn: async () => {
      let parsed: unknown;
      try { parsed = JSON.parse(document); } catch { throw new Error("ポリシーのJSONを確認してください。"); }
      if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) throw new Error("ポリシーはJSONオブジェクトで指定してください。");
      return api.organizations.createServiceControlPolicy(accountId, { name: name.trim(), document: parsed as Record<string, unknown> });
    },
    onSuccess: async () => { await refresh(); setOpen(false); setName(""); setDocument(example); },
  });
  const change = useMutation({
    mutationFn: ({ policy, type, id, attach }: { policy: string; type: "unit" | "account"; id: string; attach: boolean }) =>
      api.organizations.changeServiceControlPolicyAttachment(accountId, policy, type, id, attach),
    onSuccess: refresh,
  });
  const selectedPolicy = policies.data?.policies.find((item) => item.id === policyId);
  const targetOption = targets.find((item) => item.value === target);
  return <section className="card bg-base-100 border border-base-300 p-5"><SpaceBetween size="m">
    <div><h2 className="text-lg font-semibold">サービスコントロールポリシー（SCP）</h2>
      <p>組織ルート・OU・メンバーアカウントの管理APIに適用される権限の上限です。管理アカウントと発行済みのサービス直接接続用認証情報には適用されません。</p></div>
    {policies.isError ? <FormError message={getApiErrorMessage(policies.error)} /> : null}
    {policies.data && !policies.data.enabled ? <p>APIの更新が完了するまで、SCPの適用は停止しています。</p> : null}
    <div><Button disabled={!policies.data?.enabled} onClick={() => { create.reset(); setOpen(true); }}>ポリシーを作成</Button></div>
    <div className="overflow-x-auto"><table className="table"><thead><tr><th>ポリシー</th><th>種別</th><th>適用先</th></tr></thead>
      <tbody>{policies.data?.policies.map((policy) => <tr key={policy.id}><td>{policy.name}</td><td>{policy.is_managed ? "管理ポリシー" : "カスタム"}</td><td>
        <SpaceBetween size="xxs">{policies.data.attachments.filter((attachment) => attachment.policy_id === policy.id).map((attachment) => {
          const key = `${attachment.target_type}:${attachment.target_id}`;
          return <div key={key}>{targets.find((item) => item.value === key)?.label ?? (attachment.target_type === "account" && attachment.target_id === tree.organization.management_account_id ? "管理アカウント（SCP適用外）" : key)}{" "}
            <Button onClick={() => change.mutate({ policy: policy.id, type: attachment.target_type, id: attachment.target_id, attach: false })} disabled={change.isPending || !policies.data?.enabled}>解除</Button></div>;
        })}</SpaceBetween>
      </td></tr>)}</tbody></table></div>
    <SpaceBetween direction="horizontal" size="xs">
      <Select ariaLabel="SCPポリシー" selectedOption={selectedPolicy ? { value: selectedPolicy.id, label: selectedPolicy.name } : null}
        options={policies.data?.policies.map((item) => ({ value: item.id, label: item.name })) ?? []} placeholder="ポリシーを選択" onChange={({ detail }) => setPolicyId(detail.selectedOption.value ?? "")} />
      <Select ariaLabel="SCP適用先" selectedOption={targetOption ?? null} options={targets} placeholder="適用先を選択" onChange={({ detail }) => setTarget(detail.selectedOption.value ?? "")} />
      <Button disabled={!policies.data?.enabled || !selectedPolicy || !targetOption || change.isPending} onClick={() => {
        const [type, id] = target.split(":");
        if ((type === "unit" || type === "account") && id) change.mutate({ policy: policyId, type, id, attach: true });
      }}>適用</Button>
    </SpaceBetween>
    {change.isError ? <FormError message={getApiErrorMessage(change.error)} /> : null}
    <Modal visible={open} onDismiss={() => setOpen(false)} size="large" header="SCPを作成"
      footer={<Box float="right"><SpaceBetween direction="horizontal" size="xs"><Button onClick={() => setOpen(false)}>キャンセル</Button>
        <Button variant="primary" disabled={!name.trim() || !document.trim()} loading={create.isPending} onClick={() => create.mutate()}>作成</Button></SpaceBetween></Box>}>
      <SpaceBetween size="m"><FormField label="ポリシー名"><Input value={name} onChange={({ detail }) => setName(detail.value)} /></FormField>
        <FormField label="ポリシーJSON" description="Action、NotAction、Resource、NotResourceを使用できます。条件とPrincipalは現在サポートしていません。">
          <Textarea value={document} rows={12} onChange={({ detail }) => setDocument(detail.value)} />
        </FormField>{create.isError ? <FormError message={getApiErrorMessage(create.error)} /> : null}</SpaceBetween>
    </Modal>
  </SpaceBetween></section>;
}
