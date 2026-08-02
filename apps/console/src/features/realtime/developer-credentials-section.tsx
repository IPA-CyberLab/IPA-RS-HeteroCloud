import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Ban,
  Check,
  Copy,
  KeyRound,
  LoaderCircle,
  Plus,
  RefreshCw,
  RotateCw,
} from "lucide-react";
import { type FormEvent, useMemo, useState } from "react";
import { EmptyState } from "@/components/shared/empty-state";
import { FormError } from "@/components/shared/form-error";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
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
import {
  formatCredentialDate,
  realtimePermissions,
} from "./realtime-service-utils";

interface DeveloperCredentialsSectionProps {
  organizationId: string;
  serviceId: string;
  disabled?: boolean;
}

type SecretResult = {
  action: "created" | "rotated";
  value: RealtimeDeveloperCredentialSecret;
};

type ResourceStatus = "active" | "expired" | "revoked";

const permissionLabels = new Map<string, string>(
  realtimePermissions.map((permission) => [permission.value, permission.label]),
);

function resourceStatus(
  expiresAt: string,
  revokedAt: string | null,
): ResourceStatus {
  if (revokedAt) return "revoked";
  const expiresAtMs = Date.parse(expiresAt);
  return Number.isFinite(expiresAtMs) && expiresAtMs <= Date.now()
    ? "expired"
    : "active";
}

function ResourceStatusBadge({ status }: { status: ResourceStatus }) {
  const values = {
    active: { label: "有効", variant: "success" },
    expired: { label: "期限切れ", variant: "warning" },
    revoked: { label: "失効済み", variant: "neutral" },
  } as const;
  return <Badge variant={values[status].variant}>{values[status].label}</Badge>;
}

