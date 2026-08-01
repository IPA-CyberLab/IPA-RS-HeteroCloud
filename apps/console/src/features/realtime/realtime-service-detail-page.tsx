import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ArrowDownToLine,
  ArrowLeft,
  ArrowUpFromLine,
  Gauge,
  Infinity as InfinityIcon,
  LoaderCircle,
  Pencil,
  RadioTower,
  RefreshCw,
  Trash2,
  UsersRound,
} from "lucide-react";
import { type FormEvent, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
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
import type { RealtimeService } from "@/lib/api-types";
import {
  projectsQueryOptions,
  realtimeServiceMetricsQueryOptions,
  realtimeServiceQueryOptions,
} from "@/lib/queries";
import { formatDateTime, formatNumber } from "@/lib/utils";
import { AccessCredentialDialog } from "./access-credential-dialog";
import { RealtimeEndpoints } from "./realtime-endpoints";
import {
  RealtimeServiceForm,
  type RealtimeServiceFormValue,
} from "./realtime-service-form";
import {
  formatBytes,
  serviceEndpoints,
  trafficModeLabels,
  transferredBytes,
} from "./realtime-service-utils";

function formValue(service: RealtimeService): RealtimeServiceFormValue {
  return {
    projectId: service.project_id,
    name: service.name,
    region: service.spec.region,
    trafficMode: service.spec.traffic_mode,
    maxParticipants: service.spec.max_participants,
    turnEnabled: service.spec.turn_enabled,
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
          traffic_mode: value.trafficMode,
          max_participants: value.maxParticipants,
          turn_enabled: value.turnEnabled,
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
      navigate("/realtime/services", { replace: true });
    },
  });

  const openEdit = () => {
    if (!service.data) return;
    setEditForm(formValue(service.data));
    updateService.reset();
    setEditOpen(true);
  };

  const submitEdit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (editForm) updateService.mutate(editForm);
  };

  const refresh = async () => {
    await Promise.all([service.refetch(), metrics.refetch()]);
  };

  if (!serviceId) {
    return (
      <ErrorState
        title="サービスを指定してください"
        description="サービスIDがURLに含まれていません。"
      />
    );
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
  const projectName =
    projects.data.items.find((project) => project.id === item.project_id)?.name ??
    item.project_id;
  const disabled = item.state === "deleting";

  const metricItems = [
    {
      label: "アクティブルーム",
      value: itemMetrics ? formatNumber(itemMetrics.active_rooms) : "—",
      icon: RadioTower,
    },
    {
      label: "同時接続",
      value: itemMetrics ? formatNumber(itemMetrics.concurrent_connections) : "—",
      icon: UsersRound,
    },
    {
      label: "Ingress",
      value: itemMetrics ? formatBytes(itemMetrics.ingress_bytes) : "—",
      icon: ArrowDownToLine,
    },
    {
      label: "Egress",
      value: itemMetrics ? formatBytes(itemMetrics.egress_bytes) : "—",
      icon: ArrowUpFromLine,
    },
    {
      label: "転送量",
      value: itemMetrics ? formatBytes(transferredBytes(itemMetrics)) : "—",
      icon: Gauge,
    },
  ];

  return (
    <div className="space-y-6">
      <Link
        to="/realtime/services"
        className="inline-flex items-center gap-1.5 text-sm font-medium text-emerald-800 hover:underline"
      >
        <ArrowLeft className="size-4" />
        リアルタイム通信サービス
      </Link>

      <PageHeader
        title={item.name}
        description={`サービスID: ${item.id}`}
        actions={
          <div className="flex flex-wrap items-center justify-end gap-2">
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
            <Button
              type="button"
              variant="secondary"
              disabled={disabled}
              onClick={openEdit}
            >
              <Pencil />
              編集
            </Button>
            <AccessCredentialDialog
              organizationId={organizationId}
              serviceId={item.id}
              serviceName={item.name}
              disabled={item.state !== "ready"}
            />
            <Dialog
              open={deleteOpen}
              onOpenChange={(nextOpen) => {
                setDeleteOpen(nextOpen);
                if (nextOpen) deleteService.reset();
              }}
            >
              <DialogTrigger asChild>
                <Button variant="destructive" disabled={disabled}>
                  <Trash2 />
                  削除
                </Button>
              </DialogTrigger>
              <DialogContent>
                <DialogHeader>
                  <DialogTitle>サービスを削除</DialogTitle>
                  <DialogDescription>
                    {item.name} と関連するルームおよび認証情報を削除します。
                  </DialogDescription>
                </DialogHeader>
                <div className="border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-900">
                  この操作は取り消せません。
                </div>
                <FormError
                  message={
                    deleteService.isError
                      ? getApiErrorMessage(deleteService.error)
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
                    type="button"
                    variant="destructive"
                    disabled={deleteService.isPending}
                    onClick={() => deleteService.mutate()}
                  >
                    {deleteService.isPending ? (
                      <>
                        <LoaderCircle className="animate-spin" />
                        削除中
                      </>
                    ) : (
                      <>
                        <Trash2 />
                        削除する
                      </>
                    )}
                  </Button>
                </DialogFooter>
              </DialogContent>
            </Dialog>
          </div>
        }
      />

      <section
        className="grid border border-zinc-200 bg-white sm:grid-cols-2 xl:grid-cols-5"
        aria-label="リアルタイムメトリクス"
      >
        {metricItems.map((metric, index) => {
          const Icon = metric.icon;
          return (
            <div
              key={metric.label}
              className={`min-h-24 px-4 py-4 ${
                index > 0
                  ? "border-t border-zinc-200 sm:border-l sm:border-t-0"
                  : ""
              } ${index === 2 ? "sm:border-t xl:border-t-0" : ""} ${
                index === 4 ? "sm:col-span-2 xl:col-span-1" : ""
              }`}
            >
              <div className="flex items-center gap-2 text-xs font-medium text-zinc-500">
                <Icon className="size-4" />
                {metric.label}
              </div>
              <div className="mt-2 text-xl font-semibold text-zinc-950">
                {metric.value}
              </div>
            </div>
          );
        })}
      </section>

      {metrics.isError ? (
        <div className="flex items-center justify-between border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-950">
          <span>リアルタイムメトリクスを取得できませんでした。</span>
          <Button variant="secondary" size="sm" onClick={() => void metrics.refetch()}>
            <RefreshCw />
            再試行
          </Button>
        </div>
      ) : null}

      <div className="grid gap-6 xl:grid-cols-[minmax(0,1.4fr)_minmax(22rem,0.8fr)]">
        <section className="overflow-hidden border border-zinc-200 bg-white">
          <div className="flex items-center justify-between border-b border-zinc-200 bg-zinc-50 px-4 py-3">
            <h2 className="text-sm font-semibold">実エンドポイント</h2>
            <span className="text-xs text-zinc-500">自動割り当て</span>
          </div>
          <RealtimeEndpoints endpoints={endpoints} />
        </section>

        <section className="overflow-hidden border border-zinc-200 bg-white">
          <div className="border-b border-zinc-200 bg-zinc-50 px-4 py-3">
            <h2 className="text-sm font-semibold">サービス設定</h2>
          </div>
          <dl className="divide-y divide-zinc-100 text-sm">
            <div className="flex items-center justify-between gap-4 px-4 py-3">
              <dt className="text-zinc-500">状態</dt>
              <dd><StatusBadge status={item.state} /></dd>
            </div>
            <div className="flex items-center justify-between gap-4 px-4 py-3">
              <dt className="text-zinc-500">プロジェクト</dt>
              <dd className="truncate font-medium">{projectName}</dd>
            </div>
            <div className="flex items-center justify-between gap-4 px-4 py-3">
              <dt className="text-zinc-500">リージョン</dt>
              <dd className="font-medium">{item.spec.region}</dd>
            </div>
            <div className="flex items-center justify-between gap-4 px-4 py-3">
              <dt className="text-zinc-500">通信モード</dt>
              <dd>
                <Badge
                  variant={item.spec.traffic_mode === "direct" ? "warning" : "info"}
                >
                  {trafficModeLabels[item.spec.traffic_mode]}
                </Badge>
              </dd>
            </div>
            <div className="flex items-center justify-between gap-4 px-4 py-3">
              <dt className="text-zinc-500">ルーム数</dt>
              <dd className="flex items-center gap-1.5 font-medium">
                <InfinityIcon className="size-4" />
                無制限
              </dd>
            </div>
            <div className="flex items-center justify-between gap-4 px-4 py-3">
              <dt className="text-zinc-500">同時参加者上限</dt>
              <dd className="font-medium">
                {formatNumber(item.spec.max_participants)}
              </dd>
            </div>
            <div className="flex items-center justify-between gap-4 px-4 py-3">
              <dt className="text-zinc-500">TURN</dt>
              <dd>
                <Badge variant={item.spec.turn_enabled ? "success" : "neutral"}>
                  {item.spec.turn_enabled ? "有効" : "無効"}
                </Badge>
              </dd>
            </div>
            <div className="flex items-center justify-between gap-4 px-4 py-3">
              <dt className="text-zinc-500">更新日時</dt>
              <dd className="font-medium">{formatDateTime(item.updated_at)}</dd>
            </div>
          </dl>
        </section>
      </div>

      <Dialog open={editOpen} onOpenChange={setEditOpen}>
        <DialogContent className="max-w-xl">
          <DialogHeader>
            <DialogTitle>サービス設定を編集</DialogTitle>
            <DialogDescription>{item.name}</DialogDescription>
          </DialogHeader>
          {editForm ? (
            <RealtimeServiceForm
              value={editForm}
              onChange={setEditForm}
              onSubmit={submitEdit}
              disabled={updateService.isPending}
              projectLocked
            >
              <FormError
                message={
                  updateService.isError
                    ? getApiErrorMessage(updateService.error)
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
                  disabled={updateService.isPending || !editForm.name.trim()}
                >
                  {updateService.isPending ? (
                    <>
                      <LoaderCircle className="animate-spin" />
                      更新中
                    </>
                  ) : (
                    "変更を保存"
                  )}
                </Button>
              </DialogFooter>
            </RealtimeServiceForm>
          ) : null}
        </DialogContent>
      </Dialog>
    </div>
  );
}
