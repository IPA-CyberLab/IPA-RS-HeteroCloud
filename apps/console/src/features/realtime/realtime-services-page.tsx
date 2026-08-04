import Box from "@cloudscape-design/components/box";
import Button from "@cloudscape-design/components/button";
import ColumnLayout from "@cloudscape-design/components/column-layout";
import Container from "@cloudscape-design/components/container";
import Modal from "@cloudscape-design/components/modal";
import SpaceBetween from "@cloudscape-design/components/space-between";
import { useMutation, useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
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
import type { RealtimeService, RealtimeServiceMetrics } from "@/lib/api-types";
import {
  projectsQueryOptions,
  realtimeServiceMetricsQueryOptions,
  realtimeServicesQueryOptions,
} from "@/lib/queries";
import { formatDateTime, formatNumber } from "@/lib/utils";
import {
  defaultRealtimeServiceFormValue,
  RealtimeServiceForm,
  type RealtimeServiceFormValue,
} from "./realtime-service-form";
import { formatBytes, transferredBytes } from "./realtime-service-utils";

function MetricValue({
  metrics,
  value,
}: {
  metrics: RealtimeServiceMetrics | undefined;
  value: (metrics: RealtimeServiceMetrics) => string;
}) {
  return metrics ? <Box fontWeight="bold">{value(metrics)}</Box> : <Box color="text-body-secondary">-</Box>;
}

export function RealtimeServicesPage() {
  const { activeOrganization } = useActiveOrganization();
  const organizationId = activeOrganization.organization_id;
  const services = useQuery(realtimeServicesQueryOptions(organizationId));
  const projects = useQuery(projectsQueryOptions(organizationId));
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [createOpen, setCreateOpen] = useState(false);
  const [form, setForm] = useState<RealtimeServiceFormValue>(defaultRealtimeServiceFormValue);
  const serviceItems = services.data?.items ?? [];
  const metricQueries = useQueries({
    queries: serviceItems.map((service) => ({
      ...realtimeServiceMetricsQueryOptions(organizationId, service.id),
      enabled: service.state === "ready",
    })),
  });
  const metricsByService = useMemo(
    () =>
      new Map(
        serviceItems.map((service, index) => [service.id, metricQueries[index]?.data]),
      ),
    [metricQueries, serviceItems],
  );
  const createService = useMutation({
    mutationFn: (value: RealtimeServiceFormValue) =>
      api.realtime.services.create(organizationId, {
        project_id: value.projectId,
        name: value.name.trim(),
        spec: {
          region: value.region,
          max_participants: value.maxParticipants,
          max_rooms: value.maxRooms,
          rate_limit: {
            requests_per_second: value.rateLimitRequestsPerSecond,
            burst: value.rateLimitBurst,
          },
          turn_enabled: value.turnEnabled,
          metadata: {},
        },
      }),
    onSuccess: async () => {
      setCreateOpen(false);
      setForm(defaultRealtimeServiceFormValue);
      await queryClient.invalidateQueries({
        queryKey: ["organizations", organizationId, "realtime", "services"],
      });
    },
  });
  const projectNames = useMemo(
    () => new Map((projects.data?.items ?? []).map((project) => [project.id, project.name])),
    [projects.data],
  );
  const columns = useMemo<ColumnDef<RealtimeService, unknown>[]>(
    () => [
      {
        accessorKey: "name",
        header: "サービス",
        cell: ({ row }) => (
          <SpaceBetween size="xxs">
            <Box fontWeight="bold">{row.original.name}</Box>
            <Box variant="code" className="mobile-hidden">{row.original.id}</Box>
          </SpaceBetween>
        ),
      },
      {
        id: "project",
        accessorFn: (service) => projectNames.get(service.project_id) ?? service.project_id,
        header: "プロジェクト",
        cell: ({ row }) => projectNames.get(row.original.project_id) ?? <Box variant="code">{row.original.project_id}</Box>,
      },
      {
        accessorKey: "state",
        header: "状態",
        cell: ({ getValue }) => <StatusBadge status={getValue<RealtimeService["state"]>()} />,
      },
      {
        id: "activeRooms",
        header: "アクティブルーム",
        enableSorting: false,
        cell: ({ row }) => (
          <MetricValue
            metrics={metricsByService.get(row.original.id)}
            value={(metrics) => formatNumber(metrics.active_rooms)}
          />
        ),
      },
      {
        id: "connections",
        header: "同時接続",
        enableSorting: false,
        cell: ({ row }) => (
          <MetricValue
            metrics={metricsByService.get(row.original.id)}
            value={(metrics) => formatNumber(metrics.concurrent_connections)}
          />
        ),
      },
      {
        id: "traffic",
        header: "転送量",
        enableSorting: false,
        cell: ({ row }) => (
          <MetricValue
            metrics={metricsByService.get(row.original.id)}
            value={(metrics) => formatBytes(transferredBytes(metrics))}
          />
        ),
      },
      {
        id: "maxParticipants",
        accessorFn: (service) => service.spec.max_participants,
        header: "同時参加者上限",
        cell: ({ row }) => formatNumber(row.original.spec.max_participants),
      },
      {
        accessorKey: "updated_at",
        header: "更新日時",
        cell: ({ getValue }) => formatDateTime(getValue<string>()),
      },
    ],
    [metricsByService, projectNames],
  );
  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    createService.mutate(form);
  };

  if (services.isPending || projects.isPending) return <PageLoading label="Flowを読み込んでいます" />;
  if (services.isError || projects.isError) {
    return (
      <ErrorState
        description="サービスまたはプロジェクト一覧を取得できませんでした。"
        onRetry={() => {
          void services.refetch();
          void projects.refetch();
        }}
      />
    );
  }

  const readyCount = serviceItems.filter((service) => service.state === "ready").length;
  const activeRooms = metricQueries.reduce((sum, query) => sum + (query.data?.active_rooms ?? 0), 0);
  const connections = metricQueries.reduce(
    (sum, query) => sum + (query.data?.concurrent_connections ?? 0),
    0,
  );

  return (
    <SpaceBetween size="l">
      <PageHeader
        title="Flow"
        description={`${activeOrganization.organization_name} のWebRTC、LiveKit、STUN、TURN基盤を管理します。`}
        actions={
          <SpaceBetween direction="horizontal" size="xs">
            <Button
              variant="icon"
              iconName="refresh"
              ariaLabel="更新"
              onClick={() =>
                void queryClient.invalidateQueries({
                  queryKey: ["organizations", organizationId, "realtime", "services"],
                })
              }
            />
            <Button
              variant="primary"
              iconName="add-plus"
              onClick={() => {
                createService.reset();
                setCreateOpen(true);
              }}
            >
              サービスを作成
            </Button>
          </SpaceBetween>
        }
      />
      <Container>
        <ColumnLayout columns={3} variant="text-grid">
          {[
            ["準備完了", readyCount],
            ["アクティブルーム", activeRooms],
            ["同時接続", connections],
          ].map(([label, value]) => (
            <div key={label}>
              <Box variant="awsui-key-label">{label}</Box>
              <Box variant="awsui-value-large">{formatNumber(Number(value))}</Box>
            </div>
          ))}
        </ColumnLayout>
      </Container>
      <DataTable
        columns={columns}
        data={serviceItems}
        getRowId={(service) => service.id}
        onRowClick={(service) => navigate(`/flow/services/${service.id}`)}
        getRowAriaLabel={(service) => `${service.name}の詳細を開く`}
        mobileVisibleColumns={["name", "state", "connections"]}
        searchPlaceholder="名前、プロジェクト、リージョン、状態で検索"
        emptyTitle="Flowがありません"
        emptyDescription="プロジェクトを選択してサービスを作成してください。"
      />
      <Modal
        visible={createOpen}
        onDismiss={() => setCreateOpen(false)}
        size="large"
        header="Flowを作成"
        footer={
          <Box float="right">
            <SpaceBetween direction="horizontal" size="xs">
              <Button onClick={() => setCreateOpen(false)}>キャンセル</Button>
              <Button
                variant="primary"
                loading={createService.isPending}
                disabled={!form.projectId || !form.name.trim()}
                onClick={() => createService.mutate(form)}
              >
                作成
              </Button>
            </SpaceBetween>
          </Box>
        }
      >
        <RealtimeServiceForm
          value={form}
          onChange={setForm}
          onSubmit={submit}
          disabled={createService.isPending}
        >
          <FormError message={createService.isError ? getApiErrorMessage(createService.error) : null} />
        </RealtimeServiceForm>
      </Modal>
    </SpaceBetween>
  );
}
