import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { ColumnDef } from "@tanstack/react-table";
import { KeyRound, LoaderCircle, Plus } from "lucide-react";
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { useActiveOrganization } from "@/features/organizations/organization-context";
import { api, getApiErrorMessage } from "@/lib/api-client";
import type { IamPolicy, PolicyEffect } from "@/lib/api-types";
import { iamPoliciesQueryOptions } from "@/lib/queries";
import { formatDateTime } from "@/lib/utils";

function parseList(value: string): string[] {
  return value
    .split(/[\n,]/)
    .map((item) => item.trim())
    .filter(Boolean);
}

export function IamPoliciesPage() {
  const { activeOrganization } = useActiveOrganization();
  const organizationId = activeOrganization.organization_id;
  const policies = useQuery(iamPoliciesQueryOptions(organizationId));
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [effect, setEffect] = useState<PolicyEffect>("Allow");
  const [actions, setActions] = useState("");
  const [resources, setResources] = useState("");

  const createPolicy = useMutation({
    mutationFn: api.iam.policies.create.bind(api.iam.policies, organizationId),
    onSuccess: async () => {
      setOpen(false);
      setName("");
      setEffect("Allow");
      setActions("");
      setResources("");
      await queryClient.invalidateQueries({
        queryKey: ["organizations", organizationId, "iam", "policies"],
      });
    },
  });

  const columns = useMemo<ColumnDef<IamPolicy, unknown>[]>(
    () => [
      {
        accessorKey: "name",
        header: "ポリシー",
        cell: ({ row }) => (
          <div className="flex items-center gap-3">
            <span className="flex size-8 items-center justify-center rounded-[5px] bg-zinc-100 text-zinc-600">
              <KeyRound className="size-4" />
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
        id: "effects",
        accessorFn: (policy) =>
          policy.document.statements.map((statement) => statement.effect).join(" "),
        header: "効果",
        cell: ({ row }) => (
          <div className="flex gap-1">
            {Array.from(
              new Set(
                row.original.document.statements.map(
                  (statement) => statement.effect,
                ),
              ),
            ).map((statementEffect) => (
              <Badge
                key={statementEffect}
                variant={statementEffect === "Allow" ? "success" : "danger"}
              >
                {statementEffect === "Allow" ? "許可" : "明示的拒否"}
              </Badge>
            ))}
          </div>
        ),
      },
      {
        id: "statements",
        accessorFn: (policy) => policy.document.statements.length,
        header: "ステートメント",
      },
      {
        accessorKey: "semantics_digest",
        header: "意味論digest",
        cell: ({ getValue }) => (
          <span className="font-mono text-xs" title={getValue<string>()}>
            {getValue<string>().slice(0, 12)}…
          </span>
        ),
      },
      {
        accessorKey: "updated_at",
        header: "更新日時",
        cell: ({ getValue }) => formatDateTime(getValue<string>()),
      },
    ],
    [],
  );

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    createPolicy.mutate({
      name: name.trim(),
      document: {
        version: "2026-07-31",
        statements: [
          {
            effect,
            actions: parseList(actions),
            resources: parseList(resources),
          },
        ],
      },
    });
  };

  if (policies.isPending) {
    return <PageLoading label="ポリシーを読み込んでいます" />;
  }

  if (policies.isError) {
    return (
      <ErrorState
        description="IAMポリシー一覧を取得できませんでした。"
        onRetry={() => void policies.refetch()}
      />
    );
  }

  return (
    <div className="space-y-6">
      <PageHeader
        title="IAMポリシー"
        description={`${activeOrganization.organization_name} のLean検証済み認可意味論へ入力するルールを管理します。`}
        actions={
          <Dialog
            open={open}
            onOpenChange={(nextOpen) => {
              setOpen(nextOpen);
              if (nextOpen) createPolicy.reset();
            }}
          >
            <DialogTrigger asChild>
              <Button>
                <Plus />
                ポリシーを作成
              </Button>
            </DialogTrigger>
            <DialogContent className="max-w-2xl">
              <DialogHeader>
                <DialogTitle>IAMポリシーを作成</DialogTitle>
                <DialogDescription>
                  default-denyと明示的拒否優先のポリシー文書を作成します。
                </DialogDescription>
              </DialogHeader>
              <form onSubmit={submit} className="space-y-5">
                <div className="grid gap-4 sm:grid-cols-2">
                  <div className="space-y-2">
                    <Label htmlFor="policy-name">ポリシー名</Label>
                    <Input
                      id="policy-name"
                      required
                      maxLength={120}
                      value={name}
                      onChange={(event) => setName(event.target.value)}
                      placeholder="FlowReadOnly"
                    />
                  </div>
                  <div className="space-y-2">
                    <Label>効果</Label>
                    <Select
                      value={effect}
                      onValueChange={(value) => setEffect(value as PolicyEffect)}
                    >
                      <SelectTrigger>
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="Allow">許可</SelectItem>
                        <SelectItem value="Deny">明示的拒否</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                </div>
                <div className="grid gap-4 sm:grid-cols-2">
                  <div className="space-y-2">
                    <Label htmlFor="policy-actions">アクション</Label>
                    <Textarea
                      id="policy-actions"
                      required
                      value={actions}
                      onChange={(event) => setActions(event.target.value)}
                      placeholder={"flow:ListInstances\nflow:GetInstance"}
                    />
                    <p className="text-xs text-zinc-500">改行またはカンマ区切り</p>
                  </div>
                  <div className="space-y-2">
                    <Label htmlFor="policy-resources">リソース</Label>
                    <Textarea
                      id="policy-resources"
                      required
                      value={resources}
                      onChange={(event) => setResources(event.target.value)}
                      placeholder="hc:org:*:flow/*"
                    />
                    <p className="text-xs text-zinc-500">末尾wildcardのみ使用可能</p>
                  </div>
                </div>
                <FormError
                  message={
                    createPolicy.isError
                      ? getApiErrorMessage(createPolicy.error)
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
                    disabled={
                      createPolicy.isPending ||
                      parseList(actions).length === 0 ||
                      parseList(resources).length === 0
                    }
                  >
                    {createPolicy.isPending ? (
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
        }
      />

      <DataTable
        columns={columns}
        data={policies.data.items}
        getRowId={(policy) => policy.id}
        searchPlaceholder="名前、効果、ポリシーIDで検索"
        emptyTitle="IAMポリシーがありません"
        emptyDescription="最小権限のポリシーを作成してください。"
      />
    </div>
  );
}
