import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { ColumnDef } from "@tanstack/react-table";
import {
  Bot,
  Check,
  Copy,
  KeyRound,
  LoaderCircle,
  Plus,
  UserRound,
} from "lucide-react";
import { type FormEvent, useMemo, useState } from "react";
import { DataTable } from "@/components/shared/data-table";
import { ErrorState } from "@/components/shared/error-state";
import { FormError } from "@/components/shared/form-error";
import { PageHeader } from "@/components/shared/page-header";
import { PageLoading } from "@/components/shared/page-loading";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useActiveOrganization } from "@/features/organizations/organization-context";
import { api, getApiErrorMessage } from "@/lib/api-client";
import type { InvitationResponse, Principal } from "@/lib/api-types";
import { iamPrincipalsQueryOptions } from "@/lib/queries";
import { formatDateTime } from "@/lib/utils";

export function IamPrincipalsPage() {
  const { activeOrganization } = useActiveOrganization();
  const organizationId = activeOrganization.organization_id;
  const principals = useQuery(iamPrincipalsQueryOptions(organizationId));
  const queryClient = useQueryClient();
  const [serviceAccountOpen, setServiceAccountOpen] = useState(false);
  const [invitationOpen, setInvitationOpen] = useState(false);
  const [serviceAccountName, setServiceAccountName] = useState("");
  const [expiresInHours, setExpiresInHours] = useState(24);

  const createServiceAccount = useMutation({
    mutationFn: (name: string) =>
      api.iam.principals.createServiceAccount(organizationId, { name }),
    onSuccess: async () => {
      setServiceAccountOpen(false);
      setServiceAccountName("");
      await queryClient.invalidateQueries({
        queryKey: ["organizations", organizationId, "iam", "principals"],
      });
    },
  });

  const createInvitation = useMutation({
    mutationFn: () =>
      api.invitations.create(organizationId, {
        expires_in_hours: expiresInHours,
      }),
  });

  const columns = useMemo<ColumnDef<Principal, unknown>[]>(
    () => [
      {
        accessorKey: "name",
        header: "プリンシパル",
        cell: ({ row }) => (
          <div className="flex items-center gap-3">
            <span className="flex size-8 items-center justify-center rounded-full bg-zinc-100 text-zinc-600">
              {row.original.kind === "user" ? (
                <UserRound className="size-4" />
              ) : (
                <Bot className="size-4" />
              )}
            </span>
            <div>
              <div className="font-medium text-zinc-900">{row.original.name}</div>
              <div className="font-mono text-xs text-zinc-500">
                {row.original.id}
              </div>
            </div>
          </div>
        ),
      },
      {
        accessorKey: "kind",
        header: "種別",
        cell: ({ getValue }) => {
          const kind = getValue<Principal["kind"]>();
          return (
            <Badge variant={kind === "user" ? "info" : "neutral"}>
              {kind === "user" ? "ユーザー" : "サービスアカウント"}
            </Badge>
          );
        },
      },
      {
        accessorKey: "enabled",
        header: "状態",
        cell: ({ getValue }) => (
          <Badge variant={getValue<boolean>() ? "success" : "neutral"}>
            {getValue<boolean>() ? "有効" : "無効"}
          </Badge>
        ),
      },
      {
        accessorKey: "user_id",
        header: "ユーザーID",
        cell: ({ getValue }) => (
          <span className="font-mono text-xs">
            {getValue<string | null>() ?? "—"}
          </span>
        ),
      },
      {
        accessorKey: "created_at",
        header: "作成日時",
        cell: ({ getValue }) => formatDateTime(getValue<string>()),
      },
    ],
    [],
  );

  const submitServiceAccount = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    createServiceAccount.mutate(serviceAccountName.trim());
  };

  const resetInvitation = () => {
    createInvitation.reset();
    setExpiresInHours(24);
  };

  if (principals.isPending) {
    return <PageLoading label="プリンシパルを読み込んでいます" />;
  }

  if (principals.isError) {
    return (
      <ErrorState
        description="IAMプリンシパル一覧を取得できませんでした。"
        onRetry={() => void principals.refetch()}
      />
    );
  }

  return (
    <div className="space-y-6">
      <PageHeader
        title="IAMプリンシパル"
        description={`${activeOrganization.organization_name} のユーザーおよびサービスアカウントを管理します。`}
        actions={
          <div className="flex flex-wrap gap-2">
            {activeOrganization.role === "owner" ? (
              <InvitationDialog
                open={invitationOpen}
                onOpenChange={(nextOpen) => {
                  setInvitationOpen(nextOpen);
                  if (!nextOpen) resetInvitation();
                }}
                expiresInHours={expiresInHours}
                setExpiresInHours={setExpiresInHours}
                invitation={createInvitation.data}
                pending={createInvitation.isPending}
                error={
                  createInvitation.isError
                    ? getApiErrorMessage(createInvitation.error)
                    : null
                }
                onCreate={() => createInvitation.mutate()}
              />
            ) : null}

            <Dialog
              open={serviceAccountOpen}
              onOpenChange={(nextOpen) => {
                setServiceAccountOpen(nextOpen);
                if (nextOpen) createServiceAccount.reset();
              }}
            >
              <DialogTrigger asChild>
                <Button>
                  <Plus />
                  サービスアカウントを作成
                </Button>
              </DialogTrigger>
              <DialogContent>
                <DialogHeader>
                  <DialogTitle>サービスアカウントを作成</DialogTitle>
                  <DialogDescription>
                    自動化処理へIAMポリシーを割り当てるプリンシパルを作成します。
                  </DialogDescription>
                </DialogHeader>
                <form onSubmit={submitServiceAccount} className="space-y-5">
                  <div className="space-y-2">
                    <Label htmlFor="service-account-name">名前</Label>
                    <Input
                      id="service-account-name"
                      required
                      maxLength={120}
                      value={serviceAccountName}
                      onChange={(event) =>
                        setServiceAccountName(event.target.value)
                      }
                      placeholder="flow-deployer"
                    />
                  </div>
                  <FormError
                    message={
                      createServiceAccount.isError
                        ? getApiErrorMessage(createServiceAccount.error)
                        : null
                    }
                  />
                  <DialogFooter>
                    <DialogClose asChild>
                      <Button type="button" variant="secondary">
                        キャンセル
                      </Button>
                    </DialogClose>
                    <Button
                      type="submit"
                      disabled={createServiceAccount.isPending}
                    >
                      {createServiceAccount.isPending ? (
                        <>
                          <LoaderCircle className="animate-spin" />
                          作成中
                        </>
                      ) : (
                        "作成"
                      )}
                    </Button>
                  </DialogFooter>
                </form>
              </DialogContent>
            </Dialog>
          </div>
        }
      />

      <DataTable
        columns={columns}
        data={principals.data.items}
        getRowId={(principal) => principal.id}
        searchPlaceholder="名前、種別、プリンシパルIDで検索"
        emptyTitle="プリンシパルがありません"
        emptyDescription="サービスアカウントを作成するか、ユーザーを招待してください。"
      />
    </div>
  );
}

