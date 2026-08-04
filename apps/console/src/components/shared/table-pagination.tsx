import Box from "@cloudscape-design/components/box";
import Pagination from "@cloudscape-design/components/pagination";
import SpaceBetween from "@cloudscape-design/components/space-between";
import { formatNumber } from "@/lib/utils";

interface TablePaginationProps {
  pageIndex: number;
  pageCount: number;
  pageSize: number;
  totalItems: number;
  onPageChange: (pageIndex: number) => void;
}

export function TablePagination({
  pageIndex,
  pageCount,
  pageSize,
  totalItems,
  onPageChange,
}: TablePaginationProps) {
  const firstItem = totalItems === 0 ? 0 : pageIndex * pageSize + 1;
  const lastItem = Math.min((pageIndex + 1) * pageSize, totalItems);

  return (
    <SpaceBetween direction="horizontal" size="m" alignItems="center">
      <Box color="text-body-secondary">
        {formatNumber(firstItem)}–{formatNumber(lastItem)} / {formatNumber(totalItems)}
      </Box>
      <Box color="text-body-secondary">
        {pageIndex + 1} / {Math.max(1, pageCount)}
      </Box>
      <Pagination
        currentPageIndex={pageIndex + 1}
        pagesCount={Math.max(1, pageCount)}
        onChange={({ detail }) => onPageChange(detail.currentPageIndex - 1)}
        ariaLabels={{
          nextPageLabel: "次のページ",
          previousPageLabel: "前のページ",
          pageLabel: (page) => `${page}ページ`,
        }}
      />
    </SpaceBetween>
  );
}
