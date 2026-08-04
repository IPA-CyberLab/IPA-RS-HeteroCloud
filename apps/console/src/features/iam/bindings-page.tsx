import Alert from "@cloudscape-design/components/alert";
import Button from "@cloudscape-design/components/button";
import Container from "@cloudscape-design/components/container";
import Form from "@cloudscape-design/components/form";
import FormField from "@cloudscape-design/components/form-field";
import Header from "@cloudscape-design/components/header";
import Select from "@cloudscape-design/components/select";
import SpaceBetween from "@cloudscape-design/components/space-between";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { ErrorState } from "@/components/shared/error-state";
import { PageHeader } from "@/components/shared/page-header";
import { PageLoading } from "@/components/shared/page-loading";
import { useActiveOrganization } from "@/features/organizations/organization-context";
import { api, getApiErrorMessage } from "@/lib/api-client";
import { iamPoliciesQueryOptions, iamPrincipalsQueryOptions } from "@/lib/queries";

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
    <SpaceBetween size="l">
      <PageHeader
        title="IAMバインディング"
        description={`${activeOrganization.organization_name} のプリンシパルへポリシーを割り当てます。`}
      />
      <Container
        header={
          <Header variant="h2" description="プリンシパルと最小権限ポリシーを関連付けます。">
            バインディングを作成
          </Header>
        }
      >
        <Form
          errorText={createBinding.isError ? getApiErrorMessage(createBinding.error) : undefined}
          actions={
            <Button
              variant="primary"
              iconName="anchor-link"
              loading={createBinding.isPending}
              disabled={!principalId || !policyId}
              onClick={() => createBinding.mutate()}
            >
              ポリシーを割り当て
            </Button>
          }
        >
          <SpaceBetween size="l">
            <FormField label="プリンシパル">
              <Select
                selectedOption={
                  principals.data.items
                    .map((principal) => ({
                      value: principal.id,
                      label: principal.name,
                      description: principal.kind === "user" ? "ユーザー" : "サービスアカウント",
                    }))
                    .find((option) => option.value === principalId) ?? null
                }
                options={principals.data.items.map((principal) => ({
                  value: principal.id,
                  label: principal.name,
                  description: principal.kind === "user" ? "ユーザー" : "サービスアカウント",
                }))}
                placeholder="プリンシパルを選択"
                onChange={({ detail }) => setPrincipalId(detail.selectedOption.value ?? "")}
              />
            </FormField>
            <FormField label="ポリシー">
              <Select
                selectedOption={
                  policies.data.items
                    .map((policy) => ({ value: policy.id, label: policy.name }))
                    .find((option) => option.value === policyId) ?? null
                }
                options={policies.data.items.map((policy) => ({ value: policy.id, label: policy.name }))}
                placeholder="ポリシーを選択"
                onChange={({ detail }) => setPolicyId(detail.selectedOption.value ?? "")}
              />
            </FormField>
            {createBinding.isSuccess ? (
              <Alert type="success" header="バインディングを作成しました">
                ID: {createBinding.data.id}
              </Alert>
            ) : null}
          </SpaceBetween>
        </Form>
      </Container>
    </SpaceBetween>
  );
}
