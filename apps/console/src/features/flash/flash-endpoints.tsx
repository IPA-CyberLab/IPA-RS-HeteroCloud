import Box from "@cloudscape-design/components/box";
import CopyToClipboard from "@cloudscape-design/components/copy-to-clipboard";
import SpaceBetween from "@cloudscape-design/components/space-between";
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
    cell: ({ getValue }) => {
      const address = getValue<string>();
      return (
        <SpaceBetween direction="horizontal" size="xs">
          <Box variant="code">{address}</Box>
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

