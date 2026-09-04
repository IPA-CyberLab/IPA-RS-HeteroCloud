import Alert from "@cloudscape-design/components/alert";
import Box from "@cloudscape-design/components/box";
import Button from "@cloudscape-design/components/button";
import ColumnLayout from "@cloudscape-design/components/column-layout";
import Container from "@cloudscape-design/components/container";
import FormField from "@cloudscape-design/components/form-field";
import Header from "@cloudscape-design/components/header";
import Input from "@cloudscape-design/components/input";
import KeyValuePairs from "@cloudscape-design/components/key-value-pairs";
import Modal from "@cloudscape-design/components/modal";
import ProgressBar from "@cloudscape-design/components/progress-bar";
import SpaceBetween from "@cloudscape-design/components/space-between";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { ErrorState } from "@/components/shared/error-state";
import { FormError } from "@/components/shared/form-error";
import { PageHeader } from "@/components/shared/page-header";
import { PageLoading } from "@/components/shared/page-loading";
import { RouterLink } from "@/components/shared/router-link";
import { StatusBadge } from "@/components/shared/status-badge";
import { useActiveOrganization } from "@/features/organizations/organization-context";
import { api, getApiErrorMessage } from "@/lib/api-client";
import {
  projectsQueryOptions,
  syouyuBucketQueryOptions,
  syouyuQuotaQueryOptions,
  syouyuUsageQueryOptions,
} from "@/lib/queries";
import { formatDateTime, formatNumber } from "@/lib/utils";
import { SyouyuCredentialsSection } from "./syouyu-credentials-section";
import { formatBytes, GIBIBYTE, quotaGib } from "./syouyu-utils";

function usagePercent(used: number, limit: number): number {
  if (limit <= 0) return 0;
  return Math.min(100, Math.max(0, (used / limit) * 100));
}

