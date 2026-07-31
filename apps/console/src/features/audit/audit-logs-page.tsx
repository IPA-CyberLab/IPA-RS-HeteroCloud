import { useQuery } from "@tanstack/react-query";
import type { ColumnDef } from "@tanstack/react-table";
import { FileClock } from "lucide-react";
import { useMemo } from "react";
import { DataTable } from "@/components/shared/data-table";
import { ErrorState } from "@/components/shared/error-state";
import { PageHeader } from "@/components/shared/page-header";
import { PageLoading } from "@/components/shared/page-loading";
import { StatusBadge } from "@/components/shared/status-badge";
import { useActiveOrganization } from "@/features/organizations/organization-context";
import type { AuditEvent } from "@/lib/api-types";
import { auditEventsQueryOptions } from "@/lib/queries";
import { formatDateTime, formatNumber } from "@/lib/utils";

export function AuditLogsPage() {
  const { activeOrganization } = useActiveOrganization();
  const events = useQuery(
    auditEventsQueryOptions(activeOrganization.organization_id),
  );

  const columns = useMemo<ColumnDef<AuditEvent, unknown>[]>(
    () => [
      {
        accessorKey: "occurred_at",
        header: "日時",
        cell: ({ getValue }) => formatDateTime(getValue<string>()),
      },
      {
        id: "actor",
        accessorFn: (event) =>
          `${event.principal_id ?? ""} ${event.user_id ?? ""}`,
        header: "実行者",
        cell: ({ row }) => (
          <div>
            <div className="font-mono text-xs text-zinc-800">
              {row.original.principal_id ?? "system"}
            </div>
            {row.original.user_id ? (
              <div className="font-mono text-xs text-zinc-500">
                user: {row.original.user_id}
              </div>
            ) : null}
          </div>
        ),
      },
      {
        accessorKey: "action",
        header: "アクション",
        cell: ({ getValue }) => (
          <code className="rounded-[4px] bg-zinc-100 px-1.5 py-1 text-xs text-zinc-700">
            {getValue<string>()}
          </code>
        ),
      },
      {
        accessorKey: "resource",
        header: "リソース",
        cell: ({ getValue }) => (
          <span className="font-mono text-xs">{getValue<string>()}</span>
        ),
      },
      {
        accessorKey: "decision",
        header: "判定",
        cell: ({ getValue }) => (
          <StatusBadge status={getValue<AuditEvent["decision"]>()} />
        ),
      },
      {
        accessorKey: "reason",
        header: "理由",
      },
      {
        accessorKey: "source_ip",
        header: "送信元IP",
        cell: ({ getValue }) => getValue<string | null>() ?? "—",
      },
      {
        accessorKey: "request_id",
        header: "リクエストID",
        cell: ({ getValue }) => (
          <span className="font-mono text-xs">{getValue<string>()}</span>
        ),
      },
    ],
    [],
  );

  if (events.isPending) {
    return <PageLoading label="監査イベントを読み込んでいます" />;
  }

  if (events.isError) {
    return (
      <ErrorState
        description="監査イベントを取得できませんでした。"
        onRetry={() => void events.refetch()}
      />
    );
  }

  return (
    <div className="space-y-6">
      <PageHeader
        title="監査ログ"
        description={`${activeOrganization.organization_name} のIAM判定とリソース操作を確認します。`}
        actions={
          <span className="flex h-9 items-center gap-2 text-sm text-zinc-500">
            <FileClock className="size-4" />
            {formatNumber(events.data.items.length)} 件
          </span>
        }
      />
      <DataTable
        columns={columns}
        data={events.data.items}
        getRowId={(event) => String(event.id)}
        initialPageSize={20}
        searchPlaceholder="プリンシパル、アクション、リソース、IPで検索"
        emptyTitle="監査イベントがありません"
        emptyDescription="認可対象の操作が行われると、ここに監査イベントが表示されます。"
      />
    </div>
  );
}
