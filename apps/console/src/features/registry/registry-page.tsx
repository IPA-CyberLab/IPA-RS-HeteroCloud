import Alert from "@cloudscape-design/components/alert";
import Box from "@cloudscape-design/components/box";
import Button from "@cloudscape-design/components/button";
import ColumnLayout from "@cloudscape-design/components/column-layout";
import Container from "@cloudscape-design/components/container";
import FormField from "@cloudscape-design/components/form-field";
import Header from "@cloudscape-design/components/header";
import Input from "@cloudscape-design/components/input";
import Modal from "@cloudscape-design/components/modal";
import ProgressBar from "@cloudscape-design/components/progress-bar";
import SpaceBetween from "@cloudscape-design/components/space-between";
import StatusIndicator from "@cloudscape-design/components/status-indicator";
import Table from "@cloudscape-design/components/table";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { EmptyState } from "@/components/shared/empty-state";
import { ErrorState } from "@/components/shared/error-state";
import { FormError } from "@/components/shared/form-error";
import { PageHeader } from "@/components/shared/page-header";
import { PageLoading } from "@/components/shared/page-loading";
import { useActiveOrganization } from "@/features/organizations/organization-context";
import { api, getApiErrorMessage } from "@/lib/api-client";
import type {
  RegistryCredential,
  RegistryCredentialSecret,
} from "@/lib/api-types";
import { formatDateTime } from "@/lib/utils";

function formatBytes(value: number): string {
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let amount = Math.max(0, value);
  let unit = 0;
  while (amount >= 1024 && unit < units.length - 1) {
    amount /= 1024;
    unit += 1;
  }
  return `${amount.toLocaleString("ja-JP", { maximumFractionDigits: 2 })} ${units[unit]}`;
}

