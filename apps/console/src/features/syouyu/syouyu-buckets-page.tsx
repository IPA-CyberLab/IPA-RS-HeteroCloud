import Box from "@cloudscape-design/components/box";
import Button from "@cloudscape-design/components/button";
import ColumnLayout from "@cloudscape-design/components/column-layout";
import Container from "@cloudscape-design/components/container";
import Modal from "@cloudscape-design/components/modal";
import SpaceBetween from "@cloudscape-design/components/space-between";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { ColumnDef } from "@tanstack/react-table";
import { type FormEvent, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { DataTable } from "@/components/shared/data-table";
import { ErrorState } from "@/components/shared/error-state";
import { FormError } from "@/components/shared/form-error";
import { PageHeader } from "@/components/shared/page-header";
import { PageLoading } from "@/components/shared/page-loading";
import { StatusBadge } from "@/components/shared/status-badge";
import { useActiveOrganization } from "@/features/organizations/organization-context";
import { api, getApiErrorMessage } from "@/lib/api-client";
import type { SyouyuBucket } from "@/lib/api-types";
import {
  projectsQueryOptions,
  syouyuBucketsQueryOptions,
  syouyuQuotaQueryOptions,
} from "@/lib/queries";
import { formatDateTime, formatNumber } from "@/lib/utils";
import { SyouyuBucketForm } from "./syouyu-bucket-form";
import {
  bucketFormError,
  bucketSpecFromForm,
  defaultBucketForm,
  defaultSyouyuBucketFormValue,
  formatBytes,
  GIBIBYTE,
  type SyouyuBucketFormValue,
} from "./syouyu-utils";

export function SyouyuBucketsPage() {
  const { activeOrganization } = useActiveOrganization();
  const organizationId = activeOrganization.organization_id;
  const buckets = useQuery(syouyuBucketsQueryOptions(organizationId));
  const quota = useQuery(syouyuQuotaQueryOptions(organizationId));
  const projects = useQuery(projectsQueryOptions(organizationId));
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [createOpen, setCreateOpen] = useState(false);
  const [form, setForm] = useState<SyouyuBucketFormValue>(
    defaultSyouyuBucketFormValue,
  );

  const createBucket = useMutation({
    mutationFn: (value: SyouyuBucketFormValue) =>
      api.syouyu.buckets.create(organizationId, {
        project_id: value.projectId,
        name: value.bucketName.trim(),
        spec: bucketSpecFromForm(value),
      }),
    onSuccess: async (created) => {
      setCreateOpen(false);
      await queryClient.invalidateQueries({
        queryKey: ["organizations", organizationId, "syouyu", "buckets"],
      });
      navigate(`/syouyu/buckets/${created.id}`);
    },
  });
  const projectNames = useMemo(
    () =>
      new Map(
        (projects.data?.items ?? []).map((project) => [project.id, project.name]),
      ),
    [projects.data],
  );
  const columns = useMemo<ColumnDef<SyouyuBucket, unknown>[]>(
    () => [
      {
        id: "bucket",
        accessorFn: (bucket) => bucket.spec.bucket_name,
        header: "バケット",
        cell: ({ row }) => (
          <SpaceBetween size="xxs">
            <Box fontWeight="bold">{row.original.spec.bucket_name}</Box>
            <Box variant="code" className="mobile-hidden">
              {row.original.id}
            </Box>
          </SpaceBetween>
        ),
      },
      {
        id: "project",
        accessorFn: (bucket) =>
          projectNames.get(bucket.project_id) ?? bucket.project_id,
        header: "プロジェクト",
        cell: ({ row }) =>
          projectNames.get(row.original.project_id) ?? (
            <Box variant="code">{row.original.project_id}</Box>
          ),
      },
      {
        accessorKey: "state",
        header: "状態",
        cell: ({ getValue }) => (
          <StatusBadge status={getValue<SyouyuBucket["state"]>()} />
        ),
      },
      {
        id: "region",
        accessorFn: (bucket) => bucket.spec.region,
        header: "リージョン",
      },
      {
        id: "quota",
        accessorFn: (bucket) => bucket.spec.quota_bytes,
        header: "容量上限",
        cell: ({ row }) => formatBytes(row.original.spec.quota_bytes),
      },
      {
        id: "objects",
        accessorFn: (bucket) => bucket.status.objects ?? 0,
        header: "オブジェクト",
        cell: ({ row }) =>
          row.original.status.objects === undefined
            ? "-"
            : formatNumber(row.original.status.objects),
      },
      {
        id: "endpoint",
        accessorFn: (bucket) => bucket.status.endpoint ?? "",
        header: "エンドポイント",
        cell: ({ row }) =>
          row.original.status.endpoint ? (
            <Box variant="code">{row.original.status.endpoint}</Box>
          ) : (
            "-"
          ),
      },
      {
        accessorKey: "updated_at",
        header: "更新日時",
        cell: ({ getValue }) => formatDateTime(getValue<string>()),
      },
    ],
    [projectNames],
  );

  if (buckets.isPending || quota.isPending || projects.isPending) {
    return <PageLoading label="Syouyuバケットを読み込んでいます" />;
  }
  if (buckets.isError || quota.isError || projects.isError) {
    return (
      <ErrorState
        title="Syouyuを取得できませんでした"
        description={getApiErrorMessage(
          buckets.error ?? quota.error ?? projects.error,
        )}
        onRetry={() => {
          void buckets.refetch();
          void quota.refetch();
          void projects.refetch();
        }}
      />
    );
  }

  const items = buckets.data.items;
  const readyCount = items.filter((bucket) => bucket.state === "ready").length;
  const allocatedBytes = items.reduce(
    (sum, bucket) => sum + bucket.spec.quota_bytes,
    0,
  );
  const validationError = bucketFormError(form, quota.data);
  const atBucketLimit = items.length >= quota.data.max_buckets;
  const availableBytes = quota.data.max_total_bytes - allocatedBytes;
  const atTotalCapacityLimit = availableBytes < GIBIBYTE;

  const openCreator = () => {
    setForm(defaultBucketForm(quota.data, availableBytes));
    createBucket.reset();
    setCreateOpen(true);
  };
  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!validationError) createBucket.mutate(form);
  };

  return (
    <SpaceBetween size="l">
      <PageHeader
        title="Syouyu"
        description={`${activeOrganization.organization_name} のS3互換オブジェクトストレージを管理します。`}
        actions={
          <SpaceBetween direction="horizontal" size="xs">
            <Button
              variant="icon"
              iconName="refresh"
              ariaLabel="Syouyuバケットを更新"
              onClick={() =>
                void queryClient.invalidateQueries({
                  queryKey: ["organizations", organizationId, "syouyu"],
                })
              }
            />
            <Button
              variant="primary"
              iconName="add-plus"
              disabled={atBucketLimit || atTotalCapacityLimit}
              onClick={openCreator}
            >
              バケットを作成
            </Button>
          </SpaceBetween>
        }
      />
      <Container>
        <ColumnLayout columns={3} variant="text-grid">
          <div>
            <Box variant="awsui-key-label">バケット</Box>
            <Box variant="awsui-value-large">
              {formatNumber(items.length)} / {formatNumber(quota.data.max_buckets)}
            </Box>
          </div>
          <div>
            <Box variant="awsui-key-label">準備完了</Box>
            <Box variant="awsui-value-large">{formatNumber(readyCount)}</Box>
          </div>
          <div>
            <Box variant="awsui-key-label">割り当て容量</Box>
            <Box variant="awsui-value-large">
              {formatBytes(allocatedBytes)} / {formatBytes(quota.data.max_total_bytes)}
            </Box>
          </div>
        </ColumnLayout>
      </Container>
      <DataTable
        columns={columns}
        data={items}
        getRowId={(bucket) => bucket.id}
        onRowClick={(bucket) => navigate(`/syouyu/buckets/${bucket.id}`)}
        getRowAriaLabel={(bucket) =>
          `${bucket.spec.bucket_name}の詳細を開く`
        }
        mobileVisibleColumns={["bucket", "state", "quota"]}
        searchPlaceholder="バケット名、プロジェクト、リージョン、状態で検索"
        emptyTitle="バケットがありません"
        emptyDescription="S3互換オブジェクトストレージのバケットを作成してください。"
      />
      <Modal
        visible={createOpen}
        onDismiss={() => setCreateOpen(false)}
        size="large"
        header="Syouyuバケットを作成"
        footer={
          <Box float="right">
            <SpaceBetween direction="horizontal" size="xs">
              <Button onClick={() => setCreateOpen(false)}>キャンセル</Button>
              <Button
                variant="primary"
                loading={createBucket.isPending}
                disabled={Boolean(validationError)}
                onClick={() => createBucket.mutate(form)}
              >
                作成
              </Button>
            </SpaceBetween>
          </Box>
        }
      >
        <SyouyuBucketForm
          value={form}
          quota={quota.data}
          onChange={setForm}
          onSubmit={submit}
          disabled={createBucket.isPending}
        >
          <FormError
            message={
              createBucket.isError
                ? getApiErrorMessage(createBucket.error)
                : validationError
            }
          />
        </SyouyuBucketForm>
      </Modal>
    </SpaceBetween>
  );
}
