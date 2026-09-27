import Alert from "@cloudscape-design/components/alert";
import Box from "@cloudscape-design/components/box";
import Container from "@cloudscape-design/components/container";
import Header from "@cloudscape-design/components/header";
import SpaceBetween from "@cloudscape-design/components/space-between";
import Spinner from "@cloudscape-design/components/spinner";
import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api-client";

export function SecretManagerPage() {
  const link = useQuery({
    queryKey: ["secret-manager", "link"],
    queryFn: ({ signal }) => api.secretManager.link(signal),
  });

  return (
    <SpaceBetween size="l">
      <Header variant="h1" description="OpenBaoで機密情報を管理します。">
        Hetero Secret Manager
      </Header>
      <Container>
        <SpaceBetween size="m">
          <Box>
            Keycloakでサインインして、個人用のシークレットを作成・更新できます。
          </Box>
          {link.isPending ? <Spinner /> : link.isError ? (
            <Alert type="error">Secret Managerの接続先を取得できませんでした。</Alert>
          ) : (
            <a href={link.data.url} target="_blank" rel="noopener noreferrer">
              Secret Managerを開く
            </a>
          )}
        </SpaceBetween>
      </Container>
    </SpaceBetween>
  );
}
