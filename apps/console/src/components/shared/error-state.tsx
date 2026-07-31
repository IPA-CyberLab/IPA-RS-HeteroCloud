import { AlertTriangle, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";

interface ErrorStateProps {
  title?: string;
  description: string;
  onRetry?: () => void;
}

export function ErrorState({
  title = "データを取得できませんでした",
  description,
  onRetry,
}: ErrorStateProps) {
  return (
    <div
      className="mx-auto flex w-full max-w-xl flex-col items-center border border-red-200 bg-red-50 px-6 py-10 text-center"
      role="alert"
    >
      <span className="mb-4 flex size-10 items-center justify-center rounded-full bg-red-100 text-red-700">
        <AlertTriangle className="size-5" />
      </span>
      <h2 className="text-base font-semibold text-zinc-950">{title}</h2>
      <p className="mt-2 text-sm leading-6 text-zinc-600">{description}</p>
      {onRetry ? (
        <Button className="mt-5" variant="secondary" onClick={onRetry}>
          <RefreshCw />
          再試行
        </Button>
      ) : null}
    </div>
  );
}
