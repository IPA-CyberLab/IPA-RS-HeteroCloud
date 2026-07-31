import { Navigate, Outlet, useLocation } from "react-router-dom";
import { ErrorState } from "@/components/shared/error-state";
import { PageLoading } from "@/components/shared/page-loading";
import { useSession } from "@/features/auth/session";

export function ProtectedRoute() {
  const session = useSession();
  const location = useLocation();

  if (session.isPending) {
    return <PageLoading label="セッションを確認しています" fullScreen />;
  }

  if (session.isError) {
    return (
      <div className="flex min-h-screen items-center justify-center bg-zinc-50 p-6">
        <ErrorState
          title="認証サービスに接続できません"
          description="セッションを確認できませんでした。APIの稼働状態を確認して再試行してください。"
          onRetry={() => void session.refetch()}
        />
      </div>
    );
  }

  if (!session.data) {
    return <Navigate to="/login" replace state={{ from: location }} />;
  }

  return <Outlet />;
}
