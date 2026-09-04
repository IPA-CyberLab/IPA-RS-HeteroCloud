import Alert from "@cloudscape-design/components/alert";
import Badge from "@cloudscape-design/components/badge";
import Box from "@cloudscape-design/components/box";
import Button from "@cloudscape-design/components/button";
import ColumnLayout from "@cloudscape-design/components/column-layout";
import FormField from "@cloudscape-design/components/form-field";
import Header from "@cloudscape-design/components/header";
import Input from "@cloudscape-design/components/input";
import Modal from "@cloudscape-design/components/modal";
import Multiselect from "@cloudscape-design/components/multiselect";
import SpaceBetween from "@cloudscape-design/components/space-between";
import Table from "@cloudscape-design/components/table";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import { EmptyState } from "@/components/shared/empty-state";
import { FormError } from "@/components/shared/form-error";
import { StatusBadge } from "@/components/shared/status-badge";
import { api, getApiErrorMessage } from "@/lib/api-client";
import type {
  SyouyuCredential,
  SyouyuCredentialSecret,
  SyouyuPermission,
} from "@/lib/api-types";
import { syouyuCredentialsQueryOptions } from "@/lib/queries";
import { formatDateTime } from "@/lib/utils";

export const syouyuPermissions = [
  { value: "read", label: "読み取り" },
  { value: "write", label: "書き込み（作成・更新・削除を含む）" },
] as const;
type DisplayedPermission = (typeof syouyuPermissions)[number]["value"];

const permissionLabels = new Map<DisplayedPermission, string>(
  syouyuPermissions.map((permission) => [permission.value, permission.label]),
);

