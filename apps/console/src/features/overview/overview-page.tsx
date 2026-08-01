import { useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Activity,
  ArrowRight,
  Boxes,
  FileClock,
  KeyRound,
  RadioTower,
  RefreshCw,
  ShieldCheck,
  Users,
  UsersRound,
} from "lucide-react";
import { useMemo } from "react";
import { Link } from "react-router-dom";
import { ErrorState } from "@/components/shared/error-state";
import { PageHeader } from "@/components/shared/page-header";
import { PageLoading } from "@/components/shared/page-loading";
import { StatusBadge } from "@/components/shared/status-badge";
import { Button } from "@/components/ui/button";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { useActiveOrganization } from "@/features/organizations/organization-context";
import { formatBytes, transferredBytes } from "@/features/realtime/realtime-service-utils";
import {
  auditEventsQueryOptions,
  iamPoliciesQueryOptions,
  iamPrincipalsQueryOptions,
  projectsQueryOptions,
  realtimeServiceMetricsQueryOptions,
  realtimeServicesQueryOptions,
} from "@/lib/queries";
import { formatDateTime, formatNumber } from "@/lib/utils";

export function OverviewPage() {
  const { activeOrganization } = useActiveOrganization();
  const organizationId = activeOrganization.organization_id;
  const queryClient = useQueryClient();
  const projects = useQuery(projectsQueryOptions(organizationId));
  const principals = useQuery(iamPrincipalsQueryOptions(organizationId));
  const policies = useQuery(iamPoliciesQueryOptions(organizationId));
  const services = useQuery(realtimeServicesQueryOptions(organizationId));
  const audit = useQuery(auditEventsQueryOptions(organizationId));
  const baseQueries = [projects, principals, policies, services, audit];
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

  if (baseQueries.some((query) => query.isPending)) {
    return <PageLoading label="コンソールを読み込んでいます" />;
  }

  if (baseQueries.some((query) => query.isError)) {
    return (
      <ErrorState
        description="選択中の組織からコンソールデータを取得できませんでした。"
        onRetry={() => {
          baseQueries.forEach((query) => void query.refetch());
        }}
      />
    );
  }

  const projectItems = projects.data!.items;
  const principalItems = principals.data!.items;
  const policyItems = policies.data!.items;
  const auditItems = audit.data!.items;
  const readyServices = serviceItems.filter((service) => service.state === "ready");
  const activeRooms = metricQueries.reduce(
    (sum, query) => sum + (query.data?.active_rooms ?? 0),
    0,
  );
  const concurrentConnections = metricQueries.reduce(
    (sum, query) => sum + (query.data?.concurrent_connections ?? 0),
    0,
  );
  const transferTotal = metricQueries.reduce(
    (sum, query) => sum + (query.data ? transferredBytes(query.data) : 0),
    0,
  );

  const resources = [
    {
      label: "プロジェクト",
      value: projectItems.length,
      icon: Boxes,
      to: "/projects",
    },
    {
      label: "Flow",
      value: serviceItems.length,
      icon: RadioTower,
      to: "/realtime/services",
    },
    {
      label: "IAMプリンシパル",
      value: principalItems.length,
      icon: Users,
      to: "/iam/principals",
    },
    {
      label: "IAMポリシー",
      value: policyItems.length,
      icon: ShieldCheck,
      to: "/iam/policies",
    },
  ];

  const serviceLinks = [
    {
      title: "Flow",
      description: "WebRTC、LiveKit、STUN、TURN",
      to: "/realtime/services",
      icon: RadioTower,
    },
    {
      title: "プロジェクト",
      description: "リソースの配置と分離",
      to: "/projects",
      icon: Boxes,
    },
    {
      title: "アクセス管理",
      description: "プリンシパル、ポリシー、権限",
      to: "/iam/principals",
      icon: KeyRound,
    },
    {
      title: "監査ログ",
      description: "操作履歴と認可判定",
      to: "/audit-logs",
      icon: FileClock,
    },
  ];

  const refresh = async () => {
    await queryClient.invalidateQueries({
      queryKey: ["organizations", organizationId],
    });
  };

  return (
    <div className="space-y-7">
      <PageHeader
        title="コンソールホーム"
        description={`${activeOrganization.organization_name} のリソース、稼働状況、最近の操作です。`}
        actions={
          <Button variant="secondary" onClick={() => void refresh()}>
            <RefreshCw />
            更新
          </Button>
        }
      />

      <section aria-labelledby="resources-heading">
        <div className="mb-3 flex items-center justify-between">
          <h2 id="resources-heading" className="text-sm font-semibold text-zinc-900">
            リソース
          </h2>
        </div>
        <div className="grid border border-zinc-200 bg-white sm:grid-cols-2 xl:grid-cols-4">
          {resources.map((resource, index) => {
            const Icon = resource.icon;
            return (
              <Link
                key={resource.label}
                to={resource.to}
                className={`group flex min-h-24 items-center gap-4 p-4 outline-none hover:bg-zinc-50 focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-emerald-600 ${
                  index > 0 ? "border-t border-zinc-200 sm:border-t-0" : ""
                } ${index % 2 === 1 ? "sm:border-l" : ""} ${
                  index > 1 ? "xl:border-l" : ""
                }`}
              >
                <span className="flex size-9 shrink-0 items-center justify-center rounded-[6px] bg-zinc-100 text-zinc-600 group-hover:bg-emerald-50 group-hover:text-emerald-700">
                  <Icon className="size-4" />
                </span>
                <span className="min-w-0">
                  <span className="block text-xl font-semibold text-zinc-950">
                    {formatNumber(resource.value)}
                  </span>
                  <span className="block truncate text-xs text-zinc-500">
                    {resource.label}
                  </span>
                </span>
              </Link>
            );
          })}
        </div>
      </section>

      <section aria-labelledby="services-heading">
        <h2 id="services-heading" className="mb-3 text-sm font-semibold text-zinc-900">
          サービス
        </h2>
        <div className="grid border border-zinc-200 bg-white md:grid-cols-2 xl:grid-cols-4">
          {serviceLinks.map((serviceLink, index) => {
            const Icon = serviceLink.icon;
            return (
              <Link
                key={serviceLink.to}
                to={serviceLink.to}
                className={`group flex min-h-20 items-center gap-3 px-4 py-3 outline-none hover:bg-zinc-50 focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-emerald-600 ${
                  index > 0 ? "border-t border-zinc-200 md:border-t-0" : ""
                } ${index % 2 === 1 ? "md:border-l" : ""} ${
                  index > 1 ? "xl:border-l" : ""
                }`}
              >
                <Icon className="size-5 shrink-0 text-zinc-500 group-hover:text-emerald-700" />
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-sm font-medium text-zinc-900">
                    {serviceLink.title}
                  </span>
                  <span className="block truncate text-xs text-zinc-500">
                    {serviceLink.description}
                  </span>
                </span>
                <ArrowRight className="size-4 shrink-0 text-zinc-400" />
              </Link>
            );
          })}
        </div>
      </section>

      <section aria-labelledby="operations-heading">
        <h2 id="operations-heading" className="mb-3 text-sm font-semibold text-zinc-900">
          Flowの稼働状況
        </h2>
        <div className="grid border border-zinc-200 bg-white sm:grid-cols-2 xl:grid-cols-4">
          {[
            {
              label: "準備完了サービス",
              value: formatNumber(readyServices.length),
              icon: RadioTower,
            },
            {
              label: "アクティブルーム",
              value: formatNumber(activeRooms),
              icon: Activity,
            },
            {
              label: "同時接続",
              value: formatNumber(concurrentConnections),
              icon: UsersRound,
            },
            {
              label: "転送量",
              value: formatBytes(transferTotal),
              icon: Activity,
            },
          ].map((metric, index) => {
            const Icon = metric.icon;
            return (
              <div
                key={metric.label}
                className={`min-h-20 px-4 py-3 ${
                  index > 0 ? "border-t border-zinc-200 sm:border-t-0" : ""
                } ${index % 2 === 1 ? "sm:border-l" : ""} ${
                  index > 1 ? "xl:border-l" : ""
                }`}
              >
                <div className="flex items-center gap-2 text-xs text-zinc-500">
                  <Icon className="size-3.5" />
                  {metric.label}
                </div>
                <div className="mt-2 text-xl font-semibold text-zinc-950">
                  {metric.value}
                </div>
              </div>
            );
          })}
        </div>
      </section>

      <div className="grid gap-6 xl:grid-cols-[minmax(0,1.35fr)_minmax(22rem,0.8fr)]">
        <section className="overflow-hidden border border-zinc-200 bg-white">
          <div className="flex items-center justify-between border-b border-zinc-200 bg-zinc-50 px-4 py-3">
            <h2 className="text-sm font-semibold">最近のFlow</h2>
            <Button asChild variant="ghost" size="sm">
              <Link to="/realtime/services">すべて表示</Link>
            </Button>
          </div>
          {serviceItems.length === 0 ? (
            <div className="flex min-h-44 items-center justify-center px-4 text-sm text-zinc-500">
              サービスはありません。
            </div>
          ) : (
            <Table>
              <TableHeader>
                <TableRow className="hover:bg-transparent">
                  <TableHead>サービス</TableHead>
                  <TableHead>状態</TableHead>
                  <TableHead>ルーム</TableHead>
                  <TableHead>同時接続</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {serviceItems.slice(0, 6).map((serviceItem) => {
                  const itemMetrics = metricsByService.get(serviceItem.id);
                  return (
                    <TableRow key={serviceItem.id}>
                      <TableCell>
                        <Link
                          to={`/realtime/services/${serviceItem.id}`}
                          className="font-medium text-emerald-800 hover:underline"
                        >
                          {serviceItem.name}
                        </Link>
                      </TableCell>
                      <TableCell><StatusBadge status={serviceItem.state} /></TableCell>
                      <TableCell>
                        {itemMetrics ? formatNumber(itemMetrics.active_rooms) : "—"}
                      </TableCell>
                      <TableCell>
                        {itemMetrics
                          ? formatNumber(itemMetrics.concurrent_connections)
                          : "—"}
                      </TableCell>
                    </TableRow>
                  );
                })}
              </TableBody>
            </Table>
          )}
        </section>

        <section className="overflow-hidden border border-zinc-200 bg-white">
          <div className="flex items-center justify-between border-b border-zinc-200 bg-zinc-50 px-4 py-3">
            <h2 className="text-sm font-semibold">最近の監査イベント</h2>
            <Button asChild variant="ghost" size="sm">
              <Link to="/audit-logs">すべて表示</Link>
            </Button>
          </div>
          {auditItems.length === 0 ? (
            <div className="flex min-h-44 items-center justify-center px-4 text-sm text-zinc-500">
              監査イベントはありません。
            </div>
          ) : (
            <div className="divide-y divide-zinc-100">
              {auditItems.slice(0, 6).map((event) => (
                <div key={event.id} className="flex items-center gap-3 px-4 py-3">
                  <Activity className="size-4 shrink-0 text-zinc-400" />
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-sm font-medium text-zinc-800">
                      {event.action}
                    </p>
                    <p className="truncate text-xs text-zinc-500">
                      {formatDateTime(event.occurred_at)}
                    </p>
                  </div>
                  <StatusBadge status={event.decision} />
                </div>
              ))}
            </div>
          )}
        </section>
      </div>
    </div>
  );
}
