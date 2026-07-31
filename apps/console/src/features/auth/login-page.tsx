import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Activity, Eye, EyeOff, LoaderCircle, LockKeyhole } from "lucide-react";
import { type FormEvent, useState } from "react";
import { Navigate, useLocation, useNavigate } from "react-router-dom";
import { FormError } from "@/components/shared/form-error";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useSession, sessionQueryOptions } from "@/features/auth/session";
import { api, getApiErrorMessage } from "@/lib/api-client";

interface LoginLocationState {
  from?: {
    pathname?: string;
  };
}

export function LoginPage() {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const session = useSession();
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const location = useLocation();

  const login = useMutation({
    mutationFn: api.auth.login,
    onSuccess: (nextSession) => {
      queryClient.setQueryData(sessionQueryOptions.queryKey, nextSession);
      const state = location.state as LoginLocationState | null;
      navigate(state?.from?.pathname ?? "/overview", { replace: true });
    },
  });

  if (session.data) return <Navigate to="/overview" replace />;

  const handleSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    login.mutate({ email: email.trim(), password });
  };

  return (
    <main className="grid min-h-screen grid-cols-1 bg-white lg:grid-cols-[minmax(20rem,0.85fr)_minmax(32rem,1.15fr)]">
      <section className="hidden bg-[#151719] p-10 text-white lg:flex lg:flex-col lg:justify-between">
        <div className="flex items-center gap-3">
          <span className="flex size-9 items-center justify-center rounded-[6px] bg-emerald-500 text-zinc-950">
            <Activity className="size-5" />
          </span>
          <div>
            <p className="font-semibold">HeteroCloud</p>
            <p className="text-xs text-zinc-400">Cloud control plane</p>
          </div>
        </div>
        <div className="max-w-md">
          <p className="text-2xl font-semibold leading-9">
            クラウドリソースとアクセス権限を一か所で管理
          </p>
          <p className="mt-3 text-sm leading-7 text-zinc-400">
            組織、プロジェクト、IAM、Flowインスタンスの運用状況を確認できます。
          </p>
        </div>
        <p className="text-xs text-zinc-500">HeteroCloud Console</p>
      </section>

      <section className="flex min-h-screen items-center justify-center bg-zinc-50 px-5 py-10">
        <div className="w-full max-w-md">
          <div className="mb-8 flex items-center gap-3 lg:hidden">
            <span className="flex size-9 items-center justify-center rounded-[6px] bg-emerald-600 text-white">
              <Activity className="size-5" />
            </span>
            <div>
              <p className="font-semibold text-zinc-950">HeteroCloud</p>
              <p className="text-xs text-zinc-500">管理コンソール</p>
            </div>
          </div>

          <div className="rounded-[8px] border border-zinc-200 bg-white p-6 shadow-sm sm:p-8">
            <div className="mb-6">
              <span className="mb-4 flex size-10 items-center justify-center rounded-full bg-emerald-50 text-emerald-700">
                <LockKeyhole className="size-5" />
              </span>
              <h1 className="text-xl font-semibold text-zinc-950">ログイン</h1>
              <p className="mt-1 text-sm leading-6 text-zinc-600">
                HeteroCloudアカウントで続行してください。
              </p>
            </div>

            {session.isError ? (
              <div className="mb-5 border border-amber-200 bg-amber-50 px-3 py-2.5 text-sm text-amber-900">
                セッション確認APIに接続できません。ログイン時に再試行します。
              </div>
            ) : null}

            <form onSubmit={handleSubmit} className="space-y-5">
              <div className="space-y-2">
                <Label htmlFor="email">メールアドレス</Label>
                <Input
                  id="email"
                  type="email"
                  autoComplete="username"
                  required
                  value={email}
                  onChange={(event) => setEmail(event.target.value)}
                  placeholder="name@example.com"
                />
              </div>

              <div className="space-y-2">
                <Label htmlFor="password">パスワード</Label>
                <div className="relative">
                  <Input
                    id="password"
                    type={showPassword ? "text" : "password"}
                    autoComplete="current-password"
                    required
                    minLength={12}
                    value={password}
                    onChange={(event) => setPassword(event.target.value)}
                    className="pr-10"
                  />
                  <button
                    type="button"
                    className="absolute right-1 top-1/2 flex size-8 -translate-y-1/2 items-center justify-center rounded-[4px] text-zinc-500 hover:bg-zinc-100 hover:text-zinc-800 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-600"
                    onClick={() => setShowPassword((value) => !value)}
                    aria-label={showPassword ? "パスワードを隠す" : "パスワードを表示"}
                    title={showPassword ? "パスワードを隠す" : "パスワードを表示"}
                  >
                    {showPassword ? (
                      <EyeOff className="size-4" />
                    ) : (
                      <Eye className="size-4" />
                    )}
                  </button>
                </div>
              </div>

              <FormError
                message={login.isError ? getApiErrorMessage(login.error) : null}
              />

              <Button className="w-full" size="lg" disabled={login.isPending}>
                {login.isPending ? (
                  <>
                    <LoaderCircle className="animate-spin" />
                    ログイン中
                  </>
                ) : (
                  "ログイン"
                )}
              </Button>
            </form>
          </div>
        </div>
      </section>
    </main>
  );
}
