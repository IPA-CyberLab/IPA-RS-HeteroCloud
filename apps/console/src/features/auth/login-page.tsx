import { Alert } from "@/components/ui";
import { Button } from "@/components/ui";
import { Container } from "@/components/ui";
import { Header } from "@/components/ui";
import { SpaceBetween } from "@/components/ui";
import { TopNavigation } from "@/components/ui";
import { Navigate } from "react-router-dom";
import { readAuthReturnPath } from "@/features/auth/auth-return";
import { useSession } from "@/features/auth/session";
import { PublicSiteLinks } from "@/features/auth/public-site-links";
import { PUBLIC_HOME_URL } from "@/lib/public-site";

export function LoginPage() {
  const session = useSession();

  if (session.data) return <Navigate to={readAuthReturnPath() ?? "/overview"} replace />;

  return (
    <div className="auth-shell">
      <TopNavigation
        identity={{ href: PUBLIC_HOME_URL, title: "HeteroCloud" }}
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
            <PublicSiteLinks />
          </SpaceBetween>
        </div>
      </main>
    </div>
  );
}
