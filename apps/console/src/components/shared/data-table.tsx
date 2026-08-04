import Pagination from "@cloudscape-design/components/pagination";
import Box from "@cloudscape-design/components/box";
import Link from "@cloudscape-design/components/link";
import SpaceBetween from "@cloudscape-design/components/space-between";
import Table, { type TableProps } from "@cloudscape-design/components/table";
import TextFilter from "@cloudscape-design/components/text-filter";
import {
  type ColumnDef,
  type SortingState,
  flexRender,
  getCoreRowModel,
  getFilteredRowModel,
  getPaginationRowModel,
  getSortedRowModel,
  useReactTable,
} from "@tanstack/react-table";
import { type MouseEvent, useEffect, useMemo, useState } from "react";
import { EmptyState } from "@/components/shared/empty-state";
import { formatNumber } from "@/lib/utils";

interface DataTableProps<TData> {
  columns: ColumnDef<TData, unknown>[];
  data: TData[];
  searchPlaceholder?: string;
  emptyTitle?: string;
  emptyDescription?: string;
  initialPageSize?: number;
  getRowId?: (row: TData) => string;
  onRowClick?: (row: TData) => void;
  getRowAriaLabel?: (row: TData) => string;
  mobileVisibleColumns?: string[];
}

const interactiveElementSelector = [
  "a",
  "button",
  "input",
  "select",
  "textarea",
  "[role='button']",
  "[role='link']",
  "[role='checkbox']",
  "[role='menuitem']",
].join(",");

function stopInteractiveClick(event: MouseEvent<HTMLElement>) {
  if (
    event.target instanceof Element &&
    event.target.closest(interactiveElementSelector)
  ) {
    event.stopPropagation();
  }
}

function useMediaQuery(query: string) {
  const [matches, setMatches] = useState(() =>
    typeof window !== "undefined" ? window.matchMedia(query).matches : false,
  );

  useEffect(() => {
    const media = window.matchMedia(query);
    const update = () => setMatches(media.matches);
    update();
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, [query]);

  return matches;
}

export function DataTable<TData>({
  columns,
  data,
  searchPlaceholder = "検索",
  emptyTitle = "データがありません",
  emptyDescription = "条件に一致するデータはありません。",
  initialPageSize = 10,
  getRowId,
  onRowClick,
  getRowAriaLabel,
  mobileVisibleColumns,
}: DataTableProps<TData>) {
  const [sorting, setSorting] = useState<SortingState>([]);
  const [globalFilter, setGlobalFilter] = useState("");

  const table = useReactTable({
    data,
    columns,
    state: { sorting, globalFilter },
    initialState: { pagination: { pageSize: initialPageSize } },
    getRowId,
    onSortingChange: setSorting,
    onGlobalFilterChange: setGlobalFilter,
    getCoreRowModel: getCoreRowModel(),
    getFilteredRowModel: getFilteredRowModel(),
    getSortedRowModel: getSortedRowModel(),
    getPaginationRowModel: getPaginationRowModel(),
  });

  const rows = table.getRowModel().rows;
  const filteredCount = table.getFilteredRowModel().rows.length;
  const pageIndex = table.getState().pagination.pageIndex;
  const mobile = useMediaQuery("(max-width: 767px)");
  const rowByItem = useMemo(
    () => new Map(rows.map((row) => [row.original, row])),
    [rows],
  );

  const tableColumns = useMemo<TableProps.ColumnDefinition<TData>[]>(
    () =>
      table.getFlatHeaders().map((header, index) => ({
        id: header.column.id,
        header: flexRender(header.column.columnDef.header, header.getContext()),
        isRowHeader: index === 0,
        sortingField: header.column.getCanSort() ? header.column.id : undefined,
        cell: (item) => {
          const row = rowByItem.get(item);
          const cell = row
            ?.getVisibleCells()
            .find((candidate) => candidate.column.id === header.column.id);
          const content = cell
            ? flexRender(cell.column.columnDef.cell, cell.getContext())
            : null;
          return index === 0 && getRowAriaLabel && onRowClick ? (
            <span onClick={(event) => event.stopPropagation()}>
              <Link
                href="#"
                ariaLabel={getRowAriaLabel(item)}
                onFollow={(event) => {
                  event.preventDefault();
                  onRowClick(item);
                }}
              >
                {content}
              </Link>
            </span>
          ) : (
            <span onClick={stopInteractiveClick}>{content}</span>
          );
        },
      })),
    [getRowAriaLabel, onRowClick, rowByItem, table],
  );

  const sortingColumn = sorting[0]
    ? tableColumns.find((column) => column.id === sorting[0].id)
    : undefined;
  const columnDisplay =
    mobile && mobileVisibleColumns
      ? tableColumns.map((column) => ({
          id: String(column.id),
          visible: mobileVisibleColumns.includes(String(column.id)),
        }))
      : undefined;

  return (
    <Table
      variant="container"
      stickyHeader
      stripedRows
      wrapLines
      trackBy={getRowId}
      items={rows.map((row) => row.original)}
      columnDefinitions={tableColumns}
      columnDisplay={columnDisplay}
      sortingColumn={sortingColumn}
      sortingDescending={sorting[0]?.desc}
      onSortingChange={({ detail }) => {
        const id = (detail.sortingColumn as { id?: string }).id;
        if (id) setSorting([{ id, desc: detail.isDescending ?? false }]);
      }}
      onRowClick={
        onRowClick ? ({ detail }) => onRowClick(detail.item) : undefined
      }
      filter={
        <SpaceBetween direction="horizontal" size="m" alignItems="center">
          <TextFilter
            filteringText={globalFilter}
            filteringPlaceholder={searchPlaceholder}
            filteringAriaLabel="テーブルを検索"
            onChange={({ detail }) => {
              setGlobalFilter(detail.filteringText);
              table.setPageIndex(0);
            }}
          />
          <Box color="text-body-secondary">{formatNumber(filteredCount)} 件</Box>
        </SpaceBetween>
      }
      pagination={
        filteredCount > 0 ? (
          <Pagination
            currentPageIndex={pageIndex + 1}
            pagesCount={Math.max(1, table.getPageCount())}
            onChange={({ detail }) => table.setPageIndex(detail.currentPageIndex - 1)}
            ariaLabels={{
              nextPageLabel: "次のページ",
              previousPageLabel: "前のページ",
              pageLabel: (page) => `${page}ページ`,
            }}
          />
        ) : null
      }
      empty={
        <EmptyState title={emptyTitle} description={emptyDescription} />
      }
      ariaLabels={{
        tableLabel: "リソース一覧",
        sortAscending: "昇順に並べ替え",
        sortDescending: "降順に並べ替え",
      }}
    />
  );
}
