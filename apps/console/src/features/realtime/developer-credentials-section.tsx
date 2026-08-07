import Alert from "@cloudscape-design/components/alert";
import Badge from "@cloudscape-design/components/badge";
import Box from "@cloudscape-design/components/box";
import Button from "@cloudscape-design/components/button";
import FormField from "@cloudscape-design/components/form-field";
import Header from "@cloudscape-design/components/header";
import Input from "@cloudscape-design/components/input";
import Modal from "@cloudscape-design/components/modal";
import Multiselect from "@cloudscape-design/components/multiselect";
import SpaceBetween from "@cloudscape-design/components/space-between";
import StatusIndicator from "@cloudscape-design/components/status-indicator";
import Table from "@cloudscape-design/components/table";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import { EmptyState } from "@/components/shared/empty-state";
import { FormError } from "@/components/shared/form-error";
import { TablePagination } from "@/components/shared/table-pagination";
import { api, getApiErrorMessage } from "@/lib/api-client";
import type {
  RealtimeAccessContext,
  RealtimeDeveloperCredential,
  RealtimeDeveloperCredentialSecret,
} from "@/lib/api-types";
import {
  realtimeAccessContextsQueryOptions,
  realtimeDeveloperCredentialsQueryOptions,
} from "@/lib/queries";
import { formatCredentialDate, realtimePermissions } from "./realtime-service-utils";

type SecretResult = {
  action: "created" | "rotated";
  value: RealtimeDeveloperCredentialSecret;
};
type ResourceStatus = "active" | "expired" | "revoked";
const ACCESS_CONTEXT_PAGE_SIZE = 10;
const permissionLabels = new Map<string, string>(
  realtimePermissions.map((permission) => [permission.value, permission.label]),
);

function resourceStatus(expiresAt: string, revokedAt: string | null): ResourceStatus {
  if (revokedAt) return "revoked";
  const expiry = Date.parse(expiresAt);
  return Number.isFinite(expiry) && expiry <= Date.now() ? "expired" : "active";
}

function ResourceStatusIndicator({ status }: { status: ResourceStatus }) {
  const values = {
    active: ["success", "有効"],
    expired: ["warning", "期限切れ"],
    revoked: ["stopped", "失効済み"],
  } as const;
  return <StatusIndicator type={values[status][0]}>{values[status][1]}</StatusIndicator>;
}

function PermissionList({ permissions }: { permissions: string[] }) {
  if (!permissions.length) return <Box color="text-body-secondary">なし</Box>;
  return (
    <SpaceBetween direction="horizontal" size="xxs">
      {permissions.map((permission) => (
        <Badge key={permission}>{permissionLabels.get(permission) ?? permission}</Badge>
      ))}
    </SpaceBetween>
  );
}

function shellQuote(value: string): string {
  return `'${value.replaceAll("'", `'\\''`)}'`;
}

export function buildAccessCredentialCurl(
  mintEndpoint: string,
  credential: string,
  permissions: string[],
): string {
  const body = JSON.stringify(
    {
      principal_id: "0198a118-073f-79e4-9ca4-0c1c2501c031",
      permissions: permissions.slice(0, 2),
      expires_in_seconds: 300,
    },
    null,
    2,
  );
  const issue = [
    `curl --request POST ${shellQuote(mintEndpoint)}`,
    `  --header ${shellQuote(`Authorization: Bearer ${credential}`)}`,
    `  --header ${shellQuote("Content-Type: application/json")}`,
    `  --data ${shellQuote(body)}`,
  ].join(" \\\n");
  const revokeEndpoint = `${mintEndpoint.replace(/\/$/, "")}/{context_id}`;
  const revoke = [
    `curl --request DELETE ${shellQuote(revokeEndpoint)}`,
    `  --header ${shellQuote(`Authorization: Bearer ${credential}`)}`,
  ].join(" \\\n");
  return `# 短期アクセスを発行\n${issue}\n\n# 返却された context_id を失効\n${revoke}`;
}

