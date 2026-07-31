import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  Activity,
  Eye,
  EyeOff,
  LoaderCircle,
  UserPlus,
} from "lucide-react";
import { type FormEvent, useState } from "react";
import {
  Link,
  Navigate,
  useLocation,
  useNavigate,
} from "react-router-dom";
import { FormError } from "@/components/shared/form-error";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { sessionQueryOptions, useSession } from "@/features/auth/session";
import { api, getApiErrorMessage } from "@/lib/api-client";

export function RegisterPage() {
  const location = useLocation();
  const [invitationCode] = useState(() => {
    return new URLSearchParams(
      location.hash.replace(/^#/, ""),
    ).get("invitation_code")?.trim() ?? "";
  });
  const [email, setEmail] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [password, setPassword] = useState("");
  const [passwordConfirmation, setPasswordConfirmation] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [validationError, setValidationError] = useState<string | null>(null);
  const session = useSession();
  const queryClient = useQueryClient();
  const navigate = useNavigate();

  const register = useMutation({
    mutationFn: api.auth.register,
    onSuccess: (nextSession) => {
      queryClient.setQueryData(sessionQueryOptions.queryKey, nextSession);
      navigate("/overview", { replace: true });
    },
  });

  if (session.data) return <Navigate to="/overview" replace />;
  if (!invitationCode) return <MissingInvitationPage />;

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setValidationError(null);
    if (password !== passwordConfirmation) {
      setValidationError("確認用パスワードが一致しません。");
      return;
    }
    register.mutate({
      invitation_code: invitationCode.trim(),
      email: email.trim(),
      display_name: displayName.trim(),
      password,
    });
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
            招待された組織へ参加
          </p>
          <p className="mt-3 text-sm leading-7 text-zinc-400">
            発行された招待コードは利用回数と有効期限で保護されています。
          </p>
        </div>
        <p className="text-xs text-zinc-500">HeteroCloud Console</p>
      </section>

      <section className="flex min-h-screen items-center justify-center bg-zinc-50 px-5 py-10">
        <div className="w-full max-w-lg">
          <div className="mb-6 flex items-center gap-3 lg:hidden">
            <span className="flex size-9 items-center justify-center rounded-[6px] bg-emerald-600 text-white">
              <Activity className="size-5" />
            </span>
            <div>
              <p className="font-semibold text-zinc-950">HeteroCloud</p>
              <p className="text-xs text-zinc-500">アカウント登録</p>
            </div>
          </div>

          <div className="rounded-[8px] border border-zinc-200 bg-white p-6 shadow-sm sm:p-8">
            <div className="mb-6">
              <span className="mb-4 flex size-10 items-center justify-center rounded-full bg-emerald-50 text-emerald-700">
                <UserPlus className="size-5" />
              </span>
              <h1 className="text-xl font-semibold text-zinc-950">
                アカウントを登録
              </h1>
              <p className="mt-1 text-sm leading-6 text-zinc-600">
                組織オーナーから受け取った招待コードを入力してください。
              </p>
            </div>

            <form onSubmit={submit} className="space-y-4">
              <div className="grid gap-4 sm:grid-cols-2">
                <div className="space-y-2">
                  <Label htmlFor="register-name">表示名</Label>
                  <Input
                    id="register-name"
                    required
                    maxLength={120}
                    autoComplete="name"
                    value={displayName}
                    onChange={(event) => setDisplayName(event.target.value)}
                    placeholder="山田 太郎"
                  />
                </div>
                <div className="space-y-2">
                  <Label htmlFor="register-email">メールアドレス</Label>
                  <Input
                    id="register-email"
                    type="email"
                    required
                    autoComplete="username"
                    value={email}
                    onChange={(event) => setEmail(event.target.value)}
                    placeholder="name@example.com"
                  />
                </div>
              </div>

              <div className="grid gap-4 sm:grid-cols-2">
                <div className="space-y-2">
                  <Label htmlFor="register-password">パスワード</Label>
                  <div className="relative">
                    <Input
                      id="register-password"
                      type={showPassword ? "text" : "password"}
                      required
                      minLength={12}
                      maxLength={128}
                      autoComplete="new-password"
                      value={password}
                      onChange={(event) => setPassword(event.target.value)}
                      className="pr-10"
                    />
                    <button
                      type="button"
                      className="absolute right-1 top-1/2 flex size-8 -translate-y-1/2 items-center justify-center rounded-[4px] text-zinc-500 hover:bg-zinc-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-600"
                      onClick={() => setShowPassword((value) => !value)}
                      aria-label={
                        showPassword ? "パスワードを隠す" : "パスワードを表示"
                      }
                    >
                      {showPassword ? (
                        <EyeOff className="size-4" />
                      ) : (
                        <Eye className="size-4" />
                      )}
                    </button>
                  </div>
                  <p className="text-xs text-zinc-500">12〜128文字</p>
                </div>
                <div className="space-y-2">
                  <Label htmlFor="register-password-confirmation">
                    パスワード（確認）
                  </Label>
                  <Input
                    id="register-password-confirmation"
                    type={showPassword ? "text" : "password"}
                    required
                    minLength={12}
                    maxLength={128}
                    autoComplete="new-password"
                    value={passwordConfirmation}
                    onChange={(event) =>
                      setPasswordConfirmation(event.target.value)
                    }
                  />
                </div>
              </div>

              <FormError
                message={
                  validationError ??
                  (register.isError ? getApiErrorMessage(register.error) : null)
                }
              />

              <Button className="w-full" size="lg" disabled={register.isPending}>
                {register.isPending ? (
                  <>
                    <LoaderCircle className="animate-spin" />
                    登録中
                  </>
                ) : (
                  "登録して参加"
                )}
              </Button>
              <p className="text-center text-sm text-zinc-600">
                アカウントをお持ちですか？{" "}
                <Link
                  to="/login"
                  className="font-medium text-emerald-700 underline-offset-4 hover:underline"
                >
                  ログイン
                </Link>
              </p>
            </form>
          </div>
        </div>
      </section>
    </main>
  );
}

function MissingInvitationPage() {
  return (
    <main className="flex min-h-screen items-center justify-center bg-zinc-50 p-6">
      <div className="w-full max-w-md rounded-[8px] border border-zinc-200 bg-white p-7 text-center shadow-sm">
        <span className="mx-auto mb-4 flex size-10 items-center justify-center rounded-full bg-amber-100 text-amber-800">
          <UserPlus className="size-5" />
        </span>
        <h1 className="text-lg font-semibold text-zinc-950">
          招待リンクが必要です
        </h1>
        <p className="mt-2 text-sm leading-6 text-zinc-600">
          アカウント登録は組織オーナーが発行した有効な招待リンクからのみ開始できます。
        </p>
        <Button asChild variant="secondary" className="mt-5">
          <Link to="/login">ログインへ戻る</Link>
        </Button>
      </div>
    </main>
  );
}
