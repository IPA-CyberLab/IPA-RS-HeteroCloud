import Alert from "@cloudscape-design/components/alert";
import Box from "@cloudscape-design/components/box";
import Button from "@cloudscape-design/components/button";
import Cards from "@cloudscape-design/components/cards";
import ColumnLayout from "@cloudscape-design/components/column-layout";
import Container from "@cloudscape-design/components/container";
import Header from "@cloudscape-design/components/header";
import SpaceBetween from "@cloudscape-design/components/space-between";
import StatusIndicator from "@cloudscape-design/components/status-indicator";
import Table from "@cloudscape-design/components/table";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import {
  consoleServices,
  readRecentServiceIds,
  subscribeToRecentServices,
  type ConsoleService,
} from "@/components/layout/service-catalog";
import { ServiceSearch } from "@/components/layout/service-search";
import { RouterLink } from "@/components/shared/router-link";
import { StatusBadge } from "@/components/shared/status-badge";
import { useActiveOrganization } from "@/features/organizations/organization-context";
import {
  auditEventsQueryOptions,
  flashServicesQueryOptions,
  realtimeServicesQueryOptions,
  registryImagesQueryOptions,
  syouyuBucketsQueryOptions,
} from "@/lib/queries";
import { formatDateTime, formatNumber } from "@/lib/utils";

function ServiceCards({
  items,
  title,
  empty,
}: {
  items: ConsoleService[];
  title: string;
  empty: string;
}) {
  return (
    <Cards
      cardDefinition={{
        header: (service) => (
          <RouterLink to={service.href}>{service.name}</RouterLink>
        ),
        sections: [
          {
            id: "group",
            header: "サービスグループ",
            content: (service) => service.group,
          },
          {
            id: "summary",
            header: "用途",
            content: (service) => service.shortName,
          },
          {
            id: "description",
            content: (service) => service.description,
          },
        ],
      }}
      cardsPerRow={[
        { cards: 1 },
        { minWidth: 520, cards: 2 },
        { minWidth: 960, cards: 4 },
      ]}
      items={items}
      trackBy="id"
      header={<Header variant="h2">{title}</Header>}
      empty={
        <Box textAlign="center" color="text-body-secondary" padding="l">
          {empty}
        </Box>
      }
    />
  );
}

function ResourceStatus({
  name,
  href,
  pending,
  error,
  value,
  detail,
}: {
  name: string;
  href: string;
  pending: boolean;
  error: boolean;
  value?: number;
  detail: string;
}) {
  return (
    <SpaceBetween size="xxs">
      <RouterLink to={href}>{name}</RouterLink>
      {pending ? (
        <StatusIndicator type="loading">読み込み中</StatusIndicator>
      ) : error ? (
        <StatusIndicator type="error">取得できません</StatusIndicator>
      ) : (
        <>
          <Box variant="awsui-value-large">{formatNumber(value ?? 0)}</Box>
          <StatusIndicator type="success">{detail}</StatusIndicator>
        </>
      )}
    </SpaceBetween>
  );
}