interface InvitationDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  expiresInHours: number;
  setExpiresInHours: (value: number) => void;
  invitation?: InvitationResponse;
  pending: boolean;
  error: string | null;
  onCreate: () => void;
}

function InvitationDialog({
  open,
  onOpenChange,
  expiresInHours,
  setExpiresInHours,
  invitation,
  pending,
  error,
  onCreate,
}: InvitationDialogProps) {
  const [copied, setCopied] = useState<"code" | "url" | null>(null);
  const registrationUrl = invitation
    ? `${window.location.origin}/register#invitation_code=${encodeURIComponent(invitation.code)}`
    : "";

  const copy = async (value: string, kind: "code" | "url") => {
    await navigator.clipboard.writeText(value);
    setCopied(kind);
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => {
        setCopied(null);
        onOpenChange(nextOpen);
      }}
    >
      <DialogTrigger asChild>
        <Button variant="secondary">
          <KeyRound />
          招待コードを発行
        </Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>組織への招待</DialogTitle>
          <DialogDescription>
            1回だけ利用できる有効期限付き招待コードを発行します。
          </DialogDescription>
        </DialogHeader>

        {invitation ? (
          <div className="space-y-5">
            <div className="border border-amber-200 bg-amber-50 px-3 py-2 text-sm text-amber-900">
              このコードは閉じると再表示できません。
            </div>
            <div className="space-y-2">
              <Label>招待コード</Label>
              <div className="flex gap-2">
                <Input readOnly value={invitation.code} className="font-mono" />
                <Button
                  type="button"
                  size="icon"
                  variant="secondary"
                  title="招待コードをコピー"
                  aria-label="招待コードをコピー"
                  onClick={() => void copy(invitation.code, "code")}
                >
                  {copied === "code" ? <Check /> : <Copy />}
                </Button>
              </div>
            </div>
            <div className="space-y-2">
              <Label>登録URL</Label>
              <div className="flex gap-2">
                <Input readOnly value={registrationUrl} />
                <Button
                  type="button"
                  size="icon"
                  variant="secondary"
                  title="登録URLをコピー"
                  aria-label="登録URLをコピー"
                  onClick={() => void copy(registrationUrl, "url")}
                >
                  {copied === "url" ? <Check /> : <Copy />}
                </Button>
              </div>
            </div>
            <dl className="grid grid-cols-2 gap-3 text-sm">
              <div>
                <dt className="text-zinc-500">最大利用回数</dt>
                <dd className="mt-1 font-medium">{invitation.max_uses}</dd>
              </div>
              <div>
                <dt className="text-zinc-500">有効期限</dt>
                <dd className="mt-1 font-medium">
                  {formatDateTime(invitation.expires_at)}
                </dd>
              </div>
            </dl>
            <DialogFooter>
              <DialogClose asChild>
                <Button>完了</Button>
              </DialogClose>
            </DialogFooter>
          </div>
        ) : (
          <form
            onSubmit={(event) => {
              event.preventDefault();
              onCreate();
            }}
            className="space-y-5"
          >
            <div className="space-y-2">
              <Label htmlFor="invitation-expiry">有効時間</Label>
              <Input
                id="invitation-expiry"
                type="number"
                required
                min={1}
                max={168}
                value={expiresInHours}
                onChange={(event) =>
                  setExpiresInHours(event.currentTarget.valueAsNumber || 1)
                }
              />
              <p className="text-xs text-zinc-500">
                1〜168時間。登録が完了すると直ちに無効になります。
              </p>
            </div>
            <FormError message={error} />
            <DialogFooter>
              <DialogClose asChild>
                <Button type="button" variant="secondary">
                  キャンセル
                </Button>
              </DialogClose>
              <Button type="submit" disabled={pending}>
                {pending ? (
                  <>
                    <LoaderCircle className="animate-spin" />
                    発行中
                  </>
                ) : (
                  "発行"
                )}
              </Button>
            </DialogFooter>
          </form>
        )}
      </DialogContent>
    </Dialog>
  );
}
