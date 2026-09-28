import Alert from "@cloudscape-design/components/alert";
import Box from "@cloudscape-design/components/box";
import Button from "@cloudscape-design/components/button";
import Container from "@cloudscape-design/components/container";
import Header from "@cloudscape-design/components/header";
import Select from "@cloudscape-design/components/select";
import SpaceBetween from "@cloudscape-design/components/space-between";
import Spinner from "@cloudscape-design/components/spinner";
import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { FlashSecretsPanel } from "@/features/flash/flash-secrets-panel";
import { useActiveOrganization } from "@/features/organizations/organization-context";
import { api } from "@/lib/api-client";
import { flashServicesQueryOptions } from "@/lib/queries";

export function SecretManagerPage() {
  const { activeOrganization } = useActiveOrganization();
  const organizationId = activeOrganization.organization_id;
  const services = useQuery(flashServicesQueryOptions(organizationId));
  const link = useQuery({
    queryKey: ["secret-manager", "link"],
    queryFn: ({ signal }) => api.secretManager.link(signal),
  });
  const navigate = useNavigate();
  const [selectedServiceId, setSelectedServiceId] = useState<string | null>(null);
  const selectedService = services.data?.items.find(
    (service) => service.id === selectedServiceId,
  ) ?? services.data?.items[0];

  return (
    <SpaceBetween size="l">
      <Header variant="h1" description="登録した値をFlashコンテナのファイルとして安全に使います。">
        Hetero Secret Manager
      </Header>
      <Container header={<Header variant="h2">コンテナに使うシークレット</Header>}>
        <SpaceBetween size="m">
          <Box>
            Flashサービスを選び、シークレットを登録して「コンテナへ接続」を押してください。
            値は一覧やサービス設定に保存せず、OpenBaoに保管します。
          </Box>
          {services.isPending ? <Spinner /> : services.isError ? (
            <Alert type="error" action={<Button onClick={() => void services.refetch()}>再試行</Button>}>
              Flashサービスを取得できませんでした。
            </Alert>
          ) : services.data.items.length === 0 ? (
            <SpaceBetween size="s">
              <Box>Flashサービスがありません。先にコンテナサービスを作成してください。</Box>
              <Button onClick={() => navigate("/flash/services")}>Flashサービスを開く</Button>
            </SpaceBetween>
          ) : (
            <SpaceBetween direction="horizontal" size="s" alignItems="end">
              <Select
                ariaLabel="Flashサービス"
                selectedOption={selectedService ? { value: selectedService.id, label: selectedService.name } : null}
                options={services.data.items.map((service) => ({ value: service.id, label: service.name }))}
                onChange={({ detail }) => setSelectedServiceId(detail.selectedOption.value ?? null)}
              />
              <Button onClick={() => navigate(`/flash/services/${selectedService?.id}`)}>
                サービス詳細を開く
              </Button>
            </SpaceBetween>
          )}
        </SpaceBetween>
      </Container>
      {selectedService ? (
        <FlashSecretsPanel
          key={`${organizationId}:${selectedService.id}`}
          organizationId={organizationId}
          service={selectedService}
          disabled={selectedService.state === "deleting"}
        />
      ) : null}
      <Container header={<Header variant="h2">OpenBaoを直接開く</Header>}>
        <SpaceBetween size="s">
          <Box>
            OpenBaoの個人用保管庫は、上のFlashサービス用シークレットとは別の領域です。
            コンテナで使う値は上の画面から登録してください。直接開く場合はOIDCで
            サインインします。Roleは空欄で構いません。
          </Box>
          {link.isPending ? <Spinner /> : link.isError ? (
            <Alert type="error">OpenBaoの接続先を取得できませんでした。</Alert>
          ) : (
            <a href={link.data.url} target="_blank" rel="noopener noreferrer">
              OpenBaoにOIDCでサインイン
            </a>
          )}
        </SpaceBetween>
      </Container>
    </SpaceBetween>
  );
}