export function OverviewPage() {
  const { activeOrganization } = useActiveOrganization();
  const organizationId = activeOrganization.organization_id;
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [recentIds, setRecentIds] = useState(readRecentServiceIds);

  useEffect(
    () =>
      subscribeToRecentServices(() => {
        setRecentIds(readRecentServiceIds());
      }),
    [],
  );

  const flash = useQuery(flashServicesQueryOptions(organizationId));
  const registry = useQuery(registryImagesQueryOptions(organizationId));
  const flow = useQuery(realtimeServicesQueryOptions(organizationId));
  const syouyu = useQuery(syouyuBucketsQueryOptions(organizationId));
  const audit = useQuery(auditEventsQueryOptions(organizationId));
  const queries = [flash, registry, flow, syouyu, audit];

  const recentServices = useMemo(
    () =>
      recentIds
        .map((id) => consoleServices.find((service) => service.id === id))
        .filter((service): service is ConsoleService => Boolean(service)),
    [recentIds],
  );
  const failedCount = queries.filter((query) => query.isError).length;
  const flashItems = flash.data?.items ?? [];
  const flowItems = flow.data?.items ?? [];
  const auditItems = audit.data?.items ?? [];
  const readyFlash = flashItems.filter((service) => service.state === "ready").length;
  const readyFlow = flowItems.filter((service) => service.state === "ready").length;

  return (
    <SpaceBetween size="l">
      <section className="console-home__hero" aria-labelledby="console-home-title">
        <div className="console-home__hero-content">
          <Box variant="small">{activeOrganization.organization_name}</Box>
          <h1 id="console-home-title">何を構築しますか？</h1>
          <p>サービスを検索するか、よく使うサービスから始められます。</p>
          <div className="console-home__search">
            <ServiceSearch
              ariaLabel="コンソールホームからサービスを検索"
              placeholder="Flash、Registry、GPU、ストレージなどを検索"
            />
          </div>
        </div>
      </section>

      {failedCount > 0 ? (
        <Alert
          type="warning"
          header="一部の状態を取得できませんでした"
          action={
            <Button
              onClick={() =>
                void queryClient.invalidateQueries({
                  queryKey: ["organizations", organizationId],
                })
              }
            >
              再試行
            </Button>
          }
        >
          利用できるサービスへの移動はそのまま行えます。{failedCount}件の状態取得を再試行してください。
        </Alert>
      ) : null}

      <ServiceCards
        items={recentServices}
        title="最近使ったサービス"
        empty="まだ利用履歴がありません。サービスを開くとここに表示されます。"
      />

      <ServiceCards
        items={consoleServices}
        title="主要サービス"
        empty="利用可能なサービスがありません。"
      />

      <Container
        header={
          <Header
            variant="h2"
            description="選択中の組織で利用しているリソースの状態です。"
            actions={
              <Button
                iconName="refresh"
                onClick={() =>
                  void queryClient.invalidateQueries({
                    queryKey: ["organizations", organizationId],
                  })
                }
              >
                更新
              </Button>
            }
          >
            状態サマリー
          </Header>
        }
      >
        <ColumnLayout columns={4} variant="text-grid">
          <ResourceStatus
            name="Flash"
            href="/flash/services"
            pending={flash.isPending}
            error={flash.isError}
            value={flashItems.length}
            detail={`${readyFlash}件が準備完了`}
          />
          <ResourceStatus
            name="Flash Registry"
            href="/registry"
            pending={registry.isPending}
            error={registry.isError}
            value={registry.data?.items.length}
            detail="イメージ"
          />
          <ResourceStatus
            name="Flow"
            href="/flow/services"
            pending={flow.isPending}
            error={flow.isError}
            value={flowItems.length}
            detail={`${readyFlow}件が準備完了`}
          />
          <ResourceStatus
            name="Syouyu"
            href="/syouyu/buckets"
            pending={syouyu.isPending}
            error={syouyu.isError}
            value={syouyu.data?.items.length}
            detail="バケット"
          />
        </ColumnLayout>
      </Container>

      <ColumnLayout columns={2}>
        <Table
          variant="container"
          loading={flash.isPending}
          loadingText="Flashサービスを読み込んでいます"
          header={
            <Header
              variant="h2"
              counter={flash.isSuccess ? `(${flashItems.length})` : undefined}
              actions={<Button onClick={() => navigate("/flash/services")}>すべて表示</Button>}
            >
              最近のFlash
            </Header>
          }
          items={flashItems.slice(0, 5)}
          trackBy="id"
          columnDefinitions={[
            {
              id: "name",
              header: "サービス",
              cell: (item) => (
                <RouterLink to={`/flash/services/${item.id}`}>{item.name}</RouterLink>
              ),
            },
            {
              id: "state",
              header: "状態",
              cell: (item) => <StatusBadge status={item.state} />,
            },
            {
              id: "updated",
              header: "更新日時",
              cell: (item) => formatDateTime(item.updated_at),
            },
          ]}
          empty={
            <Box textAlign="center" color="text-body-secondary">
              Flashサービスはありません。
            </Box>
          }
        />
        <Table
          variant="container"
          loading={audit.isPending}
          loadingText="監査イベントを読み込んでいます"
          header={
            <Header
              variant="h2"
              counter={audit.isSuccess ? `(${auditItems.length})` : undefined}
              actions={<Button onClick={() => navigate("/audit-logs")}>すべて表示</Button>}
            >
              最近の操作
            </Header>
          }
          items={auditItems.slice(0, 5)}
          trackBy={(item) => String(item.id)}
          columnDefinitions={[
            {
              id: "action",
              header: "アクション",
              cell: (item) => <Box variant="code">{item.action}</Box>,
            },
            {
              id: "decision",
              header: "判定",
              cell: (item) => <StatusBadge status={item.decision} />,
            },
            {
              id: "time",
              header: "日時",
              cell: (item) => formatDateTime(item.occurred_at),
            },
          ]}
          empty={
            <Box textAlign="center" color="text-body-secondary">
              監査イベントはありません。
            </Box>
          }
        />
      </ColumnLayout>
    </SpaceBetween>
  );
}
