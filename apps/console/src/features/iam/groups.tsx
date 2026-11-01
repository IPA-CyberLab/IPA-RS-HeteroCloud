import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Button, FormField, Input, Select, SpaceBetween } from "@/components/ui";
import { FormError } from "@/components/shared/form-error";
import { api, getApiErrorMessage } from "@/lib/api-client";
import type { IamPolicy, Principal } from "@/lib/api-types";

export function IamGroups({ accountId, principals, policies }: { accountId: string; principals: Principal[]; policies: IamPolicy[] }) {
  const client = useQueryClient();
  const key = ["organizations", accountId, "iam", "groups"];
  const groups = useQuery({ queryKey: key, queryFn: ({ signal }) => api.iam.groups.list(accountId, signal) });
  const [name, setName] = useState("");
  const [group, setGroup] = useState("");
  const [user, setUser] = useState("");
  const [policy, setPolicy] = useState("");
  const refresh = () => client.invalidateQueries({ queryKey: key });
  const create = useMutation({ mutationFn: () => api.iam.groups.create(accountId, name.trim()), onSuccess: async () => { setName(""); await refresh(); } });
  const change = useMutation({
    mutationFn: ({ kind, groupId, id, add }: { kind: "member" | "policy"; groupId: string; id: string; add: boolean }) =>
      kind === "member" ? api.iam.groups.member(accountId, groupId, id, add) : api.iam.groups.policy(accountId, groupId, id, add),
    onSuccess: refresh,
  });
  const remove = useMutation({ mutationFn: (id: string) => api.iam.groups.delete(accountId, id), onSuccess: refresh });
  const options = groups.data?.items.map((item) => ({ value: item.id, label: item.name })) ?? [];
  const users = principals.filter((item) => item.kind === "user" && item.enabled).map((item) => ({ value: item.id, label: item.name }));
  const policyOptions = policies.map((item) => ({ value: item.id, label: item.name }));
  return <section className="card bg-base-100 border border-base-300 p-5"><SpaceBetween size="m">
    <div><h2 className="text-lg font-semibold">IAMユーザーグループ</h2><p>グループのポリシーは所属ユーザーの権限に追加されます。明示的な拒否は優先されます。</p></div>
    {groups.data && !groups.data.enabled ? <p>APIの更新が完了するまで、グループの変更は停止しています。</p> : null}
    {groups.isError ? <FormError message={getApiErrorMessage(groups.error)} /> : null}
    <SpaceBetween direction="horizontal" size="xs"><FormField label="グループ名"><Input value={name} onChange={({ detail }) => setName(detail.value)} /></FormField>
      <Button disabled={!name.trim() || !groups.data?.enabled || create.isPending} onClick={() => create.mutate()}>グループを作成</Button></SpaceBetween>
    {groups.data?.items.map((item) => <section key={item.id} className="rounded-box border border-base-300 p-4"><SpaceBetween size="xs">
      <div className="font-semibold">{item.name}</div>
      <div>ユーザー: {item.members.length ? item.members.map((id) => <span key={id}>{principals.find((entry) => entry.id === id)?.name ?? id}{" "}<Button disabled={!groups.data?.enabled || change.isPending} onClick={() => change.mutate({ kind: "member", groupId: item.id, id, add: false })}>解除</Button>{" "}</span>) : "なし"}</div>
      <div>ポリシー: {item.policies.length ? item.policies.map((id) => <span key={id}>{policies.find((entry) => entry.id === id)?.name ?? id}{" "}<Button disabled={!groups.data?.enabled || change.isPending} onClick={() => change.mutate({ kind: "policy", groupId: item.id, id, add: false })}>解除</Button>{" "}</span>) : "なし"}</div>
      <div><Button disabled={!groups.data?.enabled || remove.isPending} onClick={() => remove.mutate(item.id)}>グループを削除</Button></div>
    </SpaceBetween></section>)}
    <SpaceBetween direction="horizontal" size="xs">
      <Select ariaLabel="IAMグループ" selectedOption={options.find((item) => item.value === group) ?? null} options={options} placeholder="グループを選択" onChange={({ detail }) => setGroup(detail.selectedOption.value ?? "")} />
      <Select ariaLabel="グループに追加するユーザー" selectedOption={users.find((item) => item.value === user) ?? null} options={users} placeholder="ユーザーを選択" onChange={({ detail }) => setUser(detail.selectedOption.value ?? "")} />
      <Button disabled={!groups.data?.enabled || !group || !user || change.isPending} onClick={() => change.mutate({ kind: "member", groupId: group, id: user, add: true })}>ユーザーを追加</Button>
      <Select ariaLabel="グループに追加するポリシー" selectedOption={policyOptions.find((item) => item.value === policy) ?? null} options={policyOptions} placeholder="ポリシーを選択" onChange={({ detail }) => setPolicy(detail.selectedOption.value ?? "")} />
      <Button disabled={!groups.data?.enabled || !group || !policy || change.isPending} onClick={() => change.mutate({ kind: "policy", groupId: group, id: policy, add: true })}>ポリシーを追加</Button>
    </SpaceBetween>
    {create.isError ? <FormError message={getApiErrorMessage(create.error)} /> : null}{change.isError ? <FormError message={getApiErrorMessage(change.error)} /> : null}{remove.isError ? <FormError message={getApiErrorMessage(remove.error)} /> : null}
  </SpaceBetween></section>;
}
