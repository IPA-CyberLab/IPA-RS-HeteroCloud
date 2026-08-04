import Alert from "@cloudscape-design/components/alert";
import Box from "@cloudscape-design/components/box";
import Button from "@cloudscape-design/components/button";
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
import { FormError } from "@/components/shared/form-error";
import { sessionQueryOptions, useSession } from "@/features/auth/session";
import { api, getApiErrorMessage } from "@/lib/api-client";

interface LoginLocationState {
  from?: { pathname?: string };
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

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    login.mutate({ email: email.trim(), password });
  };

  return (
    <div className="auth-shell">
      <TopNavigation
        identity={{ href: "/login", title: "HeteroCloud", logo: { src: "/favicon.svg", alt: "" } }}
        utilities={[]}
        i18nStrings={{ overflowMenuTriggerText: "その他", overflowMenuTitleText: "メニュー" }}
      />
      <main className="auth-page">
        <div className="auth-panel">
          <SpaceBetween size="l">
            <Header variant="h1" description="クラウドリソース管理コンソール">
              ログイン
            </Header>
            {session.isError ? (
              <Alert type="warning">
                セッション確認APIに接続できません。ログイン時に再試行します。
              </Alert>
            ) : null}
            <Container
              header={
                <Header variant="h2" description="組織のIDプロバイダーで認証します。">
                  Keycloak
                </Header>
              }
            >
              <SpaceBetween size="s">
                <Button variant="primary" fullWidth href="/api/v1/auth/oidc/start" iconName="key">
                  Keycloakでログイン
                </Button>
                <Button fullWidth href="/api/v1/auth/oidc/start?intent=register" iconName="user-profile-active">
                  アカウントを作成
                </Button>
              </SpaceBetween>
            </Container>
            <Container
              header={
                <Header variant="h2" description="管理者が発行したローカル資格情報を使用します。">
                  ローカルアカウント
                </Header>
              }
            >
              <form onSubmit={submit}>
                <Form
                  errorText={login.isError ? getApiErrorMessage(login.error) : undefined}
                  actions={
                    <Button
                      variant="primary"
                      formAction="submit"
                      loading={login.isPending}
                      disabled={!email.trim() || password.length < 12}
                    >
                      ログイン
                    </Button>
                  }
                >
                  <SpaceBetween size="l">
                    <FormField label="メールアドレス">
                      <Input
                        type="email"
                        value={email}
                        placeholder="name@example.com"
                        autoComplete="username"
                        onChange={({ detail }) => setEmail(detail.value)}
                      />
                    </FormField>
                    <FormField
                      label="パスワード"
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
                        autoComplete="current-password"
                        onChange={({ detail }) => setPassword(detail.value)}
                      />
                    </FormField>
                    <FormError message={null} />
                  </SpaceBetween>
                </Form>
              </form>
            </Container>
            <Box textAlign="center" color="text-body-secondary">
              セッションはHttpOnly CookieとCSRF検証で保護されています。
            </Box>
          </SpaceBetween>
        </div>
      </main>
    </div>
  );
}
