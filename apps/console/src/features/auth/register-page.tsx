import Alert from "@cloudscape-design/components/alert";
import Box from "@cloudscape-design/components/box";
import Button from "@cloudscape-design/components/button";
import ColumnLayout from "@cloudscape-design/components/column-layout";
import Container from "@cloudscape-design/components/container";
import Form from "@cloudscape-design/components/form";
import FormField from "@cloudscape-design/components/form-field";
import Header from "@cloudscape-design/components/header";
import Input from "@cloudscape-design/components/input";
import SpaceBetween from "@cloudscape-design/components/space-between";
import TopNavigation from "@cloudscape-design/components/top-navigation";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { type FormEvent, useState } from "react";
import { Navigate, useLocation, useNavigate } from "react-router-dom";
import { sessionQueryOptions, useSession } from "@/features/auth/session";
import { api, getApiErrorMessage } from "@/lib/api-client";

function AuthTopNavigation() {
  return (
    <TopNavigation
      identity={{ href: "/login", title: "HeteroCloud", logo: { src: "/favicon.svg", alt: "" } }}
      utilities={[]}
      i18nStrings={{ overflowMenuTriggerText: "その他", overflowMenuTitleText: "メニュー" }}
    />
  );
}

export function RegisterPage() {
  const location = useLocation();
  const [invitationCode] = useState(
    () =>
      new URLSearchParams(location.hash.replace(/^#/, "")).get("invitation_code")?.trim() ?? "",
  );
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
      invitation_code: invitationCode,
      email: email.trim(),
      display_name: displayName.trim(),
      password,
    });
  };
  const valid =
    displayName.trim().length > 0 &&
    email.includes("@") &&
    password.length >= 12 &&
    passwordConfirmation.length >= 12;

  return (
    <div className="auth-shell">
      <AuthTopNavigation />
      <main className="auth-page">
        <div className="auth-panel auth-panel--wide">
          <SpaceBetween size="l">
            <Header variant="h1" description="招待された組織へ参加します。">
              アカウントを登録
            </Header>
            <Container>
              <form onSubmit={submit}>
                <Form
                  errorText={
                    validationError ??
                    (register.isError ? getApiErrorMessage(register.error) : undefined)
                  }
                  actions={
                    <SpaceBetween direction="horizontal" size="xs">
                      <Button href="/login">ログインへ戻る</Button>
                      <Button
                        variant="primary"
                        formAction="submit"
                        loading={register.isPending}
                        disabled={!valid}
                      >
                        登録して参加
                      </Button>
                    </SpaceBetween>
                  }
                >
                  <SpaceBetween size="l">
                    <Alert type="info">
                      招待コードはURLフラグメントから安全に読み取り、サーバーのアクセスログへ送信しません。
                    </Alert>
                    <ColumnLayout columns={2}>
                      <FormField label="表示名">
                        <Input
                          value={displayName}
                          placeholder="山田 太郎"
                          autoComplete="name"
                          onChange={({ detail }) => setDisplayName(detail.value.slice(0, 120))}
                        />
                      </FormField>
                      <FormField label="メールアドレス">
                        <Input
                          type="email"
                          value={email}
                          placeholder="name@example.com"
                          autoComplete="username"
                          onChange={({ detail }) => setEmail(detail.value)}
                        />
                      </FormField>
                    </ColumnLayout>
                    <ColumnLayout columns={2}>
                      <FormField
                        label="パスワード"
                        description="12〜128文字"
                        secondaryControl={
                          <Button
                            variant="inline-icon"
                            iconName="view-full"
                            ariaLabel={showPassword ? "パスワードを隠す" : "パスワードを表示"}
                            onClick={() => setShowPassword((value) => !value)}
                          />
                        }
                      >
                        <Input
                          type={showPassword ? "text" : "password"}
                          value={password}
                          autoComplete="new-password"
                          onChange={({ detail }) => setPassword(detail.value.slice(0, 128))}
                        />
                      </FormField>
                      <FormField label="パスワード（確認）">
                        <Input
                          type={showPassword ? "text" : "password"}
                          value={passwordConfirmation}
                          autoComplete="new-password"
                          onChange={({ detail }) => setPasswordConfirmation(detail.value.slice(0, 128))}
                        />
                      </FormField>
                    </ColumnLayout>
                  </SpaceBetween>
                </Form>
              </form>
            </Container>
          </SpaceBetween>
        </div>
      </main>
    </div>
  );
}

function MissingInvitationPage() {
  return (
    <div className="auth-shell">
      <AuthTopNavigation />
      <main className="auth-page">
        <div className="auth-panel">
          <Alert
            type="warning"
            header="招待リンクが必要です"
            action={<Button href="/login">ログインへ戻る</Button>}
          >
            アカウント登録は組織オーナーが発行した有効な招待リンクからのみ開始できます。
          </Alert>
          <Box color="text-body-secondary" textAlign="center" padding={{ top: "l" }}>
            招待URLを確認して、もう一度開いてください。
          </Box>
        </div>
      </main>
    </div>
  );
}
