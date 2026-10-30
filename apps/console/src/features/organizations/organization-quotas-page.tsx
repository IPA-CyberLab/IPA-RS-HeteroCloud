import { Box, Button, Header, Modal, SpaceBetween } from "@/components/ui";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { ErrorState } from "@/components/shared/error-state";
import { FormError } from "@/components/shared/form-error";
import { PageHeader } from "@/components/shared/page-header";
import { PageLoading } from "@/components/shared/page-loading";
import { QuotaEditor } from "@/features/operator/quotas-page";
import { useActiveOrganization } from "@/features/organizations/organization-context";
import { QuotaSummary } from "@/features/organizations/quota-summary";
import { api, getApiErrorMessage } from "@/lib/api-client";
import type { OrganizationMemberQuota, ResourceQuotaLimits, UnassignedQuotaResource } from "@/lib/api-types";

function roleLabel(role: OrganizationMemberQuota["role"]) {
  return role === "owner" ? "オーナー" : role === "admin" ? "管理者" : "メンバー";
}

function UnassignedResources({ organizationId, members }: { organizationId: string; members: OrganizationMemberQuota[] }) {
  const client = useQueryClient();
  const key = ["organizations", organizationId, "quota-unassigned"] as const;
  const items = useQuery({ queryKey: key, queryFn: ({ signal }) => api.organizations.unassignedQuotaResources(organizationId, signal) });
  const [selected, setSelected] = useState<Record<string, string>>({});
  const assign = useMutation({
    mutationFn: ({ resource, user }: { resource: UnassignedQuotaResource; user: string }) => api.organizations.assignQuotaResource(organizationId, resource, user),
    onSuccess: async () => { await Promise.all([
      client.invalidateQueries({ queryKey: key }),
      client.invalidateQueries({ queryKey: ["organizations", organizationId, "quotas"] }),
    ]); },
  });
  if (items.isPending) return <PageLoading label="未割当のリソースを確認しています" />;
  if (items.isError) return <ErrorState description="未割当のリソースを取得できませんでした。" onRetry={() => void items.refetch()} />;
  if (!items.data.items.length) return null;
  return <SpaceBetween size="m">
    <Header variant="h2" counter={`(${items.data.items.length})`} description="過去のサービスとサービスアカウントは、利用者を推測せずに表示しています。個人別上限を設定する前に割り当ててください。">割り当て先が未設定</Header>
    <Box color="text-body-secondary">サービスの割り当て後は、そのメンバーの週次上限も適用されます。</Box>
    {assign.isError ? <FormError message={getApiErrorMessage(assign.error)} /> : null}
    {items.data.items.map((resource) => <div key={resource.id} className="flex flex-wrap items-center gap-3">
      <span>{resource.name} · {resource.provider === "service_account" ? "サービスアカウント" : resource.provider}</span>
      <select className="select select-bordered" aria-label={`${resource.name} の割り当て先`} value={selected[resource.id] ?? ""}
        onChange={(event) => setSelected({ ...selected, [resource.id]: event.target.value })}>
        <option value="">メンバーを選択</option>
        {members.map((member) => <option key={member.user_id} value={member.user_id}>{member.display_name} ({member.email})</option>)}
      </select>
      <Button disabled={!selected[resource.id]} loading={assign.isPending}
        onClick={() => assign.mutate({ resource, user: selected[resource.id] })}>割り当て</Button>
    </div>)}
  </SpaceBetween>;
}

