import Box from "@cloudscape-design/components/box";
import Button from "@cloudscape-design/components/button";
import Cards from "@cloudscape-design/components/cards";
import ColumnLayout from "@cloudscape-design/components/column-layout";
import Container from "@cloudscape-design/components/container";
import Header from "@cloudscape-design/components/header";
import SpaceBetween from "@cloudscape-design/components/space-between";
import Table from "@cloudscape-design/components/table";
import { useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import { useMemo } from "react";
import { useNavigate } from "react-router-dom";
import { ErrorState } from "@/components/shared/error-state";
import { PageHeader } from "@/components/shared/page-header";
import { PageLoading } from "@/components/shared/page-loading";
import { RouterLink } from "@/components/shared/router-link";
import { StatusBadge } from "@/components/shared/status-badge";
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

const shortcuts = [
  { title: "Flow", description: "WebRTC、LiveKit、STUN、TURN", to: "/flow/services" },
  { title: "プロジェクト", description: "リソースの配置と分離", to: "/projects" },
  { title: "アクセス管理", description: "プリンシパル、ポリシー、権限", to: "/iam/principals" },
  { title: "監査ログ", description: "操作履歴と認可判定", to: "/audit-logs" },
];

export function OverviewPage() {
  const { activeOrganization } = useActiveOrganization();
  const organizationId = activeOrganization.organization_id;
  const queryClient = useQueryClient();
  const navigate = useNavigate();
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
        serviceItems.map((service, index) => [service.id, metricQueries[index]?.data]),
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
        onRetry={() => baseQueries.forEach((query) => void query.refetch())}
      />
    );
  }

  const projectItems = projects.data!.items;
  const principalItems = principals.data!.items;
  const policyItems = policies.data!.items;
  const auditItems = audit.data!.items;
  const readyServices = serviceItems.filter((service) => service.state === "ready");
  const activeRooms = metricQueries.reduce((sum, query) => sum + (query.data?.active_rooms ?? 0), 0);
  const concurrentConnections = metricQueries.reduce(
    (sum, query) => sum + (query.data?.concurrent_connections ?? 0),
    0,
  );
  const transferTotal = metricQueries.reduce(
    (sum, query) => sum + (query.data ? transferredBytes(query.data) : 0),
    0,
  );
  const resources = [
    ["プロジェクト", projectItems.length, "/projects"],
    ["Flow", serviceItems.length, "/flow/services"],
    ["IAMプリンシパル", principalItems.length, "/iam/principals"],
    ["IAMポリシー", policyItems.length, "/iam/policies"],
  ] as const;

  return (
    <SpaceBetween size="l">
      <PageHeader
        title="コンソールホーム"
        description={`${activeOrganization.organization_name} のリソース、稼働状況、最近の操作です。`}
        actions={
          <Button
            iconName="refresh"
            onClick={() =>
              void queryClient.invalidateQueries({ queryKey: ["organizations", organizationId] })
            }
          >
            更新
          </Button>
        }
      />
      <Container header={<Header variant="h2">リソース</Header>}>
        <ColumnLayout columns={4} variant="text-grid">
          {resources.map(([label, value, to]) => (
            <div key={label}>
              <Box variant="awsui-key-label">{label}</Box>
              <Box variant="awsui-value-large">{formatNumber(value)}</Box>
              <RouterLink to={to}>管理画面を開く</RouterLink>
            </div>
          ))}
        </ColumnLayout>
      </Container>
      <Cards
        cardDefinition={{
          header: (item) => <RouterLink to={item.to}>{item.title}</RouterLink>,
          sections: [{ id: "description", content: (item) => item.description }],
        }}
        cardsPerRow={[{ cards: 1 }, { minWidth: 500, cards: 2 }, { minWidth: 900, cards: 4 }]}
        items={shortcuts}
        header={<Header variant="h2">サービス</Header>}
        empty={<Box textAlign="center">利用可能なサービスがありません</Box>}
      />
      <Container header={<Header variant="h2">Flowの稼働状況</Header>}>
        <ColumnLayout columns={4} variant="text-grid">
          {[
            ["準備完了サービス", formatNumber(readyServices.length)],
            ["アクティブルーム", formatNumber(activeRooms)],
            ["同時接続", formatNumber(concurrentConnections)],
            ["転送量", formatBytes(transferTotal)],
          ].map(([label, value]) => (
            <div key={label}>
              <Box variant="awsui-key-label">{label}</Box>
              <Box variant="awsui-value-large">{value}</Box>
            </div>
          ))}
        </ColumnLayout>
      </Container>
      <ColumnLayout columns={2}>
        <Table
          variant="container"
          header={
            <Header
              variant="h2"
              counter={`(${serviceItems.length})`}
              actions={<Button onClick={() => navigate("/flow/services")}>すべて表示</Button>}
            >
              最近のFlow
            </Header>
          }
          items={serviceItems.slice(0, 6)}
          trackBy="id"
          columnDefinitions={[
            {
              id: "name",
              header: "サービス",
              cell: (item) => <RouterLink to={`/flow/services/${item.id}`}>{item.name}</RouterLink>,
            },
            { id: "state", header: "状態", cell: (item) => <StatusBadge status={item.state} /> },
            {
              id: "rooms",
              header: "ルーム",
              cell: (item) => formatNumber(metricsByService.get(item.id)?.active_rooms ?? 0),
            },
            {
              id: "connections",
              header: "同時接続",
              cell: (item) => formatNumber(metricsByService.get(item.id)?.concurrent_connections ?? 0),
            },
          ]}
          empty={<Box textAlign="center" color="text-body-secondary">サービスはありません。</Box>}
        />
        <Table
          variant="container"
          header={
            <Header
              variant="h2"
              counter={`(${auditItems.length})`}
              actions={<Button onClick={() => navigate("/audit-logs")}>すべて表示</Button>}
            >
              最近の監査イベント
            </Header>
          }
          items={auditItems.slice(0, 6)}
          trackBy={(item) => String(item.id)}
          columnDefinitions={[
            { id: "action", header: "アクション", cell: (item) => <Box variant="code">{item.action}</Box> },
            { id: "decision", header: "判定", cell: (item) => <StatusBadge status={item.decision} /> },
            { id: "time", header: "日時", cell: (item) => formatDateTime(item.occurred_at) },
          ]}
          empty={<Box textAlign="center" color="text-body-secondary">監査イベントはありません。</Box>}
        />
      </ColumnLayout>
    </SpaceBetween>
  );
}
