import Box from "@cloudscape-design/components/box";
import SpaceBetween from "@cloudscape-design/components/space-between";
import { useQuery } from "@tanstack/react-query";
import type { ColumnDef } from "@tanstack/react-table";
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
  const events = useQuery(auditEventsQueryOptions(activeOrganization.organization_id));
  const columns = useMemo<ColumnDef<AuditEvent, unknown>[]>(
    () => [
      {
        accessorKey: "occurred_at",
        header: "日時",
        cell: ({ getValue }) => formatDateTime(getValue<string>()),
      },
      {
        id: "actor",
        accessorFn: (event) => `${event.principal_id ?? ""} ${event.user_id ?? ""}`,
        header: "実行者",
        cell: ({ row }) => (
          <SpaceBetween size="xxs">
            <Box variant="code">{row.original.principal_id ?? "system"}</Box>
            {row.original.user_id ? (
              <Box color="text-body-secondary">user: {row.original.user_id}</Box>
            ) : null}
          </SpaceBetween>
        ),
      },
      {
        accessorKey: "action",
        header: "アクション",
        cell: ({ getValue }) => <Box variant="code">{getValue<string>()}</Box>,
      },
      {
        accessorKey: "resource",
        header: "リソース",
        cell: ({ getValue }) => <Box variant="code">{getValue<string>()}</Box>,
      },
      {
        accessorKey: "decision",
        header: "判定",
        cell: ({ getValue }) => <StatusBadge status={getValue<AuditEvent["decision"]>()} />,
      },
      { accessorKey: "reason", header: "理由" },
      {
        accessorKey: "source_ip",
        header: "送信元IP",
        cell: ({ getValue }) => getValue<string | null>() ?? "-",
      },
      {
        accessorKey: "request_id",
        header: "リクエストID",
        cell: ({ getValue }) => <Box variant="code">{getValue<string>()}</Box>,
      },
    ],
    [],
  );

  if (events.isPending) return <PageLoading label="監査イベントを読み込んでいます" />;
  if (events.isError) {
    return (
      <ErrorState
        description="監査イベントを取得できませんでした。"
        onRetry={() => void events.refetch()}
      />
    );
  }

  return (
    <SpaceBetween size="l">
      <PageHeader
        title="監査ログ"
        description={`${activeOrganization.organization_name} のIAM判定とリソース操作を確認します。`}
        actions={<Box color="text-body-secondary">{formatNumber(events.data.items.length)} 件</Box>}
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
    </SpaceBetween>
  );
}