export function SyouyuBucketDetailPage() {
  const { bucketId = "" } = useParams<{ bucketId: string }>();
  const { activeOrganization } = useActiveOrganization();
  const organizationId = activeOrganization.organization_id;
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const bucket = useQuery({
    ...syouyuBucketQueryOptions(organizationId, bucketId),
    enabled: Boolean(bucketId),
  });
  const quota = useQuery(syouyuQuotaQueryOptions(organizationId));
  const projects = useQuery(projectsQueryOptions(organizationId));
  const usage = useQuery({
    ...syouyuUsageQueryOptions(organizationId, bucketId),
    enabled: Boolean(bucketId) && bucket.data?.state === "ready",
  });
  const [editOpen, setEditOpen] = useState(false);
  const [editQuotaGib, setEditQuotaGib] = useState(1);
  const [editQuotaObjects, setEditQuotaObjects] = useState(1);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [deleteConfirmation, setDeleteConfirmation] = useState("");

  const updateBucket = useMutation({
    mutationFn: () => {
      if (!bucket.data) throw new Error("bucket is not loaded");
      return api.syouyu.buckets.update(organizationId, bucketId, {
        name: bucket.data.name,
        spec: {
          ...bucket.data.spec,
          quota_bytes: editQuotaGib * GIBIBYTE,
          quota_objects: editQuotaObjects,
        },
      });
    },
    onSuccess: async (updated) => {
      queryClient.setQueryData(
        syouyuBucketQueryOptions(organizationId, bucketId).queryKey,
        updated,
      );
      setEditOpen(false);
      await Promise.all([
        queryClient.invalidateQueries({
          queryKey: ["organizations", organizationId, "syouyu", "buckets"],
        }),
        queryClient.invalidateQueries({
          queryKey: [
            "organizations",
            organizationId,
            "syouyu",
            "buckets",
            bucketId,
            "usage",
          ],
        }),
      ]);
    },
  });
  const deleteBucket = useMutation({
    mutationFn: () => api.syouyu.buckets.delete(organizationId, bucketId),
    onSuccess: async () => {
      await queryClient.invalidateQueries({
        queryKey: ["organizations", organizationId, "syouyu", "buckets"],
      });
      navigate("/syouyu/buckets", { replace: true });
    },
  });

  if (!bucketId) {
    return (
      <ErrorState
        title="バケットを指定してください"
        description="バケットIDがURLに含まれていません。"
      />
    );
  }
  if (bucket.isPending || quota.isPending || projects.isPending) {
    return <PageLoading label="Syouyuバケット詳細を読み込んでいます" />;
  }
  if (bucket.isError || quota.isError || projects.isError) {
    return (
      <ErrorState
        title="Syouyuバケットを取得できませんでした"
        description={getApiErrorMessage(
          bucket.error ?? quota.error ?? projects.error,
        )}
        onRetry={() => {
          void bucket.refetch();
          void quota.refetch();
          void projects.refetch();
        }}
      />
    );
  }

  const item = bucket.data;
  const itemUsage = usage.data;
  const usedBytes = itemUsage?.used_bytes ?? item.status.bytes ?? 0;
  const objectCount = itemUsage?.object_count ?? item.status.objects ?? 0;
  const credentialCount =
    itemUsage?.credential_count ?? item.status.credentials ?? 0;
  const unfinishedUploadBytes = itemUsage?.unfinished_upload_bytes ?? 0;
  const byteLimit = itemUsage?.quota_bytes ?? item.spec.quota_bytes;
  const objectLimit = itemUsage?.quota_objects ?? item.spec.quota_objects;
  const projectName =
    projects.data.items.find((project) => project.id === item.project_id)?.name ??
    item.project_id;
  const disabled = item.state === "deleting";
  const editError =
    !Number.isInteger(editQuotaGib) || editQuotaGib < 1
      ? "容量上限は1 GiB以上の整数で入力してください。"
      : editQuotaGib * GIBIBYTE > quota.data.max_bytes_per_bucket
        ? `容量上限は${formatBytes(quota.data.max_bytes_per_bucket)}以下にしてください。`
        : !Number.isInteger(editQuotaObjects) || editQuotaObjects < 1
          ? "オブジェクト数上限は1以上の整数で入力してください。"
          : editQuotaObjects > quota.data.max_objects_per_bucket
            ? `オブジェクト数上限は${formatNumber(quota.data.max_objects_per_bucket)}以下にしてください。`
            : null;
  const editBelowUsage =
    editQuotaGib * GIBIBYTE < usedBytes || editQuotaObjects < objectCount;
  const providerMessage =
    typeof item.status.message === "string" ? item.status.message : null;

  const openEditor = () => {
    setEditQuotaGib(quotaGib(item.spec.quota_bytes));
    setEditQuotaObjects(item.spec.quota_objects);
    updateBucket.reset();
    setEditOpen(true);
  };

  return (
    <SpaceBetween size="l">
      <RouterLink to="/syouyu/buckets">Syouyu</RouterLink>
      <PageHeader
        title={item.spec.bucket_name}
        description={`バケットID: ${item.id}`}
        actions={
          <SpaceBetween direction="horizontal" size="xs">
            <Button
              variant="icon"
              iconName="refresh"
              ariaLabel="Syouyuバケットを更新"
              onClick={() =>
                void queryClient.invalidateQueries({
                  queryKey: [
                    "organizations",
                    organizationId,
                    "syouyu",
                    "buckets",
                    bucketId,
                  ],
                })
              }
            />
            <Button iconName="edit" disabled={disabled} onClick={openEditor}>
              クォータを編集
            </Button>
            <Button
              iconName="remove"
              disabled={disabled}
              onClick={() => {
                setDeleteConfirmation("");
                deleteBucket.reset();
                setDeleteOpen(true);
              }}
            >
              削除
            </Button>
          </SpaceBetween>
        }
      />

      {providerMessage ? (
        <Alert type={item.state === "error" ? "error" : "info"}>
          {providerMessage}
        </Alert>
      ) : null}

      <Container>
        <ColumnLayout columns={3} variant="text-grid">
          <div>
            <Box variant="awsui-key-label">使用容量</Box>
            <Box variant="awsui-value-large">{formatBytes(usedBytes)}</Box>
          </div>
          <div>
            <Box variant="awsui-key-label">オブジェクト</Box>
            <Box variant="awsui-value-large">{formatNumber(objectCount)}</Box>
          </div>
          <div>
            <Box variant="awsui-key-label">有効な認証情報</Box>
            <Box variant="awsui-value-large">{formatNumber(credentialCount)}</Box>
          </div>
        </ColumnLayout>
      </Container>

      {usage.isError ? (
        <Alert
          type="warning"
          action={<Button onClick={() => void usage.refetch()}>再試行</Button>}
        >
          最新の使用量を取得できませんでした。最後に報告された値を表示しています。
        </Alert>
      ) : null}

      <ColumnLayout columns={2}>
        <Container header={<Header variant="h2">使用量とクォータ</Header>}>
          <SpaceBetween size="l">
            <ProgressBar
              value={usagePercent(usedBytes, byteLimit)}
              label="保存容量"
              description={`${formatBytes(usedBytes)} / ${formatBytes(byteLimit)}`}
            />
            <ProgressBar
              value={usagePercent(objectCount, objectLimit)}
              label="オブジェクト数"
              description={`${formatNumber(objectCount)} / ${formatNumber(objectLimit)}`}
            />
            <div>
              <Box variant="awsui-key-label">未完了アップロード</Box>
              <Box>{formatBytes(unfinishedUploadBytes)}</Box>
            </div>
          </SpaceBetween>
        </Container>
        <Container header={<Header variant="h2">バケット設定</Header>}>
          <KeyValuePairs
            columns={2}
            items={[
              { label: "状態", value: <StatusBadge status={item.state} /> },
              { label: "プロジェクト", value: projectName },
              { label: "リージョン", value: item.spec.region },
              {
                label: "エンドポイント",
                value: item.status.endpoint ? (
                  <Box variant="code">{item.status.endpoint}</Box>
                ) : (
                  "未割り当て"
                ),
              },
              {
                label: "バケット名",
                value: <Box variant="code">{item.spec.bucket_name}</Box>,
              },
              { label: "更新日時", value: formatDateTime(item.updated_at) },
            ]}
          />
        </Container>
      </ColumnLayout>

      <SyouyuCredentialsSection
        key={`${organizationId}:${bucketId}`}
        organizationId={organizationId}
        bucketId={bucketId}
        maxCredentials={quota.data.max_credentials_per_bucket}
        disabled={item.state !== "ready"}
      />

      <Modal
        visible={editOpen}
        onDismiss={() => setEditOpen(false)}
        header="バケットクォータを編集"
        footer={
          <Box float="right">
            <SpaceBetween direction="horizontal" size="xs">
              <Button onClick={() => setEditOpen(false)}>キャンセル</Button>
              <Button
                variant="primary"
                loading={updateBucket.isPending}
                disabled={Boolean(editError) || editBelowUsage}
                onClick={() => updateBucket.mutate()}
              >
                変更を保存
              </Button>
            </SpaceBetween>
          </Box>
        }
      >
        <SpaceBetween size="l">
          <Box color="text-body-secondary">
            現在の使用量を下回る値には変更できません。
          </Box>
          <ColumnLayout columns={2}>
            <FormField
              label="容量上限 (GiB)"
              constraintText={`最大 ${formatBytes(quota.data.max_bytes_per_bucket)}`}
            >
              <Input
                type="number"
                inputMode="numeric"
                step={1}
                nativeInputAttributes={{
                  min: Math.max(1, Math.ceil(usedBytes / GIBIBYTE)),
                  max: Math.floor(quota.data.max_bytes_per_bucket / GIBIBYTE),
                }}
                value={String(editQuotaGib)}
                disabled={updateBucket.isPending}
                onChange={({ detail }) => setEditQuotaGib(Number(detail.value))}
              />
            </FormField>
            <FormField
              label="オブジェクト数上限"
              constraintText={`最大 ${formatNumber(quota.data.max_objects_per_bucket)}`}
            >
              <Input
                type="number"
                inputMode="numeric"
                step={1}
                nativeInputAttributes={{
                  min: Math.max(1, objectCount),
                  max: quota.data.max_objects_per_bucket,
                }}
                value={String(editQuotaObjects)}
                disabled={updateBucket.isPending}
                onChange={({ detail }) =>
                  setEditQuotaObjects(Number(detail.value))
                }
              />
            </FormField>
          </ColumnLayout>
          <FormError
            message={
              updateBucket.isError
                ? getApiErrorMessage(updateBucket.error)
                : editBelowUsage
                  ? "現在の使用量以上のクォータを指定してください。"
                  : editError
            }
          />
        </SpaceBetween>
      </Modal>

      <Modal
        visible={deleteOpen}
        onDismiss={() => setDeleteOpen(false)}
        header="Syouyuバケットを削除"
        footer={
          <Box float="right">
            <SpaceBetween direction="horizontal" size="xs">
              <Button onClick={() => setDeleteOpen(false)}>キャンセル</Button>
              <Button
                variant="primary"
                iconName="remove"
                loading={deleteBucket.isPending}
                disabled={deleteConfirmation !== item.spec.bucket_name}
                onClick={() => deleteBucket.mutate()}
              >
                削除する
              </Button>
            </SpaceBetween>
          </Box>
        }
      >
        <SpaceBetween size="l">
          <Alert type="warning" header="この操作は取り消せません">
            空のバケットと、そのアクセス認証情報を削除します。オブジェクトが残っているバケットは削除できません。
          </Alert>
          <FormField
            label="確認"
            description={`続行するには ${item.spec.bucket_name} と入力してください。`}
          >
            <Input
              value={deleteConfirmation}
              autoComplete="off"
              onChange={({ detail }) => setDeleteConfirmation(detail.value)}
            />
          </FormField>
          <FormError
            message={
              deleteBucket.isError ? getApiErrorMessage(deleteBucket.error) : null
            }
          />
        </SpaceBetween>
      </Modal>
    </SpaceBetween>
  );
}
