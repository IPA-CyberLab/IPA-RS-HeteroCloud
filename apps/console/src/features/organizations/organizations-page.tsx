import { useQuery } from "@tanstack/react-query";
import type { ColumnDef } from "@tanstack/react-table";
import { Building2 } from "lucide-react";
import { useMemo } from "react";
import { DataTable } from "@/components/shared/data-table";
import { ErrorState } from "@/components/shared/error-state";
import { PageHeader } from "@/components/shared/page-header";
import { PageLoading } from "@/components/shared/page-loading";
import { Badge } from "@/components/ui/badge";
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
          <div className="flex items-center gap-3">
            <span className="flex size-8 items-center justify-center rounded-[5px] bg-zinc-100 text-zinc-600">
              <Building2 className="size-4" />
            </span>
            <div>
              <div className="font-medium text-zinc-900">{row.original.name}</div>
              <div className="text-xs text-zinc-500">{row.original.slug}</div>
            </div>
          </div>
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
            <Badge variant={role === "owner" ? "success" : "neutral"}>
              {role === "owner" ? "オーナー" : "メンバー"}
            </Badge>
          ) : (
            "—"
          );
        },
      },
      {
        accessorKey: "id",
        header: "組織ID",
        cell: ({ getValue }) => (
          <span className="font-mono text-xs">{getValue<string>()}</span>
        ),
      },
      {
        accessorKey: "created_at",
        header: "作成日時",
        cell: ({ getValue }) => formatDateTime(getValue<string>()),
      },
    ],
    [memberships],
  );

  if (organizations.isPending) {
    return <PageLoading label="組織を読み込んでいます" />;
  }

  if (organizations.isError) {
    return (
      <ErrorState
        description="組織一覧を取得できませんでした。"
        onRetry={() => void organizations.refetch()}
      />
    );
  }

  return (
    <div className="space-y-6">
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
    </div>
  );
}
