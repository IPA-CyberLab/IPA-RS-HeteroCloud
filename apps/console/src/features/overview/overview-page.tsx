import { useQuery } from "@tanstack/react-query";
import {
  Activity,
  Boxes,
  Building2,
  KeyRound,
  Users,
  Workflow,
} from "lucide-react";
import { Link } from "react-router-dom";
import { ErrorState } from "@/components/shared/error-state";
import { PageHeader } from "@/components/shared/page-header";
import { PageLoading } from "@/components/shared/page-loading";
import { StatusBadge } from "@/components/shared/status-badge";
import { Button } from "@/components/ui/button";
import { useActiveOrganization } from "@/features/organizations/organization-context";
import {
  auditEventsQueryOptions,
  flowInstancesQueryOptions,
  iamPoliciesQueryOptions,
  iamPrincipalsQueryOptions,
  projectsQueryOptions,
} from "@/lib/queries";
import { formatDateTime, formatNumber } from "@/lib/utils";

export function OverviewPage() {
  const { activeOrganization, memberships } = useActiveOrganization();
  const organizationId = activeOrganization.organization_id;
  const projects = useQuery(projectsQueryOptions(organizationId));
  const principals = useQuery(iamPrincipalsQueryOptions(organizationId));
  const policies = useQuery(iamPoliciesQueryOptions(organizationId));
  const flow = useQuery(flowInstancesQueryOptions(organizationId));
  const audit = useQuery(auditEventsQueryOptions(organizationId));
  const queries = [projects, principals, policies, flow, audit];

  if (queries.some((query) => query.isPending)) {
    return <PageLoading label="概要を読み込んでいます" />;
  }

  if (queries.some((query) => query.isError)) {
    return (
      <ErrorState
        description="選択中の組織から概要データを取得できませんでした。"
        onRetry={() => {
          queries.forEach((query) => void query.refetch());
        }}
      />
    );
  }

  const projectItems = projects.data!.items;
  const principalItems = principals.data!.items;
  const policyItems = policies.data!.items;
  const flowItems = flow.data!.items;
  const auditItems = audit.data!.items;
  const metrics = [
    {
      label: "参加組織",
      value: memberships.length,
      icon: Building2,
      to: "/organizations",
    },
    {
      label: "プロジェクト",
      value: projectItems.length,
      icon: Boxes,
      to: "/projects",
    },
    {
      label: "稼働可能なFlow",
      value: flowItems.filter((instance) => instance.state === "ready").length,
      icon: Workflow,
      to: "/flow/instances",
    },
    {
      label: "IAMプリンシパル",
      value: principalItems.length,
      icon: Users,
      to: "/iam/principals",
    },
  ];

  const flowStates = Array.from(
    flowItems.reduce((counts, instance) => {
      counts.set(instance.state, (counts.get(instance.state) ?? 0) + 1);
      return counts;
    }, new Map<string, number>()),
  );
  const userPrincipalCount = principalItems.filter(
    (principal) => principal.kind === "user",
  ).length;

  return (
    <div className="space-y-8">
      <PageHeader
        title="概要"
        description={`${activeOrganization.organization_name} のリソースと直近のIAM判定を確認します。`}
        actions={
          <Button asChild>
            <Link to="/flow/instances">
              <Workflow />
              Flowを管理
            </Link>
          </Button>
        }
      />

      <section aria-labelledby="resource-summary-heading">
        <h2
          id="resource-summary-heading"
          className="mb-3 text-sm font-semibold text-zinc-900"
        >
          リソース
        </h2>
        <div className="grid border border-zinc-200 bg-white sm:grid-cols-2 xl:grid-cols-4">
          {metrics.map((metric, index) => {
            const Icon = metric.icon;
            return (
              <Link
                key={metric.label}
                to={metric.to}
                className={`group flex min-h-28 items-center gap-4 p-5 outline-none hover:bg-zinc-50 focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-emerald-600 ${
                  index > 0 ? "border-t border-zinc-200 sm:border-t-0" : ""
                } ${index % 2 === 1 ? "sm:border-l" : ""} ${
                  index > 1 ? "xl:border-l" : ""
                }`}
              >
                <span className="flex size-10 shrink-0 items-center justify-center rounded-[6px] bg-zinc-100 text-zinc-600 group-hover:bg-emerald-50 group-hover:text-emerald-700">
                  <Icon className="size-5" />
                </span>
                <span>
                  <span className="block text-2xl font-semibold text-zinc-950">
                    {formatNumber(metric.value)}
                  </span>
                  <span className="block text-sm text-zinc-500">
                    {metric.label}
                  </span>
                </span>
              </Link>
            );
          })}
        </div>
      </section>

      <div className="grid gap-6 xl:grid-cols-[minmax(0,1.5fr)_minmax(20rem,0.8fr)]">
        <section
          className="border border-zinc-200 bg-white"
          aria-labelledby="recent-events-heading"
        >
          <div className="flex items-center justify-between border-b border-zinc-200 px-5 py-4">
            <div>
              <h2 id="recent-events-heading" className="text-sm font-semibold">
                最近の監査イベント
              </h2>
              <p className="mt-0.5 text-xs text-zinc-500">
                認可された操作と拒否された操作
              </p>
            </div>
            <Button asChild variant="ghost" size="sm">
              <Link to="/audit-logs">すべて表示</Link>
            </Button>
          </div>
          {auditItems.length === 0 ? (
            <div className="flex min-h-48 items-center justify-center px-5 text-sm text-zinc-500">
              監査イベントはまだありません。
            </div>
          ) : (
            <div className="divide-y divide-zinc-100">
              {auditItems.slice(0, 6).map((event) => (
                <div
                  key={event.id}
                  className="flex flex-col gap-2 px-5 py-3 sm:flex-row sm:items-center"
                >
                  <span className="flex size-8 shrink-0 items-center justify-center rounded-full bg-zinc-100 text-zinc-500">
                    <Activity className="size-4" />
                  </span>
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-sm font-medium text-zinc-800">
                      {event.action}
                    </p>
                    <p className="truncate font-mono text-xs text-zinc-500">
                      {event.resource}
                    </p>
                  </div>
                  <div className="flex items-center gap-3 sm:justify-end">
                    <StatusBadge status={event.decision} />
                    <span className="whitespace-nowrap text-xs text-zinc-500">
                      {formatDateTime(event.occurred_at)}
                    </span>
                  </div>
                </div>
              ))}
            </div>
          )}
        </section>

        <div className="space-y-6">
          <section
            className="border border-zinc-200 bg-white"
            aria-labelledby="flow-state-heading"
          >
            <div className="border-b border-zinc-200 px-5 py-4">
              <div className="flex items-center gap-2">
                <Workflow className="size-4 text-zinc-500" />
                <h2 id="flow-state-heading" className="text-sm font-semibold">
                  Flowの状態
                </h2>
              </div>
            </div>
            {flowStates.length === 0 ? (
              <p className="px-5 py-8 text-center text-sm text-zinc-500">
                Flowインスタンスはありません。
              </p>
            ) : (
              <div className="divide-y divide-zinc-100">
                {flowStates.map(([state, count]) => (
                  <div
                    key={state}
                    className="flex items-center justify-between px-5 py-3"
                  >
                    <StatusBadge status={state} />
                    <span className="text-sm font-semibold">
                      {formatNumber(count)}
                    </span>
                  </div>
                ))}
              </div>
            )}
          </section>

          <section
            className="border border-zinc-200 bg-white p-5"
            aria-labelledby="iam-summary-heading"
          >
            <div className="flex items-center gap-2">
              <KeyRound className="size-4 text-zinc-500" />
              <h2 id="iam-summary-heading" className="text-sm font-semibold">
                IAM
              </h2>
            </div>
            <dl className="mt-4 divide-y divide-zinc-100 text-sm">
              <div className="flex justify-between py-2">
                <dt className="text-zinc-500">ユーザープリンシパル</dt>
                <dd className="font-semibold">{userPrincipalCount}</dd>
              </div>
              <div className="flex justify-between py-2">
                <dt className="text-zinc-500">サービスアカウント</dt>
                <dd className="font-semibold">
                  {principalItems.length - userPrincipalCount}
                </dd>
              </div>
              <div className="flex justify-between py-2">
                <dt className="text-zinc-500">ポリシー</dt>
                <dd className="font-semibold">{policyItems.length}</dd>
              </div>
            </dl>
          </section>
        </div>
      </div>
    </div>
  );
}
