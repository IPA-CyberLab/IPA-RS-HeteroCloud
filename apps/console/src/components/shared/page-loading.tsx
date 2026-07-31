import { LoaderCircle } from "lucide-react";
import { cn } from "@/lib/utils";

interface PageLoadingProps {
  label?: string;
  fullScreen?: boolean;
}

export function PageLoading({
  label = "読み込んでいます",
  fullScreen = false,
}: PageLoadingProps) {
  return (
    <div
      className={cn(
        "flex min-h-64 items-center justify-center text-zinc-600",
        fullScreen && "min-h-screen bg-zinc-50",
      )}
      role="status"
    >
      <div className="flex items-center gap-2 text-sm">
        <LoaderCircle className="size-4 animate-spin" />
        <span>{label}</span>
      </div>
    </div>
  );
}
