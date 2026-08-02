import { ChevronLeft, ChevronRight } from "lucide-react";
import { Button } from "@/components/ui/button";
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
    <div className="flex min-h-14 items-center justify-between gap-4 border-t border-zinc-200 px-4 py-2">
      <span className="text-xs text-zinc-500">
        {formatNumber(firstItem)}–{formatNumber(lastItem)} /{" "}
        {formatNumber(totalItems)}
      </span>
      <div className="flex items-center gap-1">
        <Button
          type="button"
          variant="ghost"
          size="icon"
          title="前のページ"
          aria-label="前のページ"
          onClick={() => onPageChange(pageIndex - 1)}
          disabled={pageIndex === 0}
        >
          <ChevronLeft />
        </Button>
        <span className="min-w-16 text-center text-xs text-zinc-600">
          {pageIndex + 1} / {pageCount}
        </span>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          title="次のページ"
          aria-label="次のページ"
          onClick={() => onPageChange(pageIndex + 1)}
          disabled={pageIndex >= pageCount - 1}
        >
          <ChevronRight />
        </Button>
      </div>
    </div>
  );
}
