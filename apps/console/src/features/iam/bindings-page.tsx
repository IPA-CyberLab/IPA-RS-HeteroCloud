import { useMutation, useQuery } from "@tanstack/react-query";
import { CheckCircle2, Link2, LoaderCircle } from "lucide-react";
import { type FormEvent, useState } from "react";
import { ErrorState } from "@/components/shared/error-state";
import { FormError } from "@/components/shared/form-error";
import { PageHeader } from "@/components/shared/page-header";
import { PageLoading } from "@/components/shared/page-loading";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useActiveOrganization } from "@/features/organizations/organization-context";
import { api, getApiErrorMessage } from "@/lib/api-client";
import {
  iamPoliciesQueryOptions,
  iamPrincipalsQueryOptions,
} from "@/lib/queries";

export function IamBindingsPage() {
  const { activeOrganization } = useActiveOrganization();
  const organizationId = activeOrganization.organization_id;
  const principals = useQuery(iamPrincipalsQueryOptions(organizationId));
  const policies = useQuery(iamPoliciesQueryOptions(organizationId));
  const [principalId, setPrincipalId] = useState("");
  const [policyId, setPolicyId] = useState("");

  const createBinding = useMutation({
    mutationFn: () =>
      api.iam.bindings.create(organizationId, {
        principal_id: principalId,
        policy_id: policyId,
      }),
  });

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    createBinding.mutate();
  };

  if (principals.isPending || policies.isPending) {
    return <PageLoading label="IAMリソースを読み込んでいます" />;
  }

  if (principals.isError || policies.isError) {
    return (
      <ErrorState
        description="プリンシパルまたはポリシーを取得できませんでした。"
        onRetry={() => {
          void principals.refetch();
          void policies.refetch();
        }}
      />
    );
  }

  return (
    <div className="space-y-6">
      <PageHeader
        title="IAMバインディング"
        description={`${activeOrganization.organization_name} のプリンシパルへポリシーを割り当てます。`}
      />

      <form
        onSubmit={submit}
        className="max-w-2xl border border-zinc-200 bg-white"
      >
        <div className="border-b border-zinc-200 px-5 py-4">
          <div className="flex items-center gap-2">
            <Link2 className="size-4 text-zinc-500" />
            <h2 className="text-sm font-semibold">バインディングを作成</h2>
          </div>
        </div>
        <div className="space-y-5 p-5 sm:p-6">
          <div className="space-y-2">
            <Label>プリンシパル</Label>
            <Select value={principalId} onValueChange={setPrincipalId}>
              <SelectTrigger>
                <SelectValue placeholder="プリンシパルを選択" />
              </SelectTrigger>
              <SelectContent>
                {principals.data.items.map((principal) => (
                  <SelectItem key={principal.id} value={principal.id}>
                    {principal.name} (
                    {principal.kind === "user" ? "ユーザー" : "サービスアカウント"})
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="space-y-2">
            <Label>ポリシー</Label>
            <Select value={policyId} onValueChange={setPolicyId}>
              <SelectTrigger>
                <SelectValue placeholder="ポリシーを選択" />
              </SelectTrigger>
              <SelectContent>
                {policies.data.items.map((policy) => (
                  <SelectItem key={policy.id} value={policy.id}>
                    {policy.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <FormError
            message={
              createBinding.isError
                ? getApiErrorMessage(createBinding.error)
                : null
            }
          />
          {createBinding.isSuccess ? (
            <p
              className="flex items-center gap-2 text-sm text-emerald-700"
              role="status"
            >
              <CheckCircle2 className="size-4" />
              バインディングを作成しました。
              <span className="font-mono text-xs">{createBinding.data.id}</span>
            </p>
          ) : null}
        </div>
        <div className="flex justify-end border-t border-zinc-200 bg-zinc-50 px-5 py-4 sm:px-6">
          <Button
            type="submit"
            disabled={
              createBinding.isPending || !principalId || !policyId
            }
          >
            {createBinding.isPending ? (
              <>
                <LoaderCircle className="animate-spin" />
                作成中
              </>
            ) : (
              <>
                <Link2 />
                ポリシーを割り当て
              </>
            )}
          </Button>
        </div>
      </form>
    </div>
  );
}
