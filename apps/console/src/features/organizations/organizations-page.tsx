import Badge from "@cloudscape-design/components/badge";
import Box from "@cloudscape-design/components/box";
import SpaceBetween from "@cloudscape-design/components/space-between";
import { useQuery } from "@tanstack/react-query";
import type { ColumnDef } from "@tanstack/react-table";
import { useMemo } from "react";
import { DataTable } from "@/components/shared/data-table";
import { ErrorState } from "@/components/shared/error-state";
import { PageHeader } from "@/components/shared/page-header";
import { PageLoading } from "@/components/shared/page-loading";
import { useSession } from "@/features/auth/session";
import type { Organization } from "@/lib/api-types";
import { organizationsQueryOptions } from "@/lib/queries";
import { formatDateTime } from "@/lib/utils";

export function OrganizationsPage() {
  const organizations = useQuery(organizationsQueryOptions);
  const memberships = useSession().data?.memberships ?? [];

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
              {role === "owner" ? "オーナー" : "メンバー"}
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
      />
      <DataTable
        columns={columns}
        data={organizations.data.items}
        getRowId={(organization) => organization.id}
        searchPlaceholder="組織名、組織ID、メンバーシップで検索"
        emptyTitle="組織がありません"
        emptyDescription="有効な招待コードで登録すると、参加先の組織が表示されます。"
      />
    </SpaceBetween>
  );
}
