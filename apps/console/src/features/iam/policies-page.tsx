import Badge from "@cloudscape-design/components/badge";
import Box from "@cloudscape-design/components/box";
import Button from "@cloudscape-design/components/button";
import ColumnLayout from "@cloudscape-design/components/column-layout";
import FormField from "@cloudscape-design/components/form-field";
import Input from "@cloudscape-design/components/input";
import Modal from "@cloudscape-design/components/modal";
import Select from "@cloudscape-design/components/select";
import SpaceBetween from "@cloudscape-design/components/space-between";
import Textarea from "@cloudscape-design/components/textarea";
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
        accessorKey: "semantics_digest",
        header: "意味論digest",
        cell: ({ getValue }) => <Box variant="code">{getValue<string>().slice(0, 12)}…</Box>,
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

  const valid = name.trim() && parseList(actions).length > 0 && parseList(resources).length > 0;
  const submit = () => {
    if (!valid) return;
    createPolicy.mutate({
      name: name.trim(),
      document: {
        version: "2026-07-31",
        statements: [{ effect, actions: parseList(actions), resources: parseList(resources) }],
      },
    });
  };

  return (
    <SpaceBetween size="l">
      <PageHeader
        title="IAMポリシー"
        description={`${activeOrganization.organization_name} のLean検証済み認可意味論へ入力するルールを管理します。`}
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
            default-denyと明示的拒否優先のポリシー文書を作成します。
          </Box>
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
            <FormField label="アクション" description="改行またはカンマ区切り">
              <Textarea
                value={actions}
                placeholder={"flow:ListInstances\nflow:GetInstance"}
                onChange={({ detail }) => setActions(detail.value)}
              />
            </FormField>
            <FormField label="リソース" description="末尾wildcardのみ使用可能">
              <Textarea
                value={resources}
                placeholder="hc:org:*:flow/*"
                onChange={({ detail }) => setResources(detail.value)}
              />
            </FormField>
          </ColumnLayout>
          <FormError message={createPolicy.isError ? getApiErrorMessage(createPolicy.error) : null} />
        </SpaceBetween>
      </Modal>
    </SpaceBetween>
  );
}
