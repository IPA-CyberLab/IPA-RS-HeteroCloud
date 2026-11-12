import { Badge } from "@/components/ui";
import { Box } from "@/components/ui";
import { Button } from "@/components/ui";
import { ColumnLayout } from "@/components/ui";
import { FormField } from "@/components/ui";
import { Input } from "@/components/ui";
import { Modal } from "@/components/ui";
import { Select } from "@/components/ui";
import { SpaceBetween } from "@/components/ui";
import { TagInput } from "@/components/ui";
import { Textarea } from "@/components/ui";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { ColumnDef } from "@tanstack/react-table";
import { useMemo, useState } from "react";
import { DataTable } from "@/components/shared/data-table";
import { ErrorState } from "@/components/shared/error-state";
import { FormError } from "@/components/shared/form-error";
import { PageHeader } from "@/components/shared/page-header";
import { PageLoading } from "@/components/shared/page-loading";
import { useActiveOrganization } from "@/features/organizations/organization-context";
import { api, getApiErrorMessage } from "@/lib/api-client";
import type { IamPolicy, PolicyEffect } from "@/lib/api-types";
import { iamPoliciesQueryOptions } from "@/lib/queries";
import { formatDateTime } from "@/lib/utils";
import { policyTimeConditions } from "./policy-conditions";

const parseList = (value: string) =>
  value.split(/[\n,]/).map((item) => item.trim()).filter(Boolean);
