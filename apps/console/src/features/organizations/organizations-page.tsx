import { Badge } from "@/components/ui";
import { Box } from "@/components/ui";
import { Button, FormField, Input, Modal } from "@/components/ui";
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
import { api, getApiErrorMessage } from "@/lib/api-client";
import type { Organization } from "@/lib/api-types";
import { organizationsQueryOptions } from "@/lib/queries";
import { formatDateTime } from "@/lib/utils";

export function OrganizationsPage() {
  const organizations = useQuery(organizationsQueryOptions);
  const memberships = useSession().data?.memberships ?? [];
  const { setActiveOrganizationId } = useActiveOrganization();
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [slug, setSlug] = useState("");
  const [joinOpen, setJoinOpen] = useState(false);
  const [invitationCode, setInvitationCode] = useState("");
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
        header: "組織",
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
        header: "組織ID",
        cell: ({ getValue }) => <Box variant="code">{getValue<string>()}</Box>,
      },
      {
        accessorKey: "created_at",
        header: "作成日時",
        cell: ({ getValue }) => formatDateTime(getValue<string>()),
      },
    ],
    [memberships],
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
        title="組織"
        description="参加しているテナント境界とメンバーシップを確認します。"
        actions={<SpaceBetween direction="horizontal" size="xs">
          <Button onClick={() => { joinOrganization.reset(); setJoinOpen(true); }}>招待コードで参加</Button>
          <Button variant="primary" iconName="add-plus" onClick={() => {
            createOrganization.reset();
            setOpen(true);
          }}>組織を作成</Button>
        </SpaceBetween>}
      />
      <DataTable
        columns={columns}
        data={organizations.data.items}
        getRowId={(organization) => organization.id}
        searchPlaceholder="組織名、組織ID、メンバーシップで検索"
        emptyTitle="組織がありません"
        emptyDescription="有効な招待コードで登録すると、参加先の組織が表示されます。"
      />
      <Modal
        visible={open}
        onDismiss={() => setOpen(false)}
        header="組織を作成"
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
          <FormField label="組織名"><Input value={name} onChange={({ detail }) => setName(detail.value)} /></FormField>
          <FormField label="組織の識別子" description="英小文字・数字・ハイフン、3〜63文字。">
            <Input value={slug} onChange={({ detail }) => setSlug(detail.value)} />
          </FormField>
        </SpaceBetween>
      </Modal>
      <Modal visible={joinOpen} onDismiss={() => setJoinOpen(false)} header="組織に参加"
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
