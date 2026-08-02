import { useMutation, useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import type { ColumnDef } from "@tanstack/react-table";
import {
  ArrowRight,
  LoaderCircle,
  Plus,
  RadioTower,
  RefreshCw,
  UsersRound,
} from "lucide-react";
import { type FormEvent, useMemo, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { DataTable } from "@/components/shared/data-table";
import { ErrorState } from "@/components/shared/error-state";
import { FormError } from "@/components/shared/form-error";
import { PageHeader } from "@/components/shared/page-header";
import { PageLoading } from "@/components/shared/page-loading";
import { StatusBadge } from "@/components/shared/status-badge";
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
import {
  formatBytes,
  trafficModeLabels,
  transferredBytes,
} from "./realtime-service-utils";

function MetricValue({
  metrics,
  value,
}: {
  metrics: RealtimeServiceMetrics | undefined;
  value: (metrics: RealtimeServiceMetrics) => string;
}) {
  return metrics ? (
    <span className="font-medium text-zinc-900">{value(metrics)}</span>
  ) : (
    <span className="text-zinc-400">—</span>
  );
}

export function RealtimeServicesPage() {
  const { activeOrganization } = useActiveOrganization();
  const organizationId = activeOrganization.organization_id;
  const services = useQuery(realtimeServicesQueryOptions(organizationId));
  const projects = useQuery(projectsQueryOptions(organizationId));
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [createOpen, setCreateOpen] = useState(false);
  const [form, setForm] = useState<RealtimeServiceFormValue>(
    defaultRealtimeServiceFormValue,
  );

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
        serviceItems.map((service, index) => [
          service.id,
          metricQueries[index]?.data,
        ]),
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
          traffic_mode: value.trafficMode,
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
    () =>
      new Map(
        (projects.data?.items ?? []).map((project) => [project.id, project.name]),
      ),
    [projects.data],
  );

  const columns = useMemo<ColumnDef<RealtimeService, unknown>[]>(
    () => [
      {
        accessorKey: "name",
        header: "サービス",
        cell: ({ row }) => (
          <div className="flex items-center gap-3">
            <span className="flex size-8 shrink-0 items-center justify-center rounded-[5px] bg-sky-50 text-sky-700">
              <RadioTower className="size-4" />
            </span>
            <div className="min-w-0">
              <Link
                to={`/realtime/services/${row.original.id}`}
                className="font-medium text-emerald-800 hover:underline"
              >
                {row.original.name}
              </Link>
              <div className="max-w-56 truncate font-mono text-xs text-zinc-500">
                {row.original.id}
              </div>
            </div>
          </div>
        ),
      },
      {
        id: "project",
        accessorFn: (service) =>
          projectNames.get(service.project_id) ?? service.project_id,
        header: "プロジェクト",
        cell: ({ row }) =>
          projectNames.get(row.original.project_id) ?? (
            <span className="font-mono text-xs">{row.original.project_id}</span>
          ),
      },
      {
        accessorKey: "state",
        header: "状態",
        cell: ({ getValue }) => (
          <StatusBadge status={getValue<RealtimeService["state"]>()} />
        ),
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
        id: "trafficMode",
        accessorFn: (service) => service.spec.traffic_mode,
        header: "通信モード",
        cell: ({ row }) => {
          const mode = row.original.spec.traffic_mode;
          return (
            <Badge variant={mode === "direct" ? "warning" : "info"}>
              {trafficModeLabels[mode]}
            </Badge>
          );
        },
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
      {
        id: "details",
        header: "",
        enableSorting: false,
        cell: ({ row }) => (
          <Button asChild variant="ghost" size="icon">
            <Link
              to={`/realtime/services/${row.original.id}`}
              title={`${row.original.name}の詳細`}
              aria-label={`${row.original.name}の詳細`}
            >
              <ArrowRight />
            </Link>
          </Button>
        ),
      },
    ],
    [metricsByService, projectNames],
  );

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    createService.mutate(form);
  };

  const refresh = async () => {
    await queryClient.invalidateQueries({
      queryKey: ["organizations", organizationId, "realtime", "services"],
    });
  };

  if (services.isPending || projects.isPending) {
    return <PageLoading label="Flowを読み込んでいます" />;
  }

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
  const activeRooms = metricQueries.reduce(
    (sum, query) => sum + (query.data?.active_rooms ?? 0),
    0,
  );
  const connections = metricQueries.reduce(
    (sum, query) => sum + (query.data?.concurrent_connections ?? 0),
    0,
  );

  return (
    <div className="space-y-6">
      <PageHeader
        title="Flow"
        description={`${activeOrganization.organization_name} のWebRTC、LiveKit、STUN、TURN基盤を管理します。`}
        actions={
          <div className="flex items-center gap-2">
            <Button
              type="button"
              variant="secondary"
              size="icon"
              title="更新"
              aria-label="更新"
              onClick={() => void refresh()}
            >
              <RefreshCw />
            </Button>
            <Dialog
              open={createOpen}
              onOpenChange={(nextOpen) => {
                setCreateOpen(nextOpen);
                if (nextOpen) createService.reset();
              }}
            >
              <DialogTrigger asChild>
                <Button>
                  <Plus />
                  サービスを作成
                </Button>
              </DialogTrigger>
              <DialogContent className="max-w-xl">
                <DialogHeader>
                  <DialogTitle>Flowを作成</DialogTitle>
                  <DialogDescription>
                    プロジェクトと通信モードを指定します。
                  </DialogDescription>
                </DialogHeader>
                <RealtimeServiceForm
                  value={form}
                  onChange={setForm}
                  onSubmit={submit}
                  disabled={createService.isPending}
                >
                  <FormError
                    message={
                      createService.isError
                        ? getApiErrorMessage(createService.error)
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
                        createService.isPending ||
                        !form.projectId ||
                        !form.name.trim()
                      }
                    >
                      {createService.isPending ? (
                        <>
                          <LoaderCircle className="animate-spin" />
                          作成中
                        </>
                      ) : (
                        "作成"
                      )}
                    </Button>
                  </DialogFooter>
                </RealtimeServiceForm>
              </DialogContent>
            </Dialog>
          </div>
        }
      />

      <section
        className="grid border border-zinc-200 bg-white sm:grid-cols-3"
        aria-label="サービス稼働状況"
      >
        {[
          { label: "準備完了", value: readyCount, icon: RadioTower },
          { label: "アクティブルーム", value: activeRooms, icon: RadioTower },
          { label: "同時接続", value: connections, icon: UsersRound },
        ].map((item, index) => {
          const Icon = item.icon;
          return (
            <div
              key={item.label}
              className={`flex min-h-20 items-center gap-3 px-4 py-3 ${
                index > 0 ? "border-t border-zinc-200 sm:border-l sm:border-t-0" : ""
              }`}
            >
              <Icon className="size-4 text-zinc-500" />
              <div>
                <div className="text-xl font-semibold text-zinc-950">
                  {formatNumber(item.value)}
                </div>
                <div className="text-xs text-zinc-500">{item.label}</div>
              </div>
            </div>
          );
        })}
      </section>

      <DataTable
        columns={columns}
        data={serviceItems}
        getRowId={(service) => service.id}
        onRowClick={(service) => navigate(`/realtime/services/${service.id}`)}
        getRowAriaLabel={(service) => `${service.name}の詳細を開く`}
        searchPlaceholder="名前、プロジェクト、リージョン、状態で検索"
        emptyTitle="Flowがありません"
        emptyDescription="プロジェクトを選択してサービスを作成してください。"
      />
    </div>
  );
}
