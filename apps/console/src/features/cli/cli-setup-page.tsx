import Alert from "@cloudscape-design/components/alert";
import Box from "@cloudscape-design/components/box";
import Button from "@cloudscape-design/components/button";
import Container from "@cloudscape-design/components/container";
import Header from "@cloudscape-design/components/header";
import KeyValuePairs from "@cloudscape-design/components/key-value-pairs";
import SpaceBetween from "@cloudscape-design/components/space-between";
import { useMemo, useState } from "react";
import { PageHeader } from "@/components/shared/page-header";
import { useActiveOrganization } from "@/features/organizations/organization-context";
import { buildChatGptLaunchUrl, buildCliSetupPrompt } from "./cli-setup";

export function CliSetupPanel({
  endpoint,
  organizationId,
}: {
  endpoint: string;
  organizationId: string;
}) {
  const [copyState, setCopyState] = useState<"idle" | "copied" | "error">("idle");
  const prompt = useMemo(
    () => buildCliSetupPrompt({ endpoint, organizationId }),
    [endpoint, organizationId],
  );
  const launchUrl = useMemo(() => buildChatGptLaunchUrl(prompt), [prompt]);

  const copyPrompt = async () => {
    try {
      await navigator.clipboard.writeText(prompt);
      setCopyState("copied");
    } catch {
      setCopyState("error");
    }
  };

  return (
    <SpaceBetween size="l">
      <Alert type="info" header="秘密値はChatGPTへ送りません">
        起動リンクには接続先と組織IDだけを含めます。認証はブラウザ上のKeycloakログインと
        CLI承認で完了し、APIキーやアクセストークンの貼り付けは不要です。
      </Alert>
      {copyState === "copied" ? (
        <Alert type="success" dismissible onDismiss={() => setCopyState("idle")}>
          セットアップ用プロンプトをコピーしました。
        </Alert>
      ) : null}
      {copyState === "error" ? (
        <Alert type="error" dismissible onDismiss={() => setCopyState("idle")}>
          コピーできませんでした。ブラウザのクリップボード権限を確認してください。
        </Alert>
      ) : null}
      <Container
        header={
          <Header
            variant="h2"
            description="現在のHeteroCloud環境に合わせたプロンプトをChatGPTアプリで開きます。"
          >
            ChatGPTでCLIをセットアップ
          </Header>
        }
      >
        <SpaceBetween size="l">
          <KeyValuePairs
            columns={1}
            items={[
              {
                label: "接続先",
                value: <Box variant="code">{new URL(endpoint).origin}</Box>,
              },
              {
                label: "組織ID",
                value: <Box variant="code">{organizationId}</Box>,
              },
            ]}
          />
          <Box color="text-body-secondary">
            OSとCPUの判定、最新版の取得、チェックサム検証、PATHへの配置、
            Claude Code/Codexのskill導入、OAuthログイン、接続確認までを案内します。
          </Box>
          <SpaceBetween direction="horizontal" size="xs">
            <Button variant="primary" iconName="external" href={launchUrl}>
              ChatGPTでセットアップ
            </Button>
            <Button
              iconName={copyState === "copied" ? "check" : "copy"}
              onClick={() => void copyPrompt()}
            >
              プロンプトをコピー
            </Button>
          </SpaceBetween>
          <Box color="text-body-secondary" fontSize="body-s">
            ChatGPTアプリが開かない場合は、プロンプトをコピーして新しいチャットへ
            貼り付けてください。
          </Box>
        </SpaceBetween>
      </Container>
    </SpaceBetween>
  );
}

export function CliSetupPage() {
  const { activeOrganization } = useActiveOrganization();

  return (
    <SpaceBetween size="l">
      <PageHeader
        title="CLIセットアップ"
        description="HeteroCloud CLIをこの端末へ安全に導入し、現在の環境へ接続します。"
      />
      <CliSetupPanel
        endpoint={window.location.origin}
        organizationId={activeOrganization.organization_id}
      />
    </SpaceBetween>
  );
}
