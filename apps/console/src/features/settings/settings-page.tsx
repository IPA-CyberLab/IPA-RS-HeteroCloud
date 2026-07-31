import { KeyRound, ShieldCheck, UserRound } from "lucide-react";
import { DataTable } from "@/components/shared/data-table";
import { PageHeader } from "@/components/shared/page-header";
import { Badge } from "@/components/ui/badge";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useSession } from "@/features/auth/session";
import { useActiveOrganization } from "@/features/organizations/organization-context";
import type { Membership } from "@/lib/api-types";
import { formatDateTime } from "@/lib/utils";

export function SettingsPage() {
  const session = useSession().data!;
  const {
    activeOrganization,
    memberships,
    setActiveOrganizationId,
  } = useActiveOrganization();

  return (
    <div className="space-y-6">
      <PageHeader
        title="設定"
        description="アカウント、セッション、コンソールの操作対象を確認します。"
      />

      <div className="grid gap-6 xl:grid-cols-2">
        <section className="border border-zinc-200 bg-white">
          <div className="flex items-center gap-2 border-b border-zinc-200 px-5 py-4">
            <UserRound className="size-4 text-zinc-500" />
            <h2 className="text-sm font-semibold">アカウント</h2>
          </div>
          <dl className="divide-y divide-zinc-100 px-5 text-sm">
            <div className="grid gap-1 py-3 sm:grid-cols-[10rem_1fr]">
              <dt className="text-zinc-500">表示名</dt>
              <dd className="font-medium">{session.user.display_name}</dd>
            </div>
            <div className="grid gap-1 py-3 sm:grid-cols-[10rem_1fr]">
              <dt className="text-zinc-500">メールアドレス</dt>
              <dd>{session.user.email}</dd>
            </div>
            <div className="grid gap-1 py-3 sm:grid-cols-[10rem_1fr]">
              <dt className="text-zinc-500">ユーザーID</dt>
              <dd className="break-all font-mono text-xs">{session.user.id}</dd>
            </div>
            <div className="grid gap-1 py-3 sm:grid-cols-[10rem_1fr]">
              <dt className="text-zinc-500">状態</dt>
              <dd>
                <Badge
                  variant={
                    session.user.status === "active" ? "success" : "warning"
                  }
                >
                  {session.user.status === "active" ? "有効" : "停止中"}
                </Badge>
              </dd>
            </div>
            <div className="grid gap-1 py-3 sm:grid-cols-[10rem_1fr]">
              <dt className="text-zinc-500">登録日時</dt>
              <dd>{formatDateTime(session.user.created_at)}</dd>
            </div>
          </dl>
        </section>

        <section className="border border-zinc-200 bg-white">
          <div className="flex items-center gap-2 border-b border-zinc-200 px-5 py-4">
            <ShieldCheck className="size-4 text-zinc-500" />
            <h2 className="text-sm font-semibold">セッション</h2>
          </div>
          <div className="space-y-5 p-5">
            <div className="flex items-start justify-between gap-4">
              <div>
                <p className="text-sm font-medium">HttpOnly Cookie</p>
                <p className="mt-1 text-xs leading-5 text-zinc-500">
                  認証情報はブラウザスクリプトから参照できないCookieで保持されます。
                </p>
              </div>
              <Badge variant="success">有効</Badge>
            </div>
            <div className="flex items-start justify-between gap-4 border-t border-zinc-100 pt-5">
              <div>
                <p className="text-sm font-medium">Origin + CSRF検証</p>
                <p className="mt-1 text-xs leading-5 text-zinc-500">
                  認証済みmutationは同一originとセッショントークン由来CSRFで保護されます。
                </p>
              </div>
              <KeyRound className="mt-0.5 size-4 text-emerald-700" />
            </div>
          </div>
        </section>
      </div>

      <section className="border border-zinc-200 bg-white">
        <div className="border-b border-zinc-200 px-5 py-4">
          <h2 className="text-sm font-semibold">操作対象の組織</h2>
          <p className="mt-1 text-xs text-zinc-500">
            コンソール内の組織スコープAPIに使用します。
          </p>
        </div>
        <div className="max-w-md p-5">
          <Select
            value={activeOrganization.organization_id}
            onValueChange={setActiveOrganizationId}
          >
            <SelectTrigger aria-label="操作対象の組織">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {memberships.map((membership) => (
                <SelectItem
                  key={membership.organization_id}
                  value={membership.organization_id}
                >
                  {membership.organization_name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </section>

      <DataTable<Membership>
        columns={[
          { accessorKey: "organization_name", header: "組織" },
          { accessorKey: "organization_slug", header: "slug" },
          {
            accessorKey: "role",
            header: "membership",
            cell: ({ getValue }) =>
              getValue<Membership["role"]>() === "owner"
                ? "オーナー"
                : "メンバー",
          },
          {
            accessorKey: "principal_id",
            header: "プリンシパルID",
            cell: ({ getValue }) => (
              <span className="font-mono text-xs">{getValue<string>()}</span>
            ),
          },
        ]}
        data={memberships}
        getRowId={(membership) => membership.organization_id}
        searchPlaceholder="組織名、slug、roleで検索"
        emptyTitle="membershipがありません"
        emptyDescription="このアカウントは組織に所属していません。"
      />
    </div>
  );
}
