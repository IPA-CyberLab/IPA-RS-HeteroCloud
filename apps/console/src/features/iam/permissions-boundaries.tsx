import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Button, Select, SpaceBetween } from "@/components/ui";
import { FormError } from "@/components/shared/form-error";
import { api, getApiErrorMessage } from "@/lib/api-client";
import type { IamPolicy, Principal } from "@/lib/api-types";

export function IamBoundaries({ accountId, principals, policies }: { accountId: string; principals: Principal[]; policies: IamPolicy[] }) {
  const client = useQueryClient();
  const key = ["organizations", accountId, "iam", "permissions-boundaries"];
  const boundaries = useQuery({ queryKey: key, queryFn: ({ signal }) => api.iam.boundaries.list(accountId, signal) });
  const [principal, setPrincipal] = useState("");
  const [policy, setPolicy] = useState("");
  const principalOptions = principals.map((item) => ({ value: item.id, label: item.name }));
  const policyOptions = policies.map((item) => ({ value: item.id, label: item.name }));
  const change = useMutation({ mutationFn: ({ id, policyId }: { id: string; policyId?: string }) => policyId ? api.iam.boundaries.put(accountId, id, policyId) : api.iam.boundaries.delete(accountId, id),
    onSuccess: () => client.invalidateQueries({ queryKey: key }) });
  return <section className="card bg-base-100 border border-base-300 p-5"><SpaceBetween size="m">
    <div><h2 className="text-lg font-semibold">アクセス許可の境界</h2><p>ユーザーまたはサービスアカウントに付けたポリシーは、境界でも許可された操作だけを実行できます。</p></div>
    {boundaries.isError ? <FormError message={getApiErrorMessage(boundaries.error)} /> : null}
    {boundaries.data?.items.map((item) => <div key={item.principal_id}>{principals.find((entry) => entry.id === item.principal_id)?.name ?? item.principal_id} / {policies.find((entry) => entry.id === item.policy_id)?.name ?? item.policy_id}{" "}
      <Button disabled={!boundaries.data?.enabled || change.isPending} onClick={() => change.mutate({ id: item.principal_id })}>境界を解除</Button></div>)}
    <SpaceBetween direction="horizontal" size="xs">
      <Select ariaLabel="境界を設定するプリンシパル" options={principalOptions} selectedOption={principalOptions.find((item) => item.value === principal) ?? null} onChange={({ detail }) => setPrincipal(detail.selectedOption.value ?? "")} />
      <Select ariaLabel="境界ポリシー" options={policyOptions} selectedOption={policyOptions.find((item) => item.value === policy) ?? null} onChange={({ detail }) => setPolicy(detail.selectedOption.value ?? "")} />
      <Button disabled={!boundaries.data?.enabled || !principal || !policy || change.isPending} onClick={() => change.mutate({ id: principal, policyId: policy })}>境界を設定</Button>
    </SpaceBetween>
    {change.isError ? <FormError message={getApiErrorMessage(change.error)} /> : null}
  </SpaceBetween></section>;
}
