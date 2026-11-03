import { Badge } from "@/components/ui";
import { Box } from "@/components/ui";
import { Button, FormField, Input, Modal, Select } from "@/components/ui";
import { SpaceBetween } from "@/components/ui";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { ColumnDef } from "@tanstack/react-table";
import { useMemo, useState } from "react";
import { DataTable } from "@/components/shared/data-table";
import { ErrorState } from "@/components/shared/error-state";
import { FormError } from "@/components/shared/form-error";
import { PageHeader } from "@/components/shared/page-header";
import { PageLoading } from "@/components/shared/page-loading";
import { useSession } from "@/features/auth/session";
import { useActiveOrganization } from "@/features/organizations/organization-context";
import { ServiceControlPolicies } from "@/features/organizations/service-control-policies";
import { api, getApiErrorMessage } from "@/lib/api-client";
import type { Organization } from "@/lib/api-types";
import { organizationsQueryOptions } from "@/lib/queries";
import { formatDateTime } from "@/lib/utils";

type RenameTarget = { kind: "account" | "organization" | "member" | "unit"; id: string; name: string };

export function OrganizationsPage() {
  const organizations = useQuery(organizationsQueryOptions);
  const memberships = useSession().data?.memberships ?? [];
  const { activeOrganization, setActiveOrganizationId } = useActiveOrganization();
  const accountId = activeOrganization.organization_id;
  const cloudOrganization = useQuery({
    queryKey: ["cloud-organization", accountId],
    queryFn: ({ signal }) => api.organizations.describeCloudOrganization(accountId, signal),
  });
  const tree = useQuery({
    queryKey: ["cloud-organization", accountId, "tree"],
    queryFn: ({ signal }) => api.organizations.tree(accountId, signal),
    enabled: cloudOrganization.data?.management_account_id === accountId,
  });
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [slug, setSlug] = useState("");
  const [joinOpen, setJoinOpen] = useState(false);
  const [invitationCode, setInvitationCode] = useState("");
  const [treeModal, setTreeModal] = useState<"account" | "unit" | null>(null);
  const [treeName, setTreeName] = useState("");
  const [treeSlug, setTreeSlug] = useState("");
  const [parentId, setParentId] = useState("");
  const [renameTarget, setRenameTarget] = useState<RenameTarget | null>(null);
  const [renameName, setRenameName] = useState("");
  const rename = useMutation({
    mutationFn: async ({ target, value }: { target: RenameTarget; value: string }) => {
      switch (target.kind) {
        case "account": return api.organizations.renameAccount(target.id, value);
        case "organization": return api.organizations.renameCloudOrganization(accountId, value);
        case "member": return api.organizations.renameMemberAccount(accountId, target.id, value);
        case "unit": return api.organizations.renameUnit(accountId, target.id, value);
      }
    },
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["auth", "session"] }),
        queryClient.invalidateQueries({ queryKey: ["organizations"] }),
        queryClient.invalidateQueries({ queryKey: ["cloud-organization"] }),
      ]);
      setRenameTarget(null);
    },
  });
  const openRename = (target: RenameTarget) => {
    rename.reset();
    setRenameName(target.name);
    setRenameTarget(target);
  };
  const refreshTree = () => queryClient.invalidateQueries({ queryKey: ["cloud-organization", accountId, "tree"] });
  const createTreeEntry = useMutation({
    mutationFn: async () => {
      const parent = parentId || tree.data?.organization.root_id;
      if (!parent) throw new Error("親を選択してください。");
      if (treeModal === "account") {
        await api.organizations.createMemberAccount(accountId, { name: treeName.trim(), slug: treeSlug, parent_id: parent });
        await Promise.all([
          queryClient.invalidateQueries({ queryKey: ["auth", "session"] }),
          queryClient.invalidateQueries({ queryKey: ["organizations"] }),
        ]);
      } else {
        await api.organizations.createUnit(accountId, { name: treeName.trim(), parent_id: parent });
      }
    },
    onSuccess: async () => { await refreshTree(); setTreeModal(null); setTreeName(""); setTreeSlug(""); setParentId(""); },
  });
  const moveAccount = useMutation({
    mutationFn: ({ id, parent }: { id: string; parent: string }) => api.organizations.moveMemberAccount(accountId, id, parent),
    onSuccess: refreshTree,
  });
  const valid = name.trim().length > 0 && /^[a-z][a-z0-9-]{1,61}[a-z0-9]$/.test(slug);
  const refreshMemberships = async (organization: Organization) => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["auth", "session"] }),
      queryClient.invalidateQueries({ queryKey: ["organizations"] }),
    ]);
    setActiveOrganizationId(organization.id);
  };
  const createOrganization = useMutation({
    mutationFn: api.organizations.create,
    onSuccess: async (organization) => {
      await refreshMemberships(organization);
      setName("");
      setSlug("");
      setOpen(false);
    },
  });
  const joinOrganization = useMutation({
    mutationFn: api.organizations.join,
    onSuccess: async (organization) => {
      await refreshMemberships(organization);
      setInvitationCode("");
      setJoinOpen(false);
    },
  });

  const columns = useMemo<ColumnDef<Organization, unknown>[]>(
    () => [
      {
        accessorKey: "name",
        header: "アカウント",
        cell: ({ row }) => (
          <SpaceBetween size="xxs">
            <Box fontWeight="bold">{row.original.name}</Box>
            <Box color="text-body-secondary">{row.original.slug}</Box>
          </SpaceBetween>
        ),
      },
      {
        id: "membershipRole",
        accessorFn: (organization) =>
          memberships.find(
            (membership) => membership.organization_id === organization.id,
          )?.role ?? "",
        header: "メンバーシップ",
        cell: ({ row }) => {
          const role = memberships.find(
            (membership) => membership.organization_id === row.original.id,
          )?.role;
          return role ? (
            <Badge color={role === "owner" ? "green" : "blue"}>
              {role === "owner" ? "オーナー" : role === "admin" ? "管理者" : "メンバー"}
            </Badge>
          ) : (
            "-"
          );
        },
      },
      {
        accessorKey: "id",
        header: "内部ID",
        cell: ({ getValue }) => <Box variant="code">{getValue<string>()}</Box>,
      },
      {
        id: "switch",
        header: "操作",
        cell: ({ row }) => <SpaceBetween direction="horizontal" size="xs">
          <Button disabled={row.original.id === accountId} onClick={() => setActiveOrganizationId(row.original.id)}>{row.original.id === accountId ? "選択中" : "切り替え"}</Button>
          <Button ariaLabel={`${row.original.name}のアカウント名を変更`} onClick={() => openRename({ kind: "account", id: row.original.id, name: row.original.name })}>名前を変更</Button>
        </SpaceBetween>,
      },
      {
        accessorKey: "created_at",
        header: "作成日時",
        cell: ({ getValue }) => formatDateTime(getValue<string>()),
      },
    ],
    [memberships, accountId, setActiveOrganizationId, rename],
  );

  if (organizations.isPending) return <PageLoading label="組織を読み込んでいます" />;
  if (organizations.isError) {
    return (
      <ErrorState
        description="組織一覧を取得できませんでした。"
        onRetry={() => void organizations.refetch()}
      />
    );
  }

  return (
    <SpaceBetween size="l">
      <PageHeader
        title="アカウントと組織"
        description="アカウントはサービスや上限の分離単位です。組織は管理アカウントとメンバーアカウントをまとめます。"
        actions={<SpaceBetween direction="horizontal" size="xs">
          <Button onClick={() => { joinOrganization.reset(); setJoinOpen(true); }}>招待コードで参加</Button>
          <Button variant="primary" iconName="add-plus" onClick={() => {
            createOrganization.reset();
            setOpen(true);
          }}>新しい組織を作成</Button>
        </SpaceBetween>}
      />
      <DataTable
        columns={columns}
        data={organizations.data.items}
        getRowId={(organization) => organization.id}
        searchPlaceholder="アカウント名、ID、メンバーシップで検索"
        emptyTitle="アカウントがありません"
        emptyDescription="有効な招待コードで参加すると、アカウントが表示されます。"
      />
      {cloudOrganization.data ? <section className="card bg-base-100 border border-base-300 p-5">
        <SpaceBetween size="m">
          <div><h2 className="text-lg font-semibold">組織: {cloudOrganization.data.name}</h2>
            <p>このアカウント: {cloudOrganization.data.current_account_number} / 管理アカウント: {cloudOrganization.data.management_account_number}</p></div>
          {cloudOrganization.data.management_account_id !== accountId ? <p>組織全体の管理は管理アカウントから行います。</p> : <>
            <SpaceBetween direction="horizontal" size="xs">
              <Button onClick={() => openRename({ kind: "organization", id: cloudOrganization.data.id, name: cloudOrganization.data.name })}>組織名を変更</Button>
              <Button onClick={() => { createTreeEntry.reset(); setTreeModal("unit"); }}>OUを作成</Button>
              <Button variant="primary" onClick={() => { createTreeEntry.reset(); setTreeModal("account"); }}>メンバーアカウントを作成</Button>
            </SpaceBetween>
            {tree.isError ? <FormError message={getApiErrorMessage(tree.error)} /> : null}
            {tree.isPending ? <p>組織を読み込んでいます...</p> : null}
            {tree.data ? <>
              <div><h3 className="font-semibold">組織単位</h3>
                <ul className="list-disc pl-5">{tree.data.units.map((unit) => <li key={unit.id}>{unit.name}{unit.parent_id ? `（親: ${tree.data?.units.find((parent) => parent.id === unit.parent_id)?.name ?? "-"}）` : "（ルート）"} {unit.parent_id ? <Button ariaLabel={`${unit.name}の組織単位名を変更`} onClick={() => openRename({ kind: "unit", id: unit.id, name: unit.name })}>名前を変更</Button> : null}</li>)}</ul>
              </div>
              <div className="overflow-x-auto"><table className="table"><thead><tr><th>アカウント</th><th>アカウントID</th><th>組織単位</th><th>操作</th></tr></thead>
                <tbody>{tree.data.accounts.map((account) => <tr key={account.id}><td>{account.name}{account.is_management_account ? "（管理）" : ""}</td><td>{account.account_number}</td><td>
                  {account.is_management_account ? "Root" : <Select selectedOption={{ value: account.parent_id, label: tree.data?.units.find((unit) => unit.id === account.parent_id)?.name ?? "-" }}
                    options={tree.data?.units.map((unit) => ({ value: unit.id, label: unit.parent_id ? unit.name : "Root" })) ?? []}
                    onChange={({ detail }) => { if (detail.selectedOption.value) moveAccount.mutate({ id: account.id, parent: detail.selectedOption.value }); }} />}
                </td><td><Button ariaLabel={`${account.name}のメンバーアカウント名を変更`} onClick={() => openRename({ kind: "member", id: account.id, name: account.name })}>名前を変更</Button></td></tr>)}</tbody></table></div>
              {moveAccount.isError ? <FormError message={getApiErrorMessage(moveAccount.error)} /> : null}
            </> : null}
          </>}
        </SpaceBetween>
      </section> : cloudOrganization.isError ? <FormError message={getApiErrorMessage(cloudOrganization.error)} /> : null}
      {tree.data && cloudOrganization.data?.management_account_id === accountId ? <ServiceControlPolicies accountId={accountId} tree={tree.data} /> : null}
      <Modal visible={renameTarget !== null} onDismiss={() => setRenameTarget(null)} header="名前を変更"
        footer={<Box float="right"><SpaceBetween direction="horizontal" size="xs">
          <Button onClick={() => setRenameTarget(null)}>キャンセル</Button>
          <Button variant="primary" loading={rename.isPending} disabled={!renameName.trim() || Array.from(renameName.trim()).length > 120}
            onClick={() => { if (renameTarget) rename.mutate({ target: renameTarget, value: renameName.trim() }); }}>保存</Button>
        </SpaceBetween></Box>}>
        <SpaceBetween size="m">
          <FormField label="名前"><Input value={renameName} onChange={({ detail }) => setRenameName(detail.value)} /></FormField>
          <FormError message={rename.isError ? getApiErrorMessage(rename.error) : null} />
        </SpaceBetween>
      </Modal>
      <Modal visible={treeModal !== null} onDismiss={() => setTreeModal(null)} header={treeModal === "account" ? "メンバーアカウントを作成" : "組織単位を作成"}
        footer={<Box float="right"><SpaceBetween direction="horizontal" size="xs"><Button onClick={() => setTreeModal(null)}>キャンセル</Button>
          <Button variant="primary" loading={createTreeEntry.isPending} disabled={!treeName.trim() || (treeModal === "account" && !/^[a-z][a-z0-9-]{1,61}[a-z0-9]$/.test(treeSlug))} onClick={() => createTreeEntry.mutate()}>作成</Button></SpaceBetween></Box>}>
        <SpaceBetween size="m">
          <FormField label="名前"><Input value={treeName} onChange={({ detail }) => setTreeName(detail.value)} /></FormField>
          {treeModal === "account" ? <FormField label="アカウントの識別子" description="英小文字・数字・ハイフン、3〜63文字。"><Input value={treeSlug} onChange={({ detail }) => setTreeSlug(detail.value)} /></FormField> : null}
          <FormField label="親の組織単位"><Select selectedOption={{ value: parentId || tree.data?.organization.root_id, label: tree.data?.units.find((unit) => unit.id === (parentId || tree.data?.organization.root_id))?.name ?? "Root" }}
            options={tree.data?.units.map((unit) => ({ value: unit.id, label: unit.parent_id ? unit.name : "Root" })) ?? []} onChange={({ detail }) => setParentId(detail.selectedOption.value ?? "")} /></FormField>
          {createTreeEntry.isError ? <FormError message={getApiErrorMessage(createTreeEntry.error)} /> : null}
        </SpaceBetween>
      </Modal>
      <Modal
        visible={open}
        onDismiss={() => setOpen(false)}
        header="新しい組織を作成"
        footer={<Box float="right"><SpaceBetween direction="horizontal" size="xs">
          <Button onClick={() => setOpen(false)}>キャンセル</Button>
          <Button variant="primary" disabled={!valid} loading={createOrganization.isPending}
            onClick={() => createOrganization.mutate({ name: name.trim(), slug })}>
            作成
          </Button>
        </SpaceBetween></Box>}
      >
        <SpaceBetween size="m">
          {createOrganization.isError ? <FormError message={getApiErrorMessage(createOrganization.error)} /> : null}
          <FormField label="組織と管理アカウントの名前"><Input value={name} onChange={({ detail }) => setName(detail.value)} /></FormField>
          <FormField label="管理アカウントの識別子" description="英小文字・数字・ハイフン、3〜63文字。">
            <Input value={slug} onChange={({ detail }) => setSlug(detail.value)} />
          </FormField>
        </SpaceBetween>
      </Modal>
      <Modal visible={joinOpen} onDismiss={() => setJoinOpen(false)} header="アカウントに参加"
        footer={<Box float="right"><SpaceBetween direction="horizontal" size="xs">
          <Button onClick={() => setJoinOpen(false)}>キャンセル</Button>
          <Button variant="primary" disabled={!invitationCode.trim()} loading={joinOrganization.isPending}
            onClick={() => joinOrganization.mutate(invitationCode.trim())}>参加</Button>
        </SpaceBetween></Box>}>
        <SpaceBetween size="m">
          {joinOrganization.isError ? <FormError message={getApiErrorMessage(joinOrganization.error)} /> : null}
          <FormField label="招待コード"><Input value={invitationCode} onChange={({ detail }) => setInvitationCode(detail.value)} /></FormField>
        </SpaceBetween>
      </Modal>
    </SpaceBetween>
  );
}