function PermissionList({ permissions }: { permissions: string[] }) {
  if (permissions.length === 0) return <span className="text-zinc-500">なし</span>;
  return (
    <div className="flex min-w-48 flex-wrap gap-1.5">
      {permissions.map((permission) => (
        <Badge key={permission} variant="neutral" className="whitespace-normal">
          {permissionLabels.get(permission) ?? permission}
        </Badge>
      ))}
    </div>
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
}: DeveloperCredentialsSectionProps) {
  const queryClient = useQueryClient();
  const credentialsOptions = realtimeDeveloperCredentialsQueryOptions(
    organizationId,
    serviceId,
  );
  const contextsOptions = realtimeAccessContextsQueryOptions(
    organizationId,
    serviceId,
  );
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
  const [rotateTarget, setRotateTarget] =
    useState<RealtimeDeveloperCredential | null>(null);
  const [revokeCredentialTarget, setRevokeCredentialTarget] =
    useState<RealtimeDeveloperCredential | null>(null);
  const [revokeContextTarget, setRevokeContextTarget] =
    useState<RealtimeAccessContext | null>(null);

  const invalidateCredentials = () =>
    queryClient.invalidateQueries({ queryKey: credentialsOptions.queryKey });
  const invalidateContexts = () =>
    queryClient.invalidateQueries({ queryKey: contextsOptions.queryKey });

  const createCredential = useMutation({
    mutationFn: () =>
      api.realtime.services.createDeveloperCredential(
        organizationId,
        serviceId,
        {
          name: name.trim(),
          expires_in_days: expiresInDays,
          permissions,
        },
      ),
    onSuccess: async (value) => {
      setCreateOpen(false);
      setSecretResult({ action: "created", value });
      await invalidateCredentials();
    },
  });

  const rotateCredential = useMutation({
    mutationFn: (credentialId: string) =>
      api.realtime.services.rotateDeveloperCredential(
        organizationId,
        serviceId,
        credentialId,
      ),
    onSuccess: async (value) => {
      setRotateTarget(null);
      setSecretResult({ action: "rotated", value });
      await Promise.all([invalidateCredentials(), invalidateContexts()]);
    },
  });

  const revokeCredential = useMutation({
    mutationFn: (credentialId: string) =>
      api.realtime.services.revokeDeveloperCredential(
        organizationId,
        serviceId,
        credentialId,
      ),
    onSuccess: async () => {
      setRevokeCredentialTarget(null);
      await Promise.all([invalidateCredentials(), invalidateContexts()]);
    },
  });

  const revokeContext = useMutation({
    mutationFn: (contextId: string) =>
      api.realtime.services.revokeAccessContext(
        organizationId,
        serviceId,
        contextId,
      ),
    onSuccess: async () => {
      setRevokeContextTarget(null);
      await invalidateContexts();
    },
  });

  const resetCreateForm = () => {
    setName("");
    setExpiresInDays(90);
    setPermissions(realtimePermissions.map((permission) => permission.value));
    createCredential.reset();
  };

  const handleCreateOpenChange = (nextOpen: boolean) => {
    if (nextOpen) resetCreateForm();
    setCreateOpen(nextOpen);
  };

  const submitCreate = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    createCredential.mutate();
  };

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
    try {
      await navigator.clipboard.writeText(value);
      setCopied(kind);
      window.setTimeout(() => setCopied(null), 1_500);
    } catch {
      setCopied(null);
    }
  };

  const clearSecret = () => {
    setSecretResult(null);
    setCopied(null);
    createCredential.reset();
    rotateCredential.reset();
  };

  const refreshAll = () => Promise.all([credentials.refetch(), contexts.refetch()]);

  return (
    <div className="space-y-8">
      <section aria-labelledby="developer-credentials-heading" className="space-y-3">
        <div className="flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
          <div>
            <h2
              id="developer-credentials-heading"
              className="text-sm font-semibold text-zinc-950"
            >
              開発者認証情報
            </h2>
            <p className="mt-1 text-xs text-zinc-500">
              サーバーから短期アクセスを発行するための認証情報
            </p>
          </div>
          <div className="flex flex-wrap gap-2">
            <Button
              type="button"
              variant="secondary"
              size="icon"
              title="認証情報と短期アクセスを更新"
              aria-label="認証情報と短期アクセスを更新"
              onClick={() => void refreshAll()}
            >
              <RefreshCw />
            </Button>
            <Dialog open={createOpen} onOpenChange={handleCreateOpenChange}>
              <DialogTrigger asChild>
                <Button disabled={disabled}>
                  <Plus />
                  開発者認証情報を作成
                </Button>
              </DialogTrigger>
              <DialogContent className="max-w-2xl">
                <DialogHeader>
                  <DialogTitle>開発者認証情報を作成</DialogTitle>
                  <DialogDescription>
                    この認証情報で発行できる短期アクセスの権限上限を設定します。
                  </DialogDescription>
                </DialogHeader>
                <form className="space-y-5" onSubmit={submitCreate}>
                  <div className="space-y-2">
                    <Label htmlFor="developer-credential-name">名前</Label>
                    <Input
                      id="developer-credential-name"
                      value={name}
                      maxLength={120}
                      required
                      autoComplete="off"
                      placeholder="production-backend"
                      disabled={createCredential.isPending}
                      onChange={(event) => setName(event.target.value)}
                    />
                  </div>
                  <div className="space-y-2">
                    <Label htmlFor="developer-credential-expiry">
                      有効期間（日）
                    </Label>
                    <Input
                      id="developer-credential-expiry"
                      type="number"
                      min={1}
                      max={365}
                      required
                      value={expiresInDays}
                      disabled={createCredential.isPending}
                      onChange={(event) =>
                        setExpiresInDays(event.currentTarget.valueAsNumber)
                      }
                    />
                  </div>
                  <fieldset disabled={createCredential.isPending}>
                    <legend className="mb-2 text-sm font-medium text-zinc-800">
                      権限上限
                    </legend>
                    <div className="grid overflow-hidden border border-zinc-200 sm:grid-cols-2">
                      {realtimePermissions.map((permission) => (
                        <label
                          key={permission.value}
                          className="flex min-h-11 items-center gap-3 border-b border-zinc-200 px-3 py-2 text-sm last:border-b-0 sm:even:border-l"
                        >
                          <input
                            type="checkbox"
                            className="size-4 accent-emerald-700"
                            checked={permissions.includes(permission.value)}
                            onChange={(event) =>
                              setPermissions((current) =>
                                event.target.checked
                                  ? [...current, permission.value]
                                  : current.filter(
                                      (value) => value !== permission.value,
                                    ),
                              )
                            }
                          />
                          {permission.label}
                        </label>
                      ))}
                    </div>
                  </fieldset>
                  <FormError
                    message={
                      createCredential.isError
                        ? getApiErrorMessage(createCredential.error)
                        : null
                    }
                  />
                  <DialogFooter>
                    <Button
                      type="button"
                      variant="secondary"
                      onClick={() => setCreateOpen(false)}
                    >
                      キャンセル
                    </Button>
                    <Button
                      type="submit"
                      disabled={!validCreate || createCredential.isPending}
                    >
                      {createCredential.isPending ? (
                        <>
                          <LoaderCircle className="animate-spin" />
                          作成中
                        </>
                      ) : (
                        <>
                          <KeyRound />
                          作成
                        </>
                      )}
                    </Button>
                  </DialogFooter>
                </form>
              </DialogContent>
            </Dialog>
          </div>
        </div>

        {credentials.isPending ? (
          <LoadingRows label="開発者認証情報を読み込んでいます" />
        ) : credentials.isError ? (
          <InlineQueryError
            message="開発者認証情報を取得できませんでした。"
            onRetry={() => void credentials.refetch()}
          />
        ) : credentials.data.items.length === 0 ? (
          <div className="border border-zinc-200 bg-white">
            <EmptyState
              title="開発者認証情報がありません"
              description="バックエンド連携用の認証情報を作成してください。"
            />
          </div>
        ) : (
          <div className="border border-zinc-200 bg-white">
            <Table>
              <TableHeader>
                <TableRow className="hover:bg-transparent">
                  <TableHead>名前 / Prefix</TableHead>
                  <TableHead>権限上限</TableHead>
                  <TableHead>有効期限</TableHead>
                  <TableHead>最終使用</TableHead>
                  <TableHead>状態</TableHead>
                  <TableHead className="text-right">操作</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {credentials.data.items.map((credential) => {
                  const status = resourceStatus(
                    credential.expires_at,
                    credential.revoked_at,
                  );
                  const revoked = status === "revoked";
                  return (
                    <TableRow
                      key={credential.id}
                      className={revoked ? "bg-zinc-50 text-zinc-500" : undefined}
                    >
                      <TableCell>
                        <div className="font-medium text-zinc-900">
                          {credential.name}
                        </div>
                        <code className="mt-0.5 block break-all text-xs text-zinc-500">
                          {credential.prefix}
                        </code>
                      </TableCell>
                      <TableCell className="whitespace-normal">
                        <PermissionList permissions={credential.permissions} />
                      </TableCell>
                      <TableCell>{formatCredentialDate(credential.expires_at)}</TableCell>
                      <TableCell>
                        {credential.last_used_at
                          ? formatCredentialDate(credential.last_used_at)
                          : "未使用"}
                      </TableCell>
                      <TableCell>
                        <ResourceStatusBadge status={status} />
                      </TableCell>
                      <TableCell>
                        <div className="flex justify-end gap-1">
                          <Button
                            type="button"
                            variant="ghost"
                            size="icon"
                            title={`${credential.name}をローテーション`}
                            aria-label={`${credential.name}をローテーション`}
                            disabled={disabled || revoked}
                            onClick={() => {
                              rotateCredential.reset();
                              setRotateTarget(credential);
                            }}
                          >
                            <RotateCw />
                          </Button>
                          <Button
                            type="button"
                            variant="ghost"
                            size="icon"
                            className="text-red-700 hover:bg-red-50 hover:text-red-800"
                            title={`${credential.name}を失効`}
                            aria-label={`${credential.name}を失効`}
                            disabled={revoked}
                            onClick={() => {
                              revokeCredential.reset();
                              setRevokeCredentialTarget(credential);
                            }}
                          >
                            <Ban />
                          </Button>
                        </div>
                      </TableCell>
                    </TableRow>
                  );
                })}
              </TableBody>
            </Table>
          </div>
        )}
      </section>

      <section aria-labelledby="access-contexts-heading" className="space-y-3">
        <div>
          <h2
            id="access-contexts-heading"
            className="text-sm font-semibold text-zinc-950"
          >
            発行済み短期アクセス
          </h2>
          <p className="mt-1 text-xs text-zinc-500">
            このサービスで発行された利用者コンテキスト
          </p>
        </div>
        {contexts.isPending ? (
          <LoadingRows label="短期アクセスを読み込んでいます" />
        ) : contexts.isError ? (
          <InlineQueryError
            message="発行済み短期アクセスを取得できませんでした。"
            onRetry={() => void contexts.refetch()}
          />
        ) : contexts.data.items.length === 0 ? (
          <div className="border border-zinc-200 bg-white">
            <EmptyState
              title="発行済み短期アクセスがありません"
              description="バックエンドから発行された短期アクセスがここに表示されます。"
            />
          </div>
        ) : (
          <div className="border border-zinc-200 bg-white">
            <Table>
              <TableHeader>
                <TableRow className="hover:bg-transparent">
                  <TableHead>Subject</TableHead>
                  <TableHead>権限</TableHead>
                  <TableHead>発行日時</TableHead>
                  <TableHead>有効期限</TableHead>
                  <TableHead>状態</TableHead>
                  <TableHead className="text-right">操作</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {contexts.data.items.map((context) => {
                  const status = resourceStatus(
                    context.expires_at,
                    context.revoked_at,
                  );
                  return (
                    <TableRow
                      key={context.context_id}
                      className={
                        status === "revoked" ? "bg-zinc-50 text-zinc-500" : undefined
                      }
                    >
                      <TableCell>
                        <code className="block max-w-64 break-all text-xs">
                          {context.principal_id}
                        </code>
                      </TableCell>
                      <TableCell className="whitespace-normal">
                        <PermissionList permissions={context.permissions} />
                      </TableCell>
                      <TableCell>{formatCredentialDate(context.issued_at)}</TableCell>
                      <TableCell>{formatCredentialDate(context.expires_at)}</TableCell>
                      <TableCell>
                        <ResourceStatusBadge status={status} />
                      </TableCell>
                      <TableCell>
                        <div className="flex justify-end">
                          <Button
                            type="button"
                            variant="ghost"
                            size="icon"
                            className="text-red-700 hover:bg-red-50 hover:text-red-800"
                            title={`${context.principal_id}の短期アクセスを失効`}
                            aria-label={`${context.principal_id}の短期アクセスを失効`}
                            disabled={status !== "active"}
                            onClick={() => {
                              revokeContext.reset();
                              setRevokeContextTarget(context);
                            }}
                          >
                            <Ban />
                          </Button>
                        </div>
                      </TableCell>
                    </TableRow>
                  );
                })}
              </TableBody>
            </Table>
          </div>
        )}
      </section>

      <Dialog
        open={Boolean(secretResult)}
        onOpenChange={(nextOpen) => {
          if (!nextOpen) clearSecret();
        }}
      >
        <DialogContent className="max-w-3xl">
          <DialogHeader>
            <DialogTitle>
              {secretResult?.action === "rotated"
                ? "新しい認証情報を保存"
                : "開発者認証情報を保存"}
            </DialogTitle>
            <DialogDescription>
              秘密値は今回だけ表示されます。閉じると再表示できません。
            </DialogDescription>
          </DialogHeader>
          {secretResult ? (
            <div className="space-y-5">
              <section className="overflow-hidden border border-amber-300 bg-amber-50">
                <div className="flex items-center justify-between gap-3 border-b border-amber-200 px-4 py-3">
                  <h3 className="text-sm font-semibold text-amber-950">秘密値</h3>
                  <Button
                    type="button"
                    variant="secondary"
                    size="sm"
                    onClick={() =>
                      void copy("credential", secretResult.value.credential)
                    }
                  >
                    {copied === "credential" ? <Check /> : <Copy />}
                    {copied === "credential" ? "コピー済み" : "秘密値をコピー"}
                  </Button>
                </div>
                <code className="block break-all px-4 py-4 text-sm text-amber-950">
                  {secretResult.value.credential}
                </code>
              </section>

              <section className="overflow-hidden border border-zinc-200">
                <div className="flex items-center justify-between gap-3 border-b border-zinc-200 bg-zinc-50 px-4 py-3">
                  <h3 className="text-sm font-semibold text-zinc-950">
                    サーバー側の発行・失効例
                  </h3>
                  <Button
                    type="button"
                    variant="secondary"
                    size="sm"
                    onClick={() => void copy("curl", curl)}
                  >
                    {copied === "curl" ? <Check /> : <Copy />}
                    {copied === "curl" ? "コピー済み" : "curl例をコピー"}
                  </Button>
                </div>
                <pre className="max-h-72 overflow-auto whitespace-pre-wrap break-all bg-zinc-950 p-4 text-xs leading-5 text-zinc-100">
                  <code>{curl}</code>
                </pre>
              </section>

              <DialogFooter>
                <Button type="button" onClick={clearSecret}>
                  閉じる
                </Button>
              </DialogFooter>
            </div>
          ) : null}
        </DialogContent>
      </Dialog>

      <Dialog
        open={Boolean(rotateTarget)}
        onOpenChange={(nextOpen) => {
          if (!nextOpen) setRotateTarget(null);
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>認証情報をローテーション</DialogTitle>
            <DialogDescription>
              {rotateTarget?.name} の現在の秘密値と発行済み短期アクセスを失効し、
              新しい値を発行します。
            </DialogDescription>
          </DialogHeader>
          <FormError
            message={
              rotateCredential.isError
                ? getApiErrorMessage(rotateCredential.error)
                : null
            }
          />
          <DialogFooter>
            <Button
              type="button"
              variant="secondary"
              onClick={() => setRotateTarget(null)}
            >
              キャンセル
            </Button>
            <Button
              type="button"
              disabled={!rotateTarget || rotateCredential.isPending}
              onClick={() => {
                if (rotateTarget) rotateCredential.mutate(rotateTarget.id);
              }}
            >
              {rotateCredential.isPending ? (
                <>
                  <LoaderCircle className="animate-spin" />
                  ローテーション中
                </>
              ) : (
                <>
                  <RotateCw />
                  ローテーションする
                </>
              )}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog
        open={Boolean(revokeCredentialTarget)}
        onOpenChange={(nextOpen) => {
          if (!nextOpen) setRevokeCredentialTarget(null);
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>開発者認証情報を失効</DialogTitle>
            <DialogDescription>
              {revokeCredentialTarget?.name} と、その認証情報から発行済みの短期アクセスを
              直ちに使用できなくします。
            </DialogDescription>
          </DialogHeader>
          <FormError
            message={
              revokeCredential.isError
                ? getApiErrorMessage(revokeCredential.error)
                : null
            }
          />
          <DialogFooter>
            <Button
              type="button"
              variant="secondary"
              onClick={() => setRevokeCredentialTarget(null)}
            >
              キャンセル
            </Button>
            <Button
              type="button"
              variant="destructive"
              disabled={!revokeCredentialTarget || revokeCredential.isPending}
              onClick={() => {
                if (revokeCredentialTarget) {
                  revokeCredential.mutate(revokeCredentialTarget.id);
                }
              }}
            >
              {revokeCredential.isPending ? (
                <>
                  <LoaderCircle className="animate-spin" />
                  失効中
                </>
              ) : (
                <>
                  <Ban />
                  失効する
                </>
              )}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog
        open={Boolean(revokeContextTarget)}
        onOpenChange={(nextOpen) => {
          if (!nextOpen) setRevokeContextTarget(null);
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>短期アクセスを失効</DialogTitle>
            <DialogDescription>
              {revokeContextTarget?.principal_id} のアクセスを直ちに無効化します。
            </DialogDescription>
          </DialogHeader>
          <FormError
            message={
              revokeContext.isError
                ? getApiErrorMessage(revokeContext.error)
                : null
            }
          />
          <DialogFooter>
            <Button
              type="button"
              variant="secondary"
              onClick={() => setRevokeContextTarget(null)}
            >
              キャンセル
            </Button>
            <Button
              type="button"
              variant="destructive"
              disabled={!revokeContextTarget || revokeContext.isPending}
              onClick={() => {
                if (revokeContextTarget) {
                  revokeContext.mutate(revokeContextTarget.context_id);
                }
              }}
            >
              {revokeContext.isPending ? (
                <>
                  <LoaderCircle className="animate-spin" />
                  失効中
                </>
              ) : (
                <>
                  <Ban />
                  失効する
                </>
              )}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

function LoadingRows({ label }: { label: string }) {
  return (
    <div
      className="flex min-h-24 items-center justify-center gap-2 border border-zinc-200 bg-white text-sm text-zinc-500"
      role="status"
    >
      <LoaderCircle className="size-4 animate-spin" />
      {label}
    </div>
  );
}

function InlineQueryError({
  message,
  onRetry,
}: {
  message: string;
  onRetry: () => void;
}) {
  return (
    <div className="flex items-center justify-between gap-3 border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-950">
      <span>{message}</span>
      <Button type="button" variant="secondary" size="sm" onClick={onRetry}>
        <RefreshCw />
        再試行
      </Button>
    </div>
  );
}