function shellQuote(value: string): string {
  return `'${value.replaceAll("'", `'\\''`)}'`;
}

export function awsCliSetup(secret: SyouyuCredentialSecret): string {
  return [
    `export AWS_ACCESS_KEY_ID=${shellQuote(secret.credential.access_key_id)}`,
    `export AWS_SECRET_ACCESS_KEY=${shellQuote(secret.secret_access_key)}`,
    `export AWS_DEFAULT_REGION=${shellQuote(secret.region)}`,
    `aws --endpoint-url ${shellQuote(secret.endpoint)} s3 ls ${shellQuote(`s3://${secret.bucket}`)}`,
  ].join("\n");
}

function PermissionList({ permissions }: { permissions: SyouyuPermission[] }) {
  const visiblePermissions = permissions.filter(
    (permission): permission is DisplayedPermission =>
      permission === "read" || permission === "write",
  );
  if (!visiblePermissions.length) {
    return <Box color="text-body-secondary">なし</Box>;
  }
  return (
    <SpaceBetween direction="horizontal" size="xxs">
      {visiblePermissions.map((permission) => (
        <Badge key={permission}>
          {permissionLabels.get(permission) ?? permission}
        </Badge>
      ))}
    </SpaceBetween>
  );
}

export function SyouyuCredentialsSection({
  organizationId,
  bucketId,
  maxCredentials,
  disabled = false,
}: {
  organizationId: string;
  bucketId: string;
  maxCredentials: number;
  disabled?: boolean;
}) {
  const queryClient = useQueryClient();
  const credentialsOptions = syouyuCredentialsQueryOptions(
    organizationId,
    bucketId,
  );
  const credentials = useQuery(credentialsOptions);
  const [createOpen, setCreateOpen] = useState(false);
  const [createIdempotencyKey, setCreateIdempotencyKey] = useState<string | null>(
    null,
  );
  const [name, setName] = useState("");
  const [permissions, setPermissions] = useState<SyouyuPermission[]>(
    syouyuPermissions.map((permission) => permission.value),
  );
  const [secret, setSecret] = useState<SyouyuCredentialSecret | null>(null);
  const [copied, setCopied] = useState<"access-key" | "secret" | "setup" | null>(
    null,
  );
  const [revokeTarget, setRevokeTarget] = useState<SyouyuCredential | null>(null);
  const [revokeIdempotencyKey, setRevokeIdempotencyKey] = useState<string | null>(
    null,
  );

  useEffect(() => {
    setSecret(null);
    setCopied(null);
    setCreateIdempotencyKey(null);
    setRevokeTarget(null);
    setRevokeIdempotencyKey(null);
  }, [organizationId, bucketId]);

  const refresh = () =>
    queryClient.invalidateQueries({ queryKey: credentialsOptions.queryKey });
  const createCredential = useMutation({
    mutationFn: () => {
      if (!createIdempotencyKey) throw new Error("missing idempotency key");
      return api.syouyu.buckets.credentials.create(
        organizationId,
        bucketId,
        { name: name.trim(), permissions },
        createIdempotencyKey,
      );
    },
    onSuccess: async (value) => {
      setCreateOpen(false);
      setCreateIdempotencyKey(null);
      setSecret(value);
      await refresh();
    },
  });
  const revokeCredential = useMutation({
    mutationFn: (credentialId: string) => {
      if (!revokeIdempotencyKey) throw new Error("missing idempotency key");
      return api.syouyu.buckets.credentials.revoke(
        organizationId,
        bucketId,
        credentialId,
        revokeIdempotencyKey,
      );
    },
    onSuccess: async () => {
      setRevokeTarget(null);
      setRevokeIdempotencyKey(null);
      await refresh();
    },
  });

  const items = credentials.data?.items ?? [];
  const activeCount = items.filter((credential) => credential.status === "active").length;
  const canCreate =
    !disabled &&
    activeCount < maxCredentials &&
    name.trim().length > 0 &&
    permissions.length > 0;
  const setup = useMemo(() => (secret ? awsCliSetup(secret) : ""), [secret]);
  const copy = async (
    kind: "access-key" | "secret" | "setup",
    value: string,
  ) => {
    await navigator.clipboard.writeText(value);
    setCopied(kind);
  };
  const closeSecret = () => {
    setSecret(null);
    setCopied(null);
    createCredential.reset();
  };
  const closeCreate = () => {
    setCreateOpen(false);
    setCreateIdempotencyKey(null);
    createCredential.reset();
  };
  const closeRevoke = () => {
    setRevokeTarget(null);
    setRevokeIdempotencyKey(null);
    revokeCredential.reset();
  };
  const changeCreateInput = (change: () => void) => {
    if (createCredential.isError) {
      createCredential.reset();
      setCreateIdempotencyKey(crypto.randomUUID());
    }
    change();
  };

  return (
    <>
      <Table
        variant="container"
        stickyHeader
        wrapLines
        trackBy="id"
        items={items}
        loading={credentials.isPending}
        loadingText="アクセス認証情報を読み込んでいます"
        header={
          <Header
            variant="h2"
            counter={credentials.data ? `(${items.length})` : undefined}
            description="このバケットだけにアクセスできるS3認証情報"
            actions={
              <SpaceBetween direction="horizontal" size="xs">
                <Button
                  variant="icon"
                  iconName="refresh"
                  ariaLabel="アクセス認証情報を更新"
                  onClick={() => void credentials.refetch()}
                />
                <Button
                  variant="primary"
                  iconName="add-plus"
                  disabled={disabled || activeCount >= maxCredentials}
                  onClick={() => {
                    setName("");
                    setPermissions(
                      syouyuPermissions.map((permission) => permission.value),
                    );
                    createCredential.reset();
                    setCreateIdempotencyKey(crypto.randomUUID());
                    setCreateOpen(true);
                  }}
                >
                  認証情報を発行
                </Button>
              </SpaceBetween>
            }
          >
            アクセス認証情報
          </Header>
        }
        columnDefinitions={[
          {
            id: "name",
            header: "名前",
            cell: (item) => <Box fontWeight="bold">{item.name}</Box>,
          },
          {
            id: "accessKey",
            header: "アクセスキーID",
            cell: (item) => <Box variant="code">{item.access_key_id}</Box>,
          },
          {
            id: "permissions",
            header: "権限",
            cell: (item) => <PermissionList permissions={item.permissions} />,
          },
          {
            id: "created",
            header: "発行日時",
            cell: (item) => formatDateTime(item.created_at),
          },
          {
            id: "status",
            header: "状態",
            cell: (item) => <StatusBadge status={item.status} />,
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
                  revokeCredential.reset();
                  setRevokeIdempotencyKey(crypto.randomUUID());
                  setRevokeTarget(item);
                }}
              />
            ),
          },
        ]}
        empty={
          credentials.isError ? (
            <Alert
              type="error"
              action={<Button onClick={() => void credentials.refetch()}>再試行</Button>}
            >
              アクセス認証情報を取得できませんでした。
            </Alert>
          ) : (
            <EmptyState
              title="アクセス認証情報がありません"
              description="S3クライアント用の認証情報を発行してください。"
            />
          )
        }
      />

      <Modal
        visible={createOpen}
        onDismiss={closeCreate}
        size="large"
        header="アクセス認証情報を発行"
        footer={
          <Box float="right">
            <SpaceBetween direction="horizontal" size="xs">
              <Button onClick={closeCreate}>キャンセル</Button>
              <Button
                variant="primary"
                iconName="key"
                loading={createCredential.isPending}
                disabled={!canCreate}
                onClick={() => createCredential.mutate()}
              >
                発行
              </Button>
            </SpaceBetween>
          </Box>
        }
      >
        <SpaceBetween size="l">
          <FormField label="名前" description="利用するアプリや環境を識別する名前">
            <Input
              value={name}
              autoComplete="off"
              placeholder="production-backend"
              disabled={createCredential.isPending}
              onChange={({ detail }) =>
                changeCreateInput(() => setName(detail.value.slice(0, 120)))
              }
            />
          </FormField>
          <FormField
            label="権限"
            description="書き込み権限にはオブジェクトの作成、更新、削除が含まれます。"
          >
            <Multiselect
              selectedOptions={syouyuPermissions.filter((permission) =>
                permissions.includes(permission.value),
              )}
              options={syouyuPermissions}
              placeholder="権限を選択"
              disabled={createCredential.isPending}
              onChange={({ detail }) =>
                changeCreateInput(() =>
                  setPermissions(
                    detail.selectedOptions.flatMap((option) =>
                      option.value ? [option.value as SyouyuPermission] : [],
                    ),
                  ),
                )
              }
            />
          </FormField>
          <FormError
            message={
              createCredential.isError
                ? getApiErrorMessage(createCredential.error)
                : null
            }
          />
        </SpaceBetween>
      </Modal>

      <Modal
        visible={secret !== null}
        onDismiss={closeSecret}
        size="large"
        header="アクセス認証情報を保存"
        footer={
          <Box float="right">
            <Button variant="primary" onClick={closeSecret}>
              閉じる
            </Button>
          </Box>
        }
      >
        {secret ? (
          <SpaceBetween size="l">
            <Alert type="warning">
              シークレットアクセスキーは今回だけ表示されます。閉じると再表示できません。
            </Alert>
            <ColumnLayout columns={2} variant="text-grid">
              <div>
                <Box variant="awsui-key-label">アクセスキーID</Box>
                <SpaceBetween direction="horizontal" size="xs">
                  <Box variant="code">{secret.credential.access_key_id}</Box>
                  <Button
                    variant="inline-icon"
                    iconName={copied === "access-key" ? "check" : "copy"}
                    ariaLabel="アクセスキーIDをコピー"
                    onClick={() =>
                      void copy("access-key", secret.credential.access_key_id)
                    }
                  />
                </SpaceBetween>
              </div>
              <div>
                <Box variant="awsui-key-label">シークレットアクセスキー</Box>
                <SpaceBetween direction="horizontal" size="xs">
                  <Box variant="code">{secret.secret_access_key}</Box>
                  <Button
                    variant="inline-icon"
                    iconName={copied === "secret" ? "check" : "copy"}
                    ariaLabel="シークレットアクセスキーをコピー"
                    onClick={() => void copy("secret", secret.secret_access_key)}
                  />
                </SpaceBetween>
              </div>
              <div>
                <Box variant="awsui-key-label">エンドポイント</Box>
                <Box variant="code">{secret.endpoint}</Box>
              </div>
              <div>
                <Box variant="awsui-key-label">リージョン</Box>
                <Box variant="code">{secret.region}</Box>
              </div>
              <div>
                <Box variant="awsui-key-label">バケット</Box>
                <Box variant="code">{secret.bucket}</Box>
              </div>
            </ColumnLayout>
            <div>
              <Header
                variant="h3"
                actions={
                  <Button
                    iconName={copied === "setup" ? "check" : "copy"}
                    onClick={() => void copy("setup", setup)}
                  >
                    {copied === "setup" ? "コピー済み" : "AWS CLI設定をコピー"}
                  </Button>
                }
              >
                AWS CLI
              </Header>
              <pre className="code-block"><code>{setup}</code></pre>
            </div>
          </SpaceBetween>
        ) : null}
      </Modal>

      <Modal
        visible={revokeTarget !== null}
        onDismiss={closeRevoke}
        header="アクセス認証情報を失効"
        footer={
          <Box float="right">
            <SpaceBetween direction="horizontal" size="xs">
              <Button onClick={closeRevoke}>キャンセル</Button>
              <Button
                variant="primary"
                iconName="remove"
                loading={revokeCredential.isPending}
                onClick={() =>
                  revokeTarget && revokeCredential.mutate(revokeTarget.id)
                }
              >
                失効する
              </Button>
            </SpaceBetween>
          </Box>
        }
      >
        <SpaceBetween size="l">
          <Alert type="warning" header="この操作は取り消せません">
            {revokeTarget?.name ?? ""} を直ちに使用できなくします。
          </Alert>
          <FormError
            message={
              revokeCredential.isError
                ? getApiErrorMessage(revokeCredential.error)
                : null
            }
          />
        </SpaceBetween>
      </Modal>
    </>
  );
}