const effectOptions = [
  { value: "Allow", label: "許可" },
  { value: "Deny", label: "明示的拒否" },
];

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
  const [conditionStart, setConditionStart] = useState("");
  const [conditionEnd, setConditionEnd] = useState("");
  const [mode, setMode] = useState<"visual" | "json">("visual");
  const [policyJson, setPolicyJson] = useState(JSON.stringify({ Version: "2012-10-17", Statement: [{ Effect: "Allow", Action: ["flash:ListInstances"], Resource: "*" }] }, null, 2));
  const [jsonError, setJsonError] = useState("");
  const createPolicy = useMutation({
    mutationFn: api.iam.policies.create.bind(api.iam.policies, organizationId),
    onSuccess: async () => {
      setOpen(false);
      setName("");
      setEffect("Allow");
      setActions("");
      setResources("");
      setConditionStart("");
      setConditionEnd("");
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
          <SpaceBetween size="xxs">
            <Box fontWeight="bold">{row.original.name}</Box>
            <Box variant="code">{row.original.id}</Box>
          </SpaceBetween>
        ),
      },
      {
        id: "effects",
        accessorFn: (policy) => policy.document.statements.map((item) => item.effect).join(" "),
        header: "効果",
        cell: ({ row }) => (
          <SpaceBetween direction="horizontal" size="xxs">
            {Array.from(new Set(row.original.document.statements.map((item) => item.effect))).map(
              (value) => (
                <Badge key={value} color={value === "Allow" ? "green" : "red"}>
                  {value === "Allow" ? "許可" : "明示的拒否"}
                </Badge>
              ),
            )}
          </SpaceBetween>
        ),
      },
      {
        id: "statements",
        accessorFn: (policy) => policy.document.statements.length,
        header: "ステートメント",
      },
      {
        accessorKey: "updated_at",
        header: "更新日時",
        cell: ({ getValue }) => formatDateTime(getValue<string>()),
      },
    ],
    [],
  );

  if (policies.isPending) return <PageLoading label="ポリシーを読み込んでいます" />;
  if (policies.isError) {
    return (
      <ErrorState
        description="IAMポリシー一覧を取得できませんでした。"
        onRetry={() => void policies.refetch()}
      />
    );
  }

  const valid = name.trim() && (mode === "json" ? policyJson.trim().length > 0 : parseList(actions).length > 0 && parseList(resources).length > 0);
  const submit = () => {
    if (!valid) return;
    if (mode === "json") {
      try {
        const document: unknown = JSON.parse(policyJson);
        if (typeof document !== "object" || document === null || Array.isArray(document)) throw new Error("JSONオブジェクトを指定してください。");
        setJsonError("");
        createPolicy.mutate({ name: name.trim(), document: document as Record<string, unknown> });
      } catch {
        setJsonError("有効なポリシーJSONを指定してください。");
      }
      return;
    }
    let conditions;
    try {
      conditions = policyTimeConditions(conditionStart, conditionEnd);
      setJsonError("");
    } catch (error) {
      setJsonError(error instanceof Error ? error.message : "有効な日時を指定してください。");
      return;
    }
    createPolicy.mutate({
      name: name.trim(),
      document: {
        version: "2026-07-31",
        statements: [{ effect, actions: parseList(actions), resources: parseList(resources), ...(conditions ? { conditions } : {}) }],
      },
    });
  };

  return (
    <SpaceBetween size="l">
      <PageHeader
        title="IAMポリシー"
        description={`${activeOrganization.organization_name} のアクションとリソースに対する権限を管理します。`}
        actions={
          <Button
            variant="primary"
            iconName="add-plus"
            onClick={() => {
              createPolicy.reset();
              setOpen(true);
            }}
          >
            ポリシーを作成
          </Button>
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
      <Modal
        visible={open}
        onDismiss={() => setOpen(false)}
        size="large"
        header="IAMポリシーを作成"
        footer={
          <Box float="right">
            <SpaceBetween direction="horizontal" size="xs">
              <Button onClick={() => setOpen(false)}>キャンセル</Button>
              <Button variant="primary" loading={createPolicy.isPending} disabled={!valid} onClick={submit}>
                作成
              </Button>
            </SpaceBetween>
          </Box>
        }
      >
        <SpaceBetween size="l">
          <Box color="text-body-secondary">
            デフォルトは拒否で、明示的な拒否が優先されます。
          </Box>
          <SpaceBetween direction="horizontal" size="xs"><Button variant={mode === "visual" ? "primary" : "normal"} onClick={() => setMode("visual")}>ビジュアル</Button>
            <Button variant={mode === "json" ? "primary" : "normal"} onClick={() => setMode("json")}>JSON</Button></SpaceBetween>
          {mode === "json" ? <SpaceBetween size="m"><FormField label="ポリシー名"><Input value={name} onChange={({ detail }) => setName(detail.value.slice(0, 120))} /></FormField>
            <FormField label="ポリシーJSON" description="Action、NotAction、Resource、NotResourceと、heterocloud:CurrentTime の日付条件に対応しています。その他の条件とPrincipalは使用できません。">
              <Textarea rows={14} value={policyJson} onChange={({ detail }) => setPolicyJson(detail.value)} />
            </FormField><FormError message={jsonError || null} /></SpaceBetween> : <>
          <ColumnLayout columns={2}>
            <FormField label="ポリシー名">
              <Input
                value={name}
                placeholder="FlowReadOnly"
                onChange={({ detail }) => setName(detail.value.slice(0, 120))}
              />
            </FormField>
            <FormField label="効果">
              <Select
                selectedOption={effectOptions.find((option) => option.value === effect) ?? effectOptions[0]}
                options={effectOptions}
                onChange={({ detail }) => setEffect(detail.selectedOption.value as PolicyEffect)}
              />
            </FormField>
          </ColumnLayout>
          <ColumnLayout columns={2}>
            <FormField label="アクション" description="許可または拒否する操作を追加してください。">
              <TagInput values={parseList(actions)} onChange={(items) => setActions(items.join("\n"))} ariaLabel="アクションを追加" placeholder="例: flow:ListInstances" />
            </FormField>
            <FormField label="リソース" description="対象を追加してください。* と ? を使用できます。">
              <TagInput values={parseList(resources)} onChange={(items) => setResources(items.join("\n"))} ariaLabel="リソースを追加" placeholder="例: hc:org:*:flow/*" />
            </FormField>
          </ColumnLayout>
          <ColumnLayout columns={2}>
            <FormField label="条件の開始日時（任意）" description="指定した日時以降にこのステートメントを適用します。端末のタイムゾーンで入力します。">
              <Input type="datetime-local" value={conditionStart} onChange={({ detail }) => setConditionStart(detail.value)} />
            </FormField>
            <FormField label="条件の終了日時（任意）" description="指定した日時より前にこのステートメントを適用します。">
              <Input type="datetime-local" value={conditionEnd} onChange={({ detail }) => setConditionEnd(detail.value)} />
            </FormField>
          </ColumnLayout>
          <FormError message={jsonError || null} />
          </>}
          <FormError message={createPolicy.isError ? getApiErrorMessage(createPolicy.error) : null} />
        </SpaceBetween>
      </Modal>
    </SpaceBetween>
  );
}