export function OrganizationQuotasPage() {
  const { activeOrganization } = useActiveOrganization();
  const id = activeOrganization.organization_id;
  const queryClient = useQueryClient();
  const key = ["organizations", id, "quotas"] as const;
  const quotas = useQuery({ queryKey: key, queryFn: ({ signal }) => api.organizations.quotas(id, signal) });
  const [editing, setEditing] = useState<OrganizationMemberQuota | null>(null);
  const [limits, setLimits] = useState<ResourceQuotaLimits | null>(null);
  useEffect(() => { setEditing(null); setLimits(null); }, [id]);
  const refresh = async () => { await queryClient.invalidateQueries({ queryKey: key }); };
  const save = useMutation({
    mutationFn: ({ user, value }: { user: string; value: ResourceQuotaLimits }) => api.organizations.updateMemberQuota(id, user, value),
    onSuccess: async () => { setEditing(null); setLimits(null); await refresh(); },
  });
  const reset = useMutation({
    mutationFn: (user: string) => api.organizations.clearMemberQuota(id, user),
    onSuccess: async () => { setEditing(null); setLimits(null); await refresh(); },
  });
  const changeRole = useMutation({
    mutationFn: ({ user, role }: { user: string; role: "admin" | "member" }) => api.organizations.updateMemberRole(id, user, role),
    onSuccess: refresh,
  });

  if (quotas.isPending) return <PageLoading label="リソース上限を読み込んでいます" />;
  if (quotas.isError) return <ErrorState description="リソース上限を取得できませんでした。" onRetry={() => void quotas.refetch()} />;
  const { member, members, organization } = quotas.data;
  return <SpaceBetween size="l">
    <PageHeader title="リソース上限" description={`${activeOrganization.organization_name} の適用中の上限です。個人の上限と組織全体の上限の両方が適用されます。`} />
    <Header variant="h2" description="上限は予約枠ではありません。組織全体の空きも必要です。">自分の上限</Header>
    <QuotaSummary limits={member.effective_limits} member />
    {organization ? <>
      <UnassignedResources organizationId={id} members={members} />
      <details><summary className="cursor-pointer font-semibold">組織全体の上限を表示</summary>
        <QuotaSummary limits={organization.effective_limits} />
      </details>
      <Header variant="h2" counter={`(${members.length})`} description="各メンバーの上限は、組織の上限を超えられません。">メンバーの上限</Header>
      {changeRole.isError ? <FormError message={getApiErrorMessage(changeRole.error)} /> : null}
      {members.map((item) => <details key={item.user_id}>
        <summary className="cursor-pointer font-semibold">{item.display_name} · {item.email} · {roleLabel(item.role)}</summary>
        <SpaceBetween size="m">
          <Box color="text-body-secondary">{item.override_limits ? "個別上限を適用中" : "組織の上限を継承中"}</Box>
          <SpaceBetween direction="horizontal" size="xs">
            <Button onClick={() => { save.reset(); reset.reset(); setEditing(item); setLimits(item.effective_limits); }}>上限を変更</Button>
            {activeOrganization.role === "owner" && item.role !== "owner" ? <Button
              loading={changeRole.isPending}
              onClick={() => changeRole.mutate({ user: item.user_id, role: item.role === "admin" ? "member" : "admin" })}
            >{item.role === "admin" ? "管理者を解除" : "管理者にする"}</Button> : null}
          </SpaceBetween>
          <QuotaSummary limits={item.effective_limits} member />
        </SpaceBetween>
      </details>)}
    </> : null}
    <Modal visible={editing !== null && limits !== null} size="max" header={`${editing?.display_name ?? ""} の上限を変更`} onDismiss={() => setEditing(null)}
      footer={<Box float="right"><SpaceBetween direction="horizontal" size="xs">
        {editing?.override_limits ? <Button loading={reset.isPending} onClick={() => reset.mutate(editing.user_id)}>組織の上限に戻す</Button> : null}
        <Button onClick={() => setEditing(null)}>キャンセル</Button>
        <Button variant="primary" loading={save.isPending} onClick={() => { if (editing && limits) save.mutate({ user: editing.user_id, value: limits }); }}>保存</Button>
      </SpaceBetween></Box>}>
      <SpaceBetween size="m">
        {save.isError ? <FormError message={getApiErrorMessage(save.error)} /> : null}
        {reset.isError ? <FormError message={getApiErrorMessage(reset.error)} /> : null}
        <Box color="text-body-secondary">各値には組織全体の上限も適用されます。</Box>
        {limits ? <QuotaEditor value={limits} onChange={setLimits} member /> : null}
      </SpaceBetween>
    </Modal>
  </SpaceBetween>;
}
