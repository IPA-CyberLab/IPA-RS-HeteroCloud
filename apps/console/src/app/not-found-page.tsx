import { ArrowLeft, FileQuestion } from "lucide-react";
import { Link } from "react-router-dom";
import { Button } from "@/components/ui/button";

export function NotFoundPage() {
  return (
    <main className="flex min-h-screen items-center justify-center bg-zinc-50 p-6">
      <div className="max-w-md text-center">
        <span className="mx-auto mb-4 flex size-11 items-center justify-center rounded-full bg-zinc-200 text-zinc-600">
          <FileQuestion className="size-5" />
        </span>
        <p className="text-sm font-medium text-zinc-500">404</p>
        <h1 className="mt-1 text-xl font-semibold text-zinc-950">
          ページが見つかりません
        </h1>
        <p className="mt-2 text-sm leading-6 text-zinc-600">
          URLを確認するか、コンソールの概要へ戻ってください。
        </p>
        <Button asChild className="mt-5">
          <Link to="/overview">
            <ArrowLeft />
            概要へ戻る
          </Link>
        </Button>
      </div>
    </main>
  );
}
