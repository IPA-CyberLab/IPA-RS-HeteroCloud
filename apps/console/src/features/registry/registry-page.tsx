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
import { registryImagesQueryOptions } from "@/lib/queries";
import type {
  RegistryCredential,
  RegistryCredentialSecret,
  RegistryImage,
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
  const images = useQuery(registryImagesQueryOptions(organizationId));
  const [createOpen, setCreateOpen] = useState(false);
  const [name, setName] = useState("");
  const [secret, setSecret] = useState<RegistryCredentialSecret | null>(null);
  const [deleteCredentialTarget, setDeleteCredentialTarget] = useState<RegistryCredential | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<RegistryImage | null>(null);
  const [copied, setCopied] = useState(false);

  const refreshRegistry = () => queryClient.invalidateQueries({ queryKey });
  const refreshImages = () =>
    queryClient.invalidateQueries({
      queryKey: ["registry", organizationId, "images"],
    });
  const create = useMutation({
    mutationFn: () => api.registry.createCredential(organizationId, name.trim()),
    onSuccess: async (value) => {
      setCreateOpen(false);
      setSecret(value);
      await refreshRegistry();
    },
  });
  const deleteCredential = useMutation({
    mutationFn: (credentialId: string) =>
      api.registry.deleteCredential(organizationId, credentialId),
    onSuccess: async () => {
      setDeleteCredentialTarget(null);
      await refreshRegistry();
    },
  });
  const deleteImage = useMutation({
    mutationFn: (image: RegistryImage) =>
      api.registry.deleteImage(
        organizationId,
        image.repository,
        image.digest,
      ),
    onSuccess: async () => {
      setDeleteTarget(null);
      await Promise.all([refreshRegistry(), refreshImages()]);
    },
  });

  if (registry.isPending || images.isPending) {
    return <PageLoading label="Flash Registryを読み込んでいます" />;
  }
  if (registry.isError || images.isError) {
    return (
      <ErrorState
        title="Flash Registryに接続できません"
        description={getApiErrorMessage(registry.error ?? images.error)}
        onRetry={() => {
          void registry.refetch();
          void images.refetch();
        }}
      />
    );
  }

  const value = registry.data;
  const usagePercent = value.storage_limit_bytes
    ? Math.min(100, (value.storage_used_bytes / value.storage_limit_bytes) * 100)
    : 0;
  const activeCredentials = value.credentials.length;
  const commands = secret ? credentialCommands(secret) : "";

  return (
    <SpaceBetween size="l">
      <PageHeader
        title="Flash Registry"
        description="Flashコンテナイメージを保存・配布する組織専用OCIレジストリ"
        actions={
          <Button
            variant="icon"
            iconName="refresh"
            ariaLabel="Flash Registry情報を更新"
            onClick={() => {
              void registry.refetch();
              void images.refetch();
            }}
          />
        }
      />

      <Container header={<Header variant="h2">Flash Registry</Header>}>
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
        trackBy="reference"
        items={images.data.items}
        header={
          <Header
            variant="h2"
            counter={`(${images.data.items.length})`}
            description="Flashで起動できるタグ付きコンテナイメージ"
          >
            イメージ
          </Header>
        }
        columnDefinitions={[
          {
            id: "repository",
            header: "リポジトリ",
            cell: (item) => <Box fontWeight="bold">{item.repository}</Box>,
          },
          {
            id: "tag",
            header: "タグ",
            cell: (item) => <Box variant="code">{item.tag}</Box>,
          },
          {
            id: "digest",
            header: "Digest",
            cell: (item) => (
              <Box variant="code">
                {item.digest.length > 24
                  ? `${item.digest.slice(0, 24)}...`
                  : item.digest}
              </Box>
            ),
          },
          {
            id: "size",
            header: "サイズ",
            cell: (item) => formatBytes(item.size_bytes),
          },
          {
            id: "pushed",
            header: "Push日時",
            cell: (item) =>
              item.pushed_at ? formatDateTime(item.pushed_at) : "-",
          },
          {
            id: "actions",
            header: "操作",
            cell: (item) => (
              <Button
                variant="inline-icon"
                iconName="remove"
                ariaLabel={`${item.repository}:${item.tag}を削除`}
                onClick={() => {
                  deleteImage.reset();
                  setDeleteTarget(item);
                }}
              />
            ),
          },
        ]}
        empty={
          <EmptyState
            title="イメージがありません"
            description="認証情報を発行してコンテナイメージをPushしてください。"
          />
        }
      />

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
            Flash Registry認証情報
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
            cell: () => <StatusIndicator type="success">有効</StatusIndicator>,
          },
          {
            id: "actions",
            header: "操作",
            cell: (item) => (
              <Button
                variant="inline-icon"
                iconName="remove"
                ariaLabel={`${item.name}を削除`}
                onClick={() => {
                  deleteCredential.reset();
                  setDeleteCredentialTarget(item);
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
        visible={deleteTarget !== null}
        header="コンテナイメージを削除"
        onDismiss={() => setDeleteTarget(null)}
        footer={
          <Box float="right">
            <SpaceBetween direction="horizontal" size="xs">
              <Button onClick={() => setDeleteTarget(null)}>キャンセル</Button>
              <Button
                variant="primary"
                iconName="remove"
                loading={deleteImage.isPending}
                onClick={() => deleteTarget && deleteImage.mutate(deleteTarget)}
              >
                削除
              </Button>
            </SpaceBetween>
          </Box>
        }
      >
        <SpaceBetween size="l">
          <Alert type="warning">
            {deleteTarget
              ? `${deleteTarget.repository}:${deleteTarget.tag} を削除します。同じDigestを参照するタグも削除され、Flashの再起動時に取得できなくなる可能性があります。`
              : ""}
          </Alert>
          <FormError
            message={deleteImage.isError ? getApiErrorMessage(deleteImage.error) : null}
          />
        </SpaceBetween>
      </Modal>

      <Modal
        visible={createOpen}
        header="Flash Registry認証情報を発行"
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
        visible={deleteCredentialTarget !== null}
        header="Flash Registry認証情報を削除"
        onDismiss={() => setDeleteCredentialTarget(null)}
        footer={
          <Box float="right">
            <SpaceBetween direction="horizontal" size="xs">
              <Button onClick={() => setDeleteCredentialTarget(null)}>キャンセル</Button>
              <Button
                variant="primary"
                iconName="remove"
                loading={deleteCredential.isPending}
                onClick={() =>
                  deleteCredentialTarget && deleteCredential.mutate(deleteCredentialTarget.id)
                }
              >
                削除
              </Button>
            </SpaceBetween>
          </Box>
        }
      >
        <SpaceBetween size="l">
          <Alert type="warning">
            {deleteCredentialTarget?.name ?? ""}
            を削除します。この認証情報を使用するPush/Pullは直ちに拒否されます。
          </Alert>
          <FormError
            message={
              deleteCredential.isError ? getApiErrorMessage(deleteCredential.error) : null
            }
          />
        </SpaceBetween>
      </Modal>
    </SpaceBetween>
  );
}