function shellQuote(value: string): string {
  return `'${value.replaceAll("'", `'\\''`)}'`;
}

function credentialCommands(secret: RegistryCredentialSecret): string {
  return [
    `printf '%s' ${shellQuote(secret.password)} | ${secret.login_command}`,
    `docker tag my-image:latest ${secret.image_prefix}/my-image:latest`,
    `docker push ${secret.image_prefix}/my-image:latest`,
  ].join("\n");
}

export function RegistryPage() {
  const { activeOrganization } = useActiveOrganization();
  const organizationId = activeOrganization.organization_id;
  const queryClient = useQueryClient();
  const queryKey = useMemo(
    () => ["registry", organizationId] as const,
    [organizationId],
  );
  const registry = useQuery({
    queryKey,
    queryFn: ({ signal }) => api.registry.get(organizationId, signal),
  });
  const [createOpen, setCreateOpen] = useState(false);
  const [name, setName] = useState("");
  const [secret, setSecret] = useState<RegistryCredentialSecret | null>(null);
  const [revokeTarget, setRevokeTarget] = useState<RegistryCredential | null>(null);
  const [copied, setCopied] = useState(false);

  const refresh = () => queryClient.invalidateQueries({ queryKey });
  const create = useMutation({
    mutationFn: () => api.registry.createCredential(organizationId, name.trim()),
    onSuccess: async (value) => {
      setCreateOpen(false);
      setSecret(value);
      await refresh();
    },
  });
  const revoke = useMutation({
    mutationFn: (credentialId: string) =>
      api.registry.revokeCredential(organizationId, credentialId),
    onSuccess: async () => {
      setRevokeTarget(null);
      await refresh();
    },
  });

  if (registry.isPending) {
    return <PageLoading label="コンテナレジストリを読み込んでいます" />;
  }
  if (registry.isError) {
    return (
      <ErrorState
        title="コンテナレジストリに接続できません"
        description={getApiErrorMessage(registry.error)}
        onRetry={() => void registry.refetch()}
      />
    );
  }

  const value = registry.data;
  const usagePercent = value.storage_limit_bytes
    ? Math.min(100, (value.storage_used_bytes / value.storage_limit_bytes) * 100)
    : 0;
  const activeCredentials = value.credentials.filter(
    (credential) => credential.status === "active",
  ).length;
  const commands = secret ? credentialCommands(secret) : "";

  return (
    <SpaceBetween size="l">
      <PageHeader
        title="コンテナレジストリ"
        description="組織専用のプライベートOCIレジストリ"
        actions={
          <Button
            variant="icon"
            iconName="refresh"
            ariaLabel="レジストリ情報を更新"
            onClick={() => void registry.refetch()}
          />
        }
      />

      <Container header={<Header variant="h2">レジストリ</Header>}>
        <ColumnLayout columns={3} variant="text-grid">
          <div>
            <Box variant="awsui-key-label">エンドポイント</Box>
            <Box variant="code">{value.endpoint}</Box>
          </div>
          <div>
            <Box variant="awsui-key-label">イメージPrefix</Box>
            <Box variant="code">{value.image_prefix}</Box>
          </div>
          <div>
            <Box variant="awsui-key-label">認証情報</Box>
            <Box>
              {activeCredentials} / {value.max_credentials}
            </Box>
          </div>
        </ColumnLayout>
        <Box margin={{ top: "l" }}>
          <ProgressBar
            value={usagePercent}
            label="保存容量"
            description={`${formatBytes(value.storage_used_bytes)} / ${formatBytes(value.storage_limit_bytes)}`}
          />
        </Box>
      </Container>

      <Table
        variant="container"
        stickyHeader
        wrapLines
        trackBy="id"
        items={value.credentials}
        header={
          <Header
            variant="h2"
            counter={`(${value.credentials.length})`}
            description="Push/Pullに使用する認証情報。秘密値は発行時に一度だけ表示されます。"
            actions={
              <Button
                variant="primary"
                iconName="add-plus"
                disabled={activeCredentials >= value.max_credentials}
                onClick={() => {
                  setName("");
                  create.reset();
                  setCreateOpen(true);
                }}
              >
                認証情報を発行
              </Button>
            }
          >
            レジストリ認証情報
          </Header>
        }
        columnDefinitions={[
          {
            id: "name",
            header: "名前",
            cell: (item) => <Box fontWeight="bold">{item.name}</Box>,
          },
          {
            id: "username",
            header: "ユーザー名",
            cell: (item) => item.username ? <Box variant="code">{item.username}</Box> : "-",
          },
          {
            id: "created",
            header: "発行日時",
            cell: (item) => formatDateTime(item.created_at),
          },
          {
            id: "status",
            header: "状態",
            cell: (item) =>
              item.status === "active" ? (
                <StatusIndicator type="success">有効</StatusIndicator>
              ) : (
                <StatusIndicator type="stopped">失効済み</StatusIndicator>
              ),
          },
          {
            id: "actions",
            header: "操作",
            cell: (item) => (
              <Button
                variant="inline-icon"
                iconName="remove"
                ariaLabel={`${item.name}を失効`}
                disabled={item.status !== "active"}
                onClick={() => {
                  revoke.reset();
                  setRevokeTarget(item);
                }}
              />
            ),
          },
        ]}
        empty={
          <EmptyState
            title="認証情報がありません"
            description="イメージをPushするための認証情報を発行してください。"
          />
        }
      />

      <Modal
        visible={createOpen}
        header="レジストリ認証情報を発行"
        onDismiss={() => setCreateOpen(false)}
        footer={
          <Box float="right">
            <SpaceBetween direction="horizontal" size="xs">
              <Button onClick={() => setCreateOpen(false)}>キャンセル</Button>
              <Button
                variant="primary"
                iconName="key"
                loading={create.isPending}
                disabled={!name.trim()}
                onClick={() => create.mutate()}
              >
                発行
              </Button>
            </SpaceBetween>
          </Box>
        }
      >
        <SpaceBetween size="l">
          <FormField label="名前" description="用途が分かる名前を設定します。">
            <Input
              value={name}
              autoComplete="off"
              placeholder="development-machine"
              onChange={({ detail }) => setName(detail.value.slice(0, 120))}
            />
          </FormField>
          <FormError message={create.isError ? getApiErrorMessage(create.error) : null} />
        </SpaceBetween>
      </Modal>

      <Modal
        visible={secret !== null}
        size="large"
        header="認証情報を保存"
        onDismiss={() => {
          setSecret(null);
          setCopied(false);
        }}
        footer={
          <Box float="right">
            <Button
              variant="primary"
              onClick={() => {
                setSecret(null);
                setCopied(false);
              }}
            >
              閉じる
            </Button>
          </Box>
        }
      >
        {secret ? (
          <SpaceBetween size="l">
            <Alert type="warning">
              パスワードは今回だけ表示されます。閉じると再表示できません。
            </Alert>
            <ColumnLayout columns={2} variant="text-grid">
              <div>
                <Box variant="awsui-key-label">ユーザー名</Box>
                <Box variant="code">{secret.username}</Box>
              </div>
              <div>
                <Box variant="awsui-key-label">パスワード</Box>
                <Box variant="code">{secret.password}</Box>
              </div>
            </ColumnLayout>
            <div>
              <Header
                variant="h3"
                actions={
                  <Button
                    iconName={copied ? "check" : "copy"}
                    onClick={async () => {
                      await navigator.clipboard.writeText(commands);
                      setCopied(true);
                    }}
                  >
                    {copied ? "コピー済み" : "コマンドをコピー"}
                  </Button>
                }
              >
                Docker CLI
              </Header>
              <pre className="code-block"><code>{commands}</code></pre>
            </div>
          </SpaceBetween>
        ) : null}
      </Modal>

      <Modal
        visible={revokeTarget !== null}
        header="レジストリ認証情報を失効"
        onDismiss={() => setRevokeTarget(null)}
        footer={
          <Box float="right">
            <SpaceBetween direction="horizontal" size="xs">
              <Button onClick={() => setRevokeTarget(null)}>キャンセル</Button>
              <Button
                variant="primary"
                iconName="remove"
                loading={revoke.isPending}
                onClick={() => revokeTarget && revoke.mutate(revokeTarget.id)}
              >
                失効
              </Button>
            </SpaceBetween>
          </Box>
        }
      >
        <SpaceBetween size="l">
          <Alert type="warning">
            {revokeTarget?.name ?? ""} を使用するPush/Pullを直ちに拒否します。
          </Alert>
          <FormError message={revoke.isError ? getApiErrorMessage(revoke.error) : null} />
        </SpaceBetween>
      </Modal>
    </SpaceBetween>
  );
}
