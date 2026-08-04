import Alert from "@cloudscape-design/components/alert";
import Box from "@cloudscape-design/components/box";
import Button from "@cloudscape-design/components/button";
import ColumnLayout from "@cloudscape-design/components/column-layout";
import Container from "@cloudscape-design/components/container";
import Header from "@cloudscape-design/components/header";
import KeyValuePairs from "@cloudscape-design/components/key-value-pairs";
import Modal from "@cloudscape-design/components/modal";
import SegmentedControl from "@cloudscape-design/components/segmented-control";
import SpaceBetween from "@cloudscape-design/components/space-between";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { type FormEvent, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { ErrorState } from "@/components/shared/error-state";
import { FormError } from "@/components/shared/form-error";
import { PageHeader } from "@/components/shared/page-header";
import { PageLoading } from "@/components/shared/page-loading";
import { RouterLink } from "@/components/shared/router-link";
import { StatusBadge } from "@/components/shared/status-badge";
import { useActiveOrganization } from "@/features/organizations/organization-context";
import { api, getApiErrorMessage } from "@/lib/api-client";
import type {
  RealtimeMetricsRange,
  RealtimeService,
  RealtimeServiceMetricSample,
} from "@/lib/api-types";
import {
  projectsQueryOptions,
  realtimeServiceMetricHistoryQueryOptions,
  realtimeServiceMetricsQueryOptions,
  realtimeServiceQueryOptions,
} from "@/lib/queries";
import { formatDateTime, formatNumber } from "@/lib/utils";
import { AccessCredentialDialog } from "./access-credential-dialog";
import { DeveloperCredentialsSection } from "./developer-credentials-section";
import { RealtimeEndpoints } from "./realtime-endpoints";
import { RealtimeMetricChart } from "./realtime-metric-chart";
import { RealtimeServiceForm, type RealtimeServiceFormValue } from "./realtime-service-form";
import {
  formatBytes,
  serviceEndpoints,
  transferRateSamplesPerHour,
  transferredBytes,
} from "./realtime-service-utils";

const metricRanges: { id: RealtimeMetricsRange; text: string }[] = [
  { id: "1h", text: "1時間" },
  { id: "6h", text: "6時間" },
  { id: "24h", text: "24時間" },
  { id: "7d", text: "7日" },
  { id: "30d", text: "30日" },
];

function apiResourceUrl(endpoint: string | undefined, path: string): string | null {
  if (!endpoint) return null;
  try {
    return new URL(path, endpoint).toString();
  } catch {
    return null;
  }
}

function formValue(service: RealtimeService): RealtimeServiceFormValue {
  return {
    projectId: service.project_id,
    name: service.name,
    region: service.spec.region,
    maxParticipants: service.spec.max_participants,
    maxRooms: service.spec.max_rooms,
    rateLimitRequestsPerSecond: service.spec.rate_limit.requests_per_second,
    rateLimitBurst: service.spec.rate_limit.burst,
  };
}

export function RealtimeServiceDetailPage() {
  const { serviceId = "" } = useParams<{ serviceId: string }>();
  const { activeOrganization } = useActiveOrganization();
  const organizationId = activeOrganization.organization_id;
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const service = useQuery({
    ...realtimeServiceQueryOptions(organizationId, serviceId),
    enabled: Boolean(serviceId),
  });
  const metrics = useQuery({
    ...realtimeServiceMetricsQueryOptions(organizationId, serviceId),
    enabled: Boolean(serviceId) && service.data?.state === "ready",
  });
  const [metricsRange, setMetricsRange] = useState<RealtimeMetricsRange>("24h");
  const metricHistory = useQuery({
    ...realtimeServiceMetricHistoryQueryOptions(
      organizationId,
      service.data?.project_id ?? "",
      serviceId,
      metricsRange,
    ),
    enabled: Boolean(serviceId) && Boolean(service.data?.project_id) && service.data?.state === "ready",
  });
  const projects = useQuery(projectsQueryOptions(organizationId));
  const [editOpen, setEditOpen] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [editForm, setEditForm] = useState<RealtimeServiceFormValue | null>(null);
  const updateService = useMutation({
    mutationFn: (value: RealtimeServiceFormValue) =>
      api.realtime.services.update(organizationId, serviceId, {
        name: value.name.trim(),
        spec: {
          region: value.region,
          max_participants: value.maxParticipants,
          max_rooms: value.maxRooms,
          rate_limit: {
            requests_per_second: value.rateLimitRequestsPerSecond,
            burst: value.rateLimitBurst,
          },
          metadata: service.data?.spec.metadata ?? {},
        },
      }),
    onSuccess: async (updated) => {
      queryClient.setQueryData(
        realtimeServiceQueryOptions(organizationId, serviceId).queryKey,
        updated,
      );
      setEditOpen(false);
      await queryClient.invalidateQueries({
        queryKey: ["organizations", organizationId, "realtime", "services"],
      });
    },
  });
  const deleteService = useMutation({
    mutationFn: () => api.realtime.services.delete(organizationId, serviceId),
    onSuccess: async () => {
      await queryClient.invalidateQueries({
        queryKey: ["organizations", organizationId, "realtime", "services"],
      });
      navigate("/flow/services", { replace: true });
    },
  });

  if (!serviceId) {
    return <ErrorState title="サービスを指定してください" description="サービスIDがURLに含まれていません。" />;
  }
  if (service.isPending || projects.isPending) {
    return <PageLoading label="サービス詳細を読み込んでいます" />;
  }
  if (service.isError || projects.isError) {
    return (
      <ErrorState
        title="サービスを取得できませんでした"
        description="サービスが存在しないか、参照権限がありません。"
        onRetry={() => {
          void service.refetch();
          void projects.refetch();
        }}
      />
    );
  }

  const item = service.data;
  const itemMetrics = metrics.data;
  const endpoints = itemMetrics?.endpoints ?? serviceEndpoints(item);
  const apiDocumentationUrl = apiResourceUrl(endpoints.api[0], "/docs/");
  const openApiUrl = apiResourceUrl(endpoints.api[0], "/openapi.json");
  const projectName =
    projects.data.items.find((project) => project.id === item.project_id)?.name ?? item.project_id;
  const disabled = item.state === "deleting";
  const metricItems = [
    ["アクティブルーム", itemMetrics ? formatNumber(itemMetrics.active_rooms) : "-"],
    ["同時接続", itemMetrics ? formatNumber(itemMetrics.concurrent_connections) : "-"],
    ["Ingress", itemMetrics ? formatBytes(itemMetrics.ingress_bytes) : "-"],
    ["Egress", itemMetrics ? formatBytes(itemMetrics.egress_bytes) : "-"],
    ["転送量", itemMetrics ? formatBytes(transferredBytes(itemMetrics)) : "-"],
  ];
  const historySamples = metricHistory.data?.samples ?? [];
  const transferRateSamples = transferRateSamplesPerHour(historySamples);
  const historyCharts: {
    label: string;
    samples: RealtimeServiceMetricSample[];
    value: (sample: RealtimeServiceMetricSample) => number;
    formatValue: (value: number) => string;
    color: string;
  }[] = [
    { label: "アクティブルーム", samples: historySamples, value: (sample) => sample.active_rooms, formatValue: formatNumber, color: "#0972d3" },
    { label: "同時接続", samples: historySamples, value: (sample) => sample.concurrent_connections, formatValue: formatNumber, color: "#1d8102" },
    { label: "Ingress / 時間", samples: transferRateSamples, value: (sample) => sample.ingress_bytes, formatValue: (value) => `${formatBytes(value)}/h`, color: "#8d6605" },
    { label: "Egress / 時間", samples: transferRateSamples, value: (sample) => sample.egress_bytes, formatValue: (value) => `${formatBytes(value)}/h`, color: "#d91515" },
    { label: "転送量 / 時間", samples: transferRateSamples, value: (sample) => sample.transferred_bytes, formatValue: (value) => `${formatBytes(value)}/h`, color: "#414d5c" },
  ];
  const submitEdit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (editForm) updateService.mutate(editForm);
  };

  return (
    <SpaceBetween size="l">
      <RouterLink to="/flow/services">Flow</RouterLink>
      <PageHeader
        title={item.name}
        description={`サービスID: ${item.id}`}
        actions={
          <SpaceBetween direction="horizontal" size="xs">
            <Button
              variant="icon"
              iconName="refresh"
              ariaLabel="更新"
              onClick={() => void Promise.all([service.refetch(), metrics.refetch(), metricHistory.refetch()])}
            />
            <Button
              iconName="edit"
              disabled={disabled}
              onClick={() => {
                setEditForm(formValue(item));
                updateService.reset();
                setEditOpen(true);
              }}
            >
              編集
            </Button>
            {apiDocumentationUrl ? (
              <Button href={apiDocumentationUrl} target="_blank" iconName="file-open" external>
                APIドキュメント
              </Button>
            ) : null}
            {openApiUrl ? (
              <Button
                variant="icon"
                href={openApiUrl}
                target="_blank"
                iconName="file"
                external
                ariaLabel="OpenAPI JSONを開く"
              />
            ) : null}
            <AccessCredentialDialog
              organizationId={organizationId}
              serviceId={item.id}
              serviceName={item.name}
              disabled={item.state !== "ready"}
            />
            <Button
              iconName="remove"
              disabled={disabled}
              onClick={() => {
                deleteService.reset();
                setDeleteOpen(true);
              }}
            >
              削除
            </Button>
          </SpaceBetween>
        }
      />
      <Container>
        <ColumnLayout columns={5} variant="text-grid">
          {metricItems.map(([label, value]) => (
            <div key={label}>
              <Box variant="awsui-key-label">{label}</Box>
              <Box variant="awsui-value-large">{value}</Box>
            </div>
          ))}
        </ColumnLayout>
      </Container>
      {metrics.isError ? (
        <Alert
          type="warning"
          action={<Button onClick={() => void metrics.refetch()}>再試行</Button>}
        >
          Flowメトリクスを取得できませんでした。
        </Alert>
      ) : null}
      <SpaceBetween size="m">
        <Header
          variant="h2"
          description="アクティブルーム、接続数、時間あたり通信量の履歴"
          actions={
            <SegmentedControl
              selectedId={metricsRange}
              options={metricRanges}
              label="メトリクスの表示期間"
              onChange={({ detail }) => setMetricsRange(detail.selectedId as RealtimeMetricsRange)}
            />
          }
        >
          モニタリング
        </Header>
        {metricHistory.isError ? (
          <Alert
            type="warning"
            action={<Button onClick={() => void metricHistory.refetch()}>再試行</Button>}
          >
            メトリクス履歴を取得できませんでした。
          </Alert>
        ) : null}
        <ColumnLayout columns={2}>
          {historyCharts.map((chart) => (
            <RealtimeMetricChart
              key={chart.label}
              {...chart}
              loading={metricHistory.isPending}
            />
          ))}
        </ColumnLayout>
      </SpaceBetween>
      <ColumnLayout columns={2}>
        <Container
          header={
            <Header variant="h2" description="自動割り当て">
              実エンドポイント
            </Header>
          }
        >
          <RealtimeEndpoints endpoints={endpoints} />
        </Container>
        <Container header={<Header variant="h2">サービス設定</Header>}>
          <KeyValuePairs
            columns={2}
            items={[
              { label: "状態", value: <StatusBadge status={item.state} /> },
              { label: "プロジェクト", value: projectName },
              { label: "リージョン", value: item.spec.region },
              { label: "ルーム上限", value: formatNumber(item.spec.max_rooms) },
              { label: "同時参加者上限", value: formatNumber(item.spec.max_participants) },
              {
                label: "IPレート制限",
                value: `${formatNumber(item.spec.rate_limit.requests_per_second)} RPS / burst ${formatNumber(item.spec.rate_limit.burst)}`,
              },
              { label: "更新日時", value: formatDateTime(item.updated_at) },
            ]}
          />
        </Container>
      </ColumnLayout>
      <DeveloperCredentialsSection
        organizationId={organizationId}
        serviceId={item.id}
        disabled={item.state !== "ready"}
      />
      <Modal
        visible={editOpen}
        onDismiss={() => setEditOpen(false)}
        size="large"
        header="サービス設定を編集"
        footer={
          <Box float="right">
            <SpaceBetween direction="horizontal" size="xs">
              <Button onClick={() => setEditOpen(false)}>キャンセル</Button>
              <Button
                variant="primary"
                loading={updateService.isPending}
                disabled={!editForm?.name.trim()}
                onClick={() => editForm && updateService.mutate(editForm)}
              >
                変更を保存
              </Button>
            </SpaceBetween>
          </Box>
        }
      >
        {editForm ? (
          <RealtimeServiceForm
            value={editForm}
            onChange={setEditForm}
            onSubmit={submitEdit}
            disabled={updateService.isPending}
            projectLocked
          >
            <FormError message={updateService.isError ? getApiErrorMessage(updateService.error) : null} />
          </RealtimeServiceForm>
        ) : null}
      </Modal>
      <Modal
        visible={deleteOpen}
        onDismiss={() => setDeleteOpen(false)}
        header="サービスを削除"
        footer={
          <Box float="right">
            <SpaceBetween direction="horizontal" size="xs">
              <Button onClick={() => setDeleteOpen(false)}>キャンセル</Button>
              <Button
                variant="primary"
                iconName="remove"
                loading={deleteService.isPending}
                onClick={() => deleteService.mutate()}
              >
                削除する
              </Button>
            </SpaceBetween>
          </Box>
        }
      >
        <SpaceBetween size="l">
          <Alert type="warning" header="この操作は取り消せません">
            {item.name} と関連するルームおよび認証情報を削除します。
          </Alert>
          <FormError message={deleteService.isError ? getApiErrorMessage(deleteService.error) : null} />
        </SpaceBetween>
      </Modal>
    </SpaceBetween>
  );
}