export function DeveloperCredentialsSection({
  organizationId,
  serviceId,
  disabled = false,
}: {
  organizationId: string;
  serviceId: string;
  disabled?: boolean;
}) {
  const queryClient = useQueryClient();
  const credentialsOptions = realtimeDeveloperCredentialsQueryOptions(organizationId, serviceId);
  const contextsOptions = realtimeAccessContextsQueryOptions(organizationId, serviceId);
  const credentials = useQuery(credentialsOptions);
  const contexts = useQuery(contextsOptions);
  const [createOpen, setCreateOpen] = useState(false);
  const [name, setName] = useState("");
  const [expiresInDays, setExpiresInDays] = useState(90);
  const [permissions, setPermissions] = useState<string[]>(
    realtimePermissions.map((permission) => permission.value),
  );
  const [secretResult, setSecretResult] = useState<SecretResult | null>(null);
  const [copied, setCopied] = useState<"credential" | "curl" | null>(null);
  const [rotateTarget, setRotateTarget] = useState<RealtimeDeveloperCredential | null>(null);
  const [revokeCredentialTarget, setRevokeCredentialTarget] =
    useState<RealtimeDeveloperCredential | null>(null);
  const [revokeContextTarget, setRevokeContextTarget] = useState<RealtimeAccessContext | null>(null);
  const [accessContextPage, setAccessContextPage] = useState(0);
  const accessContextItems = contexts.data?.items ?? [];
  const accessContextPageCount = Math.max(
    1,
    Math.ceil(accessContextItems.length / ACCESS_CONTEXT_PAGE_SIZE),
  );
  const resolvedAccessContextPage = Math.min(accessContextPage, accessContextPageCount - 1);
  const visibleAccessContexts = accessContextItems.slice(
    resolvedAccessContextPage * ACCESS_CONTEXT_PAGE_SIZE,
    (resolvedAccessContextPage + 1) * ACCESS_CONTEXT_PAGE_SIZE,
  );

  useEffect(() => {
    setAccessContextPage((current) => Math.min(current, accessContextPageCount - 1));
  }, [accessContextPageCount]);
  useEffect(() => setAccessContextPage(0), [organizationId, serviceId]);

  const invalidateCredentials = () =>
    queryClient.invalidateQueries({ queryKey: credentialsOptions.queryKey });
  const invalidateContexts = () =>
    queryClient.invalidateQueries({ queryKey: contextsOptions.queryKey });
  const createCredential = useMutation({
    mutationFn: () =>
      api.realtime.services.createDeveloperCredential(organizationId, serviceId, {
        name: name.trim(),
        expires_in_days: expiresInDays,
        permissions,
      }),
    onSuccess: async (value) => {
      setCreateOpen(false);
      setSecretResult({ action: "created", value });
      await invalidateCredentials();
    },
  });
  const rotateCredential = useMutation({
    mutationFn: (credentialId: string) =>
      api.realtime.services.rotateDeveloperCredential(organizationId, serviceId, credentialId),
    onSuccess: async (value) => {
      setRotateTarget(null);
      setSecretResult({ action: "rotated", value });
      await Promise.all([invalidateCredentials(), invalidateContexts()]);
    },
  });
  const revokeCredential = useMutation({
    mutationFn: (credentialId: string) =>
      api.realtime.services.revokeDeveloperCredential(organizationId, serviceId, credentialId),
    onSuccess: async () => {
      setRevokeCredentialTarget(null);
      await Promise.all([invalidateCredentials(), invalidateContexts()]);
    },
  });
  const revokeContext = useMutation({
    mutationFn: (contextId: string) =>
      api.realtime.services.revokeAccessContext(organizationId, serviceId, contextId),
    onSuccess: async () => {
      setRevokeContextTarget(null);
      await invalidateContexts();
    },
  });
  const validCreate =
    name.trim().length > 0 &&
    Number.isInteger(expiresInDays) &&
    expiresInDays >= 1 &&
    expiresInDays <= 365 &&
    permissions.length > 0;
  const curl = useMemo(
    () =>
      secretResult
        ? buildAccessCredentialCurl(
            secretResult.value.mint_endpoint,
            secretResult.value.credential,
            secretResult.value.permissions,
          )
        : "",
    [secretResult],
  );
  const copy = async (kind: "credential" | "curl", value: string) => {
    await navigator.clipboard.writeText(value);
    setCopied(kind);
    window.setTimeout(() => setCopied(null), 1_500);
  };
  const clearSecret = () => {
    setSecretResult(null);
    setCopied(null);
    createCredential.reset();
    rotateCredential.reset();
  };

  const credentialItems = credentials.data?.items ?? [];
  return (
    <SpaceBetween size="xl">
      <Table
        variant="container"
        header={
          <Header
            variant="h2"
            description="サーバーから短期アクセスを発行するための認証情報"
            counter={credentials.data ? `(${credentialItems.length})` : undefined}
            actions={
              <SpaceBetween direction="horizontal" size="xs">
                <Button
                  variant="icon"
                  iconName="refresh"
                  ariaLabel="認証情報と短期アクセスを更新"
                  onClick={() => void Promise.all([credentials.refetch(), contexts.refetch()])}
                />
                <Button
                  variant="primary"
                  iconName="add-plus"
                  disabled={disabled}
                  onClick={() => {
                    setName("");
                    setExpiresInDays(90);
                    setPermissions(realtimePermissions.map((permission) => permission.value));
                    createCredential.reset();
                    setCreateOpen(true);
                  }}
                >
                  開発者認証情報を作成
                </Button>
              </SpaceBetween>
            }
          >
            開発者認証情報
          </Header>
        }
        loading={credentials.isPending}
        loadingText="開発者認証情報を読み込んでいます"
        items={credentialItems}
        trackBy="id"
        columnDefinitions={[
          {
            id: "name",
            header: "名前 / Prefix",
            cell: (item) => (
              <SpaceBetween size="xxs">
                <Box fontWeight="bold">{item.name}</Box>
                <Box variant="code">{item.prefix}</Box>
              </SpaceBetween>
            ),
          },
          { id: "permissions", header: "権限上限", cell: (item) => <PermissionList permissions={item.permissions} /> },
          { id: "expires", header: "有効期限", cell: (item) => formatCredentialDate(item.expires_at) },
          { id: "used", header: "最終使用", cell: (item) => item.last_used_at ? formatCredentialDate(item.last_used_at) : "未使用" },
          {
            id: "status",
            header: "状態",
            cell: (item) => <ResourceStatusIndicator status={resourceStatus(item.expires_at, item.revoked_at)} />,
          },
          {
            id: "actions",
            header: "操作",
            cell: (item) => {
              const revoked = resourceStatus(item.expires_at, item.revoked_at) === "revoked";
              return (
                <SpaceBetween direction="horizontal" size="xxs">
                  <Button
                    variant="inline-icon"
                    iconName="refresh"
                    ariaLabel={`${item.name}をローテーション`}
                    disabled={disabled || revoked}
                    onClick={() => {
                      rotateCredential.reset();
                      setRotateTarget(item);
                    }}
                  />
                  <Button
                    variant="inline-icon"
                    iconName="remove"
                    ariaLabel={`${item.name}を失効`}
                    disabled={revoked}
                    onClick={() => {
                      revokeCredential.reset();
                      setRevokeCredentialTarget(item);
                    }}
                  />
                </SpaceBetween>
              );
            },
          },
        ]}
        empty={
          credentials.isError ? (
            <Alert
              type="error"
              action={<Button onClick={() => void credentials.refetch()}>再試行</Button>}
            >
              開発者認証情報を取得できませんでした。
            </Alert>
          ) : (
            <EmptyState
              title="開発者認証情報がありません"
              description="バックエンド連携用の認証情報を作成してください。"
            />
          )
        }
      />

      <Table
        variant="container"
        header={
          <Header
            variant="h2"
            description="このサービスで発行された直近100件の利用者コンテキスト"
            counter={contexts.data ? `(${accessContextItems.length})` : undefined}
          >
            発行済み短期アクセス
          </Header>
        }
        loading={contexts.isPending}
        loadingText="短期アクセスを読み込んでいます"
        items={visibleAccessContexts}
        trackBy="context_id"
        columnDefinitions={[
          { id: "subject", header: "Subject", cell: (item) => <Box variant="code">{item.principal_id}</Box> },
          { id: "permissions", header: "権限", cell: (item) => <PermissionList permissions={item.permissions} /> },
          { id: "issued", header: "発行日時", cell: (item) => formatCredentialDate(item.issued_at) },
          { id: "expires", header: "有効期限", cell: (item) => formatCredentialDate(item.expires_at) },
          {
            id: "status",
            header: "状態",
            cell: (item) => <ResourceStatusIndicator status={resourceStatus(item.expires_at, item.revoked_at)} />,
          },
          {
            id: "actions",
            header: "操作",
            cell: (item) => (
              <Button
                variant="inline-icon"
                iconName="remove"
                ariaLabel={`${item.principal_id}の短期アクセスを失効`}
                disabled={resourceStatus(item.expires_at, item.revoked_at) !== "active"}
                onClick={() => {
                  revokeContext.reset();
                  setRevokeContextTarget(item);
                }}
              />
            ),
          },
        ]}
        pagination={
          accessContextItems.length ? (
            <TablePagination
              pageIndex={resolvedAccessContextPage}
              pageCount={accessContextPageCount}
              pageSize={ACCESS_CONTEXT_PAGE_SIZE}
              totalItems={accessContextItems.length}
              onPageChange={setAccessContextPage}
            />
          ) : null
        }
        empty={
          contexts.isError ? (
            <Alert
              type="error"
              action={<Button onClick={() => void contexts.refetch()}>再試行</Button>}
            >
              発行済み短期アクセスを取得できませんでした。
            </Alert>
          ) : (
            <EmptyState
              title="発行済み短期アクセスがありません"
              description="バックエンドから発行された短期アクセスがここに表示されます。"
            />
          )
        }
      />

      <Modal
        visible={createOpen}
        onDismiss={() => setCreateOpen(false)}
        size="large"
        header="開発者認証情報を作成"
        footer={
          <Box float="right">
            <SpaceBetween direction="horizontal" size="xs">
              <Button onClick={() => setCreateOpen(false)}>キャンセル</Button>
              <Button
                variant="primary"
                iconName="key"
                loading={createCredential.isPending}
                disabled={!validCreate}
                onClick={() => createCredential.mutate()}
              >
                作成
              </Button>
            </SpaceBetween>
          </Box>
        }
      >
        <SpaceBetween size="l">
          <Box color="text-body-secondary">
            この認証情報で発行できる短期アクセスの権限上限を設定します。
          </Box>
          <FormField label="名前">
            <Input
              value={name}
              autoComplete="off"
              placeholder="production-backend"
              disabled={createCredential.isPending}
              onChange={({ detail }) => setName(detail.value.slice(0, 120))}
            />
          </FormField>
          <FormField label="有効期間（日）" constraintText="1〜365日">
            <Input
              type="number"
              value={String(expiresInDays)}
              disabled={createCredential.isPending}
              onChange={({ detail }) => setExpiresInDays(Number(detail.value))}
            />
          </FormField>
          <FormField label="権限上限">
            <Multiselect
              selectedOptions={realtimePermissions.filter((option) => permissions.includes(option.value))}
              options={realtimePermissions}
              disabled={createCredential.isPending}
              placeholder="権限を選択"
              tokenLimit={4}
              onChange={({ detail }) =>
                setPermissions(detail.selectedOptions.flatMap((option) => option.value ? [option.value] : []))
              }
            />
          </FormField>
          <FormError message={createCredential.isError ? getApiErrorMessage(createCredential.error) : null} />
        </SpaceBetween>
      </Modal>

      <Modal
        visible={Boolean(secretResult)}
        onDismiss={clearSecret}
        size="large"
        header={secretResult?.action === "rotated" ? "新しい認証情報を保存" : "開発者認証情報を保存"}
        footer={<Box float="right"><Button variant="primary" onClick={clearSecret}>閉じる</Button></Box>}
      >
        {secretResult ? (
          <SpaceBetween size="l">
            <Alert type="warning">秘密値は今回だけ表示されます。閉じると再表示できません。</Alert>
            <div>
              <Header
                variant="h3"
                actions={
                  <Button
                    iconName={copied === "credential" ? "check" : "copy"}
                    onClick={() => void copy("credential", secretResult.value.credential)}
                  >
                    {copied === "credential" ? "コピー済み" : "秘密値をコピー"}
                  </Button>
                }
              >
                秘密値
              </Header>
              <Box variant="code">{secretResult.value.credential}</Box>
            </div>
            <div>
              <Header
                variant="h3"
                actions={
                  <Button
                    iconName={copied === "curl" ? "check" : "copy"}
                    onClick={() => void copy("curl", curl)}
                  >
                    {copied === "curl" ? "コピー済み" : "curl例をコピー"}
                  </Button>
                }
              >
                サーバー側の発行・失効例
              </Header>
              <pre className="code-block"><code>{curl}</code></pre>
            </div>
          </SpaceBetween>
        ) : null}
      </Modal>

      <ConfirmModal
        visible={Boolean(rotateTarget)}
        header="認証情報をローテーション"
        description={`${rotateTarget?.name ?? ""} の現在の秘密値と発行済み短期アクセスを失効し、新しい値を発行します。`}
        actionLabel="ローテーションする"
        loading={rotateCredential.isPending}
        error={rotateCredential.isError ? getApiErrorMessage(rotateCredential.error) : null}
        onDismiss={() => setRotateTarget(null)}
        onConfirm={() => rotateTarget && rotateCredential.mutate(rotateTarget.id)}
      />
      <ConfirmModal
        visible={Boolean(revokeCredentialTarget)}
        header="開発者認証情報を失効"
        description={`${revokeCredentialTarget?.name ?? ""} と、その認証情報から発行済みの短期アクセスを直ちに使用できなくします。`}
        actionLabel="失効する"
        loading={revokeCredential.isPending}
        error={revokeCredential.isError ? getApiErrorMessage(revokeCredential.error) : null}
        destructive
        onDismiss={() => setRevokeCredentialTarget(null)}
        onConfirm={() => revokeCredentialTarget && revokeCredential.mutate(revokeCredentialTarget.id)}
      />
      <ConfirmModal
        visible={Boolean(revokeContextTarget)}
        header="短期アクセスを失効"
        description={`${revokeContextTarget?.principal_id ?? ""} のアクセスを直ちに無効化します。`}
        actionLabel="失効する"
        loading={revokeContext.isPending}
        error={revokeContext.isError ? getApiErrorMessage(revokeContext.error) : null}
        destructive
        onDismiss={() => setRevokeContextTarget(null)}
        onConfirm={() => revokeContextTarget && revokeContext.mutate(revokeContextTarget.context_id)}
      />
    </SpaceBetween>
  );
}

function ConfirmModal({
  visible,
  header,
  description,
  actionLabel,
  loading,
  error,
  destructive,
  onDismiss,
  onConfirm,
}: {
  visible: boolean;
  header: string;
  description: string;
  actionLabel: string;
  loading: boolean;
  error: string | null;
  destructive?: boolean;
  onDismiss: () => void;
  onConfirm: () => void;
}) {
  return (
    <Modal
      visible={visible}
      onDismiss={onDismiss}
      header={header}
      footer={
        <Box float="right">
          <SpaceBetween direction="horizontal" size="xs">
            <Button onClick={onDismiss}>キャンセル</Button>
            <Button
              variant={destructive ? "primary" : "primary"}
              iconName={destructive ? "remove" : "refresh"}
              loading={loading}
              onClick={onConfirm}
            >
              {actionLabel}
            </Button>
          </SpaceBetween>
        </Box>
      }
    >
      <SpaceBetween size="l">
        <Alert type={destructive ? "warning" : "info"}>{description}</Alert>
        <FormError message={error} />
      </SpaceBetween>
    </Modal>
  );
}
