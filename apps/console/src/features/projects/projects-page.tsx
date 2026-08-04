import Box from "@cloudscape-design/components/box";
import Button from "@cloudscape-design/components/button";
import FormField from "@cloudscape-design/components/form-field";
import Input from "@cloudscape-design/components/input";
import Modal from "@cloudscape-design/components/modal";
import SpaceBetween from "@cloudscape-design/components/space-between";
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
import type { Project } from "@/lib/api-types";
import { projectsQueryOptions } from "@/lib/queries";
import { formatDateTime } from "@/lib/utils";

const slugPattern = /^[a-z][a-z0-9-]{1,61}[a-z0-9]$/;

export function ProjectsPage() {
  const { activeOrganization } = useActiveOrganization();
  const organizationId = activeOrganization.organization_id;
  const projects = useQuery(projectsQueryOptions(organizationId));
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [slug, setSlug] = useState("");

  const createProject = useMutation({
    mutationFn: (input: { name: string; slug: string }) =>
      api.projects.create(organizationId, input),
    onSuccess: async () => {
      setOpen(false);
      setName("");
      setSlug("");
      await queryClient.invalidateQueries({
        queryKey: ["organizations", organizationId, "projects"],
      });
    },
  });

  const columns = useMemo<ColumnDef<Project, unknown>[]>(
    () => [
      {
        accessorKey: "name",
        header: "プロジェクト",
        cell: ({ row }) => (
          <SpaceBetween size="xxs">
            <Box fontWeight="bold">{row.original.name}</Box>
            <Box color="text-body-secondary">{row.original.slug}</Box>
          </SpaceBetween>
        ),
      },
      {
        accessorKey: "id",
        header: "プロジェクトID",
        cell: ({ getValue }) => <Box variant="code">{getValue<string>()}</Box>,
      },
      {
        accessorKey: "created_at",
        header: "作成日時",
        cell: ({ getValue }) => formatDateTime(getValue<string>()),
      },
    ],
    [],
  );

  if (projects.isPending) return <PageLoading label="プロジェクトを読み込んでいます" />;
  if (projects.isError) {
    return (
      <ErrorState
        description="選択中の組織からプロジェクト一覧を取得できませんでした。"
        onRetry={() => void projects.refetch()}
      />
    );
  }

  const valid = name.trim().length > 0 && slugPattern.test(slug);
  const submit = () => {
    if (valid && !createProject.isPending) {
      createProject.mutate({ name: name.trim(), slug });
    }
  };

  return (
    <SpaceBetween size="l">
      <PageHeader
        title="プロジェクト"
        description={`${activeOrganization.organization_name} のサービスリソース境界を管理します。`}
        actions={
          <Button
            variant="primary"
            iconName="add-plus"
            onClick={() => {
              createProject.reset();
              setOpen(true);
            }}
          >
            プロジェクトを作成
          </Button>
        }
      />
      <DataTable
        columns={columns}
        data={projects.data.items}
        getRowId={(project) => project.id}
        searchPlaceholder="プロジェクト名、slug、IDで検索"
        emptyTitle="プロジェクトがありません"
        emptyDescription="最初のプロジェクトを作成してください。"
      />
      <Modal
        visible={open}
        onDismiss={() => setOpen(false)}
        header="プロジェクトを作成"
        footer={
          <Box float="right">
            <SpaceBetween direction="horizontal" size="xs">
              <Button onClick={() => setOpen(false)}>キャンセル</Button>
              <Button
                variant="primary"
                loading={createProject.isPending}
                disabled={!valid}
                onClick={submit}
              >
                作成
              </Button>
            </SpaceBetween>
          </Box>
        }
      >
        <SpaceBetween size="l">
          <Box color="text-body-secondary">
            {activeOrganization.organization_name} に新しいプロジェクトを作成します。
          </Box>
          <FormField label="プロジェクト名">
            <Input
              value={name}
              placeholder="Realtime Production"
              onChange={({ detail }) => setName(detail.value.slice(0, 120))}
            />
          </FormField>
          <FormField
            label="プロジェクトslug"
            description="3〜63文字の英小文字、数字、ハイフンを使用します。"
            errorText={slug && !slugPattern.test(slug) ? "slugの形式が正しくありません。" : undefined}
          >
            <Input
              value={slug}
              placeholder="realtime-prod"
              onChange={({ detail }) =>
                setSlug(detail.value.toLowerCase().replace(/\s+/g, "-").slice(0, 63))
              }
            />
          </FormField>
          <FormError
            message={createProject.isError ? getApiErrorMessage(createProject.error) : null}
          />
        </SpaceBetween>
      </Modal>
    </SpaceBetween>
  );
}
