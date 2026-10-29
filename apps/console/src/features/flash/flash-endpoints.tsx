import { Box } from "@/components/ui";
import { CopyToClipboard } from "@/components/ui";
import { Link } from "@/components/ui";
import { SpaceBetween } from "@/components/ui";
import type { ColumnDef } from "@tanstack/react-table";
import { DataTable } from "@/components/shared/data-table";
import type { DisplayFlashEndpoint } from "./flash-service-utils";

const columns: ColumnDef<DisplayFlashEndpoint, unknown>[] = [
  {
    accessorKey: "name",
    header: "ポート",
    cell: ({ getValue }) => <Box fontWeight="bold">{getValue<string>()}</Box>,
  },
  {
    accessorKey: "protocol",
    header: "プロトコル",
  },
  {
    accessorKey: "address",
    header: "接続先",
    cell: ({ getValue, row }) => {
      const address = getValue<string>();
      return (
        <SpaceBetween direction="horizontal" size="xs">
          {row.original.href ? (
            <Link href={row.original.href} external externalIconAriaLabel="新しいタブで開く">{address}</Link>
          ) : <Box variant="code">{address}</Box>}
          <CopyToClipboard
            textToCopy={address}
            copyButtonAriaLabel="接続先をコピー"
            copySuccessText="コピーしました"
            copyErrorText="コピーできませんでした"
          />
        </SpaceBetween>
      );
    },
  },
];

export function FlashEndpoints({
  endpoints,
}: {
  endpoints: DisplayFlashEndpoint[];
}) {
  return (
    <DataTable
      columns={columns}
      data={endpoints}
      getRowId={(endpoint) => endpoint.key}
      mobileVisibleColumns={["name", "address"]}
      searchPlaceholder="ポート名、プロトコル、接続先で検索"
      emptyTitle="エンドポイントはまだありません"
      emptyDescription="サービスの構築完了後に接続先が表示されます。"
    />
  );
}
