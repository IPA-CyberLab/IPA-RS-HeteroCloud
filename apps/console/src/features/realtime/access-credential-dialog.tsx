import { Check, Copy, KeyRound, LoaderCircle } from "lucide-react";
import { useMutation } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { FormError } from "@/components/shared/form-error";
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
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { api, getApiErrorMessage } from "@/lib/api-client";
import type { RealtimeAccessCredential } from "@/lib/api-types";
import { RealtimeEndpoints } from "./realtime-endpoints";
import {
  formatCredentialDate,
  normalizeEndpoints,
  realtimePermissions,
} from "./realtime-service-utils";

interface AccessCredentialDialogProps {
  organizationId: string;
  serviceId: string;
  serviceName: string;
  disabled?: boolean;
}

export function AccessCredentialDialog({
  organizationId,
  serviceId,
  serviceName,
  disabled,
}: AccessCredentialDialogProps) {
  const [open, setOpen] = useState(false);
  const [expiresInSeconds, setExpiresInSeconds] = useState(300);
  const [permissions, setPermissions] = useState<string[]>(
    realtimePermissions.map((permission) => permission.value),
  );
  const [credential, setCredential] =
    useState<RealtimeAccessCredential | null>(null);
  const [copied, setCopied] = useState<string | null>(null);

  const issue = useMutation({
    mutationFn: () =>
      api.realtime.services.issueAccessCredential(
        organizationId,
        serviceId,
        {
          expires_in_seconds: expiresInSeconds,
          permissions,
        },
      ),
    onSuccess: setCredential,
  });

  const close = () => {
    setOpen(false);
    setCredential(null);
    setCopied(null);
    issue.reset();
  };

  const handleOpenChange = (nextOpen: boolean) => {
    if (nextOpen) {
      setOpen(true);
      return;
    }
    close();
  };

  const headerEntries = useMemo(
    () => Object.entries(credential?.headers ?? {}),
    [credential],
  );

  const copy = async (key: string, value: string) => {
    try {
      await navigator.clipboard.writeText(value);
      setCopied(key);
      window.setTimeout(() => setCopied(null), 1_500);
    } catch {
      setCopied(null);
    }
  };

  const copyAll = () =>
    copy(
      "all",
      headerEntries.map(([key, value]) => `${key}: ${value}`).join("\n"),
    );

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogTrigger asChild>
        <Button variant="secondary" disabled={disabled}>
          <KeyRound />
          テスト用短期アクセス
        </Button>
      </DialogTrigger>
      <DialogContent className="max-w-3xl">
        <DialogHeader>
          <DialogTitle>短期アクセスを手動発行</DialogTitle>
          <DialogDescription>
            {serviceName} の動作確認用です。開発者連携には開発者認証情報を使用します。
          </DialogDescription>
        </DialogHeader>

        {credential ? (
          <div className="space-y-5">
            <div className="border border-amber-300 bg-amber-50 px-4 py-3 text-sm text-amber-950">
              この秘密値は今回だけ表示されます。閉じると再表示できません。
            </div>

            <dl className="grid border border-zinc-200 bg-zinc-50 text-sm sm:grid-cols-3">
              <div className="border-b border-zinc-200 px-4 py-3 sm:border-b-0 sm:border-r">
                <dt className="text-xs text-zinc-500">発行日時</dt>
                <dd className="mt-1 font-medium">
                  {formatCredentialDate(credential.issued_at)}
                </dd>
              </div>
              <div className="border-b border-zinc-200 px-4 py-3 sm:border-b-0 sm:border-r">
                <dt className="text-xs text-zinc-500">有効期限</dt>
                <dd className="mt-1 font-medium">
                  {formatCredentialDate(credential.expires_at)}
                </dd>
              </div>
              <div className="px-4 py-3">
                <dt className="text-xs text-zinc-500">IPレート制限</dt>
                <dd className="mt-1 font-medium">
                  {credential.rate_limit.requests_per_second} RPS / burst{" "}
                  {credential.rate_limit.burst}
                </dd>
              </div>
            </dl>

            <section className="overflow-hidden border border-zinc-200">
              <div className="flex items-center justify-between border-b border-zinc-200 bg-zinc-50 px-4 py-3">
                <h3 className="text-sm font-semibold">リクエストヘッダー</h3>
                <Button
                  type="button"
                  variant="secondary"
                  size="sm"
                  onClick={() => void copyAll()}
                >
                  {copied === "all" ? <Check /> : <Copy />}
                  すべてコピー
                </Button>
              </div>
              <div className="divide-y divide-zinc-100">
                {headerEntries.map(([key, value]) => (
                  <div
                    key={key}
                    className="grid gap-2 px-4 py-3 sm:grid-cols-[11rem_minmax(0,1fr)_2.25rem] sm:items-center"
                  >
                    <code className="text-xs font-semibold text-zinc-600">
                      {key}
                    </code>
                    <code className="break-all text-xs text-zinc-900">
                      {value}
                    </code>
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      className="justify-self-end"
                      title={`${key}をコピー`}
                      aria-label={`${key}をコピー`}
                      onClick={() => void copy(key, value)}
                    >
                      {copied === key ? <Check /> : <Copy />}
                    </Button>
                  </div>
                ))}
              </div>
            </section>

            <section className="overflow-hidden border border-zinc-200">
              <div className="border-b border-zinc-200 bg-zinc-50 px-4 py-3">
                <h3 className="text-sm font-semibold">接続先</h3>
              </div>
              <RealtimeEndpoints
                endpoints={normalizeEndpoints(credential.endpoints)}
              />
            </section>

            <DialogFooter>
              <Button type="button" onClick={close}>
                閉じる
              </Button>
            </DialogFooter>
          </div>
        ) : (
          <div className="space-y-5">
            <div className="space-y-2">
              <Label>有効期間</Label>
              <Select
                value={String(expiresInSeconds)}
                onValueChange={(value) => setExpiresInSeconds(Number(value))}
                disabled={issue.isPending}
              >
                <SelectTrigger aria-label="有効期間">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="60">1分</SelectItem>
                  <SelectItem value="180">3分</SelectItem>
                  <SelectItem value="300">5分</SelectItem>
                </SelectContent>
              </Select>
            </div>

            <fieldset className="space-y-2" disabled={issue.isPending}>
              <legend className="text-sm font-medium text-zinc-800">権限</legend>
              <div className="grid border border-zinc-200 sm:grid-cols-2">
                {realtimePermissions.map((permission, index) => (
                  <label
                    key={permission.value}
                    className={`flex min-h-11 items-center gap-3 px-3 py-2 text-sm ${
                      index > 1 ? "border-t border-zinc-200" : ""
                    } ${index % 2 === 1 ? "sm:border-l" : ""}`}
                  >
                    <input
                      type="checkbox"
                      className="size-4 accent-emerald-700"
                      checked={permissions.includes(permission.value)}
                      onChange={(event) =>
                        setPermissions((current) =>
                          event.target.checked
                            ? [...current, permission.value]
                            : current.filter((value) => value !== permission.value),
                        )
                      }
                    />
                    {permission.label}
                  </label>
                ))}
              </div>
            </fieldset>

            <FormError
              message={issue.isError ? getApiErrorMessage(issue.error) : null}
            />
            <DialogFooter>
              <Button type="button" variant="secondary" onClick={close}>
                キャンセル
              </Button>
              <Button
                type="button"
                disabled={issue.isPending || permissions.length === 0}
                onClick={() => issue.mutate()}
              >
                {issue.isPending ? (
                  <>
                    <LoaderCircle className="animate-spin" />
                    発行中
                  </>
                ) : (
                  <>
                    <KeyRound />
                    発行
                  </>
                )}
              </Button>
            </DialogFooter>
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}
