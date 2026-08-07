import Alert from "@cloudscape-design/components/alert";
import Button from "@cloudscape-design/components/button";
import Container from "@cloudscape-design/components/container";
import Header from "@cloudscape-design/components/header";
import SpaceBetween from "@cloudscape-design/components/space-between";
import TopNavigation from "@cloudscape-design/components/top-navigation";
import { Navigate } from "react-router-dom";
import { useSession } from "@/features/auth/session";

export function LoginPage() {
  const session = useSession();

  if (session.data) return <Navigate to="/overview" replace />;

  return (
    <div className="auth-shell">
      <TopNavigation
        identity={{ href: "/login", title: "HeteroCloud" }}
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
          </SpaceBetween>
        </div>
      </main>
    </div>
  );
}
