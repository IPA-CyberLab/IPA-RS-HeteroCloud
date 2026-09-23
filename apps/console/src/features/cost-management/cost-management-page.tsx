import Badge from "@cloudscape-design/components/badge";
import Box from "@cloudscape-design/components/box";
import Button from "@cloudscape-design/components/button";
import ColumnLayout from "@cloudscape-design/components/column-layout";
import Container from "@cloudscape-design/components/container";
import Header from "@cloudscape-design/components/header";
import ProgressBar from "@cloudscape-design/components/progress-bar";
import SpaceBetween from "@cloudscape-design/components/space-between";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import type { ColumnDef } from "@tanstack/react-table";
import { useMemo } from "react";
import { DataTable } from "@/components/shared/data-table";
import { ErrorState } from "@/components/shared/error-state";
import { PageHeader } from "@/components/shared/page-header";
import { PageLoading } from "@/components/shared/page-loading";
import { RouterLink } from "@/components/shared/router-link";
import { useActiveOrganization } from "@/features/organizations/organization-context";
import type {
  FlashCurrentAllocation,
  FlashQuotaLimits,
  FlashRuntimeUsage,
  FlashUsageService,
  OwnerFlashCostTenant,
} from "@/lib/api-types";
import {
  flashCostManagementQueryOptions,
  ownerFlashCostManagementQueryOptions,
} from "@/lib/queries";
import { formatDateTime, formatNumber } from "@/lib/utils";

const CPU_MILLICORE_SECONDS_PER_VCPU_HOUR = 1_000 * 60 * 60;
const MEMORY_MIB_SECONDS_PER_GIB_HOUR = 1_024 * 60 * 60;
const SECONDS_PER_HOUR = 60 * 60;

function formatDecimal(value: number, maximumFractionDigits = 2) {
  return value.toLocaleString("ja-JP", { maximumFractionDigits });
}

export function formatCpuHours(value: number) {
  return `${formatDecimal(value / CPU_MILLICORE_SECONDS_PER_VCPU_HOUR)} vCPU 時間`;
}

export function formatMemoryHours(value: number) {
  return `${formatDecimal(value / MEMORY_MIB_SECONDS_PER_GIB_HOUR)} GiB 時間`;
}

export function formatGpuHours(value: number) {
  return `${formatDecimal(value / SECONDS_PER_HOUR)} GPU 時間`;
}

function formatUnixTime(value: number) {
  return formatDateTime(new Date(value * 1_000).toISOString());
}

function usagePercentage(used: number, limit: number) {
  if (limit === 0) return used === 0 ? 0 : 100;
  return Math.min(100, (used / limit) * 100);
}

function RuntimeMetric({
  label,
  used,
  limit,
  format,
}: {
  label: string;
  used: number;
  limit: number;
  format: (value: number) => string;
}) {
  return (
    <ProgressBar
      value={Math.round(usagePercentage(used, limit) * 100) / 100}
      label={label}
      description={`${format(used)} / ${format(limit)}`}
      status={used >= limit ? "error" : "in-progress"}
      additionalInfo={used >= limit ? "上限に達しています" : undefined}
    />
  );
}

function RuntimeUsagePanel({
  usage,
  limits,
}: {
  usage: FlashRuntimeUsage;
  limits: FlashQuotaLimits;
}) {
  return (
    <Container
      header={
        <Header variant="h2" description="起動中レプリカに割り当てたリソース量と稼働時間の積です。">
          今週のFlash使用量
        </Header>
      }
    >
      <ColumnLayout columns={3} variant="text-grid">
        <RuntimeMetric
          label="CPU 実行時間"
          used={usage.cpu_millicore_seconds}
          limit={limits.max_weekly_cpu_millicore_seconds}
          format={formatCpuHours}
        />
        <RuntimeMetric
          label="メモリ実行時間"
          used={usage.memory_mib_seconds}
          limit={limits.max_weekly_memory_mib_seconds}
          format={formatMemoryHours}
        />
        <RuntimeMetric
          label="GPU 実行時間"
          used={usage.gpu_seconds}
          limit={limits.max_weekly_gpu_seconds}
          format={formatGpuHours}
        />
      </ColumnLayout>
    </Container>
  );
}

function CurrentAllocationPanel({ current }: { current: FlashCurrentAllocation }) {
  return (
    <Container header={<Header variant="h2">現在の割当</Header>}>
      <ColumnLayout columns={5} variant="text-grid">
        <div>
          <Box variant="awsui-key-label">サービス</Box>
          <Box>{formatNumber(current.active_services)}</Box>
        </div>
        <div>
          <Box variant="awsui-key-label">Ready レプリカ</Box>
          <Box>{formatNumber(current.ready_replicas)}</Box>
        </div>
        <div>
          <Box variant="awsui-key-label">CPU</Box>
          <Box>{formatDecimal(current.cpu_millis / 1_000)} vCPU</Box>
        </div>
        <div>
          <Box variant="awsui-key-label">メモリ</Box>
          <Box>{formatDecimal(current.memory_mib / 1_024)} GiB</Box>
        </div>
        <div>
          <Box variant="awsui-key-label">GPU</Box>
          <Box>{formatNumber(current.gpus)}</Box>
        </div>
      </ColumnLayout>
    </Container>
  );
}

function serviceColumns(): ColumnDef<FlashUsageService, unknown>[] {
  return [
    {
      id: "service",
      header: "サービス",
      accessorFn: (service) =>
        `${service.display_name} ${service.service_instance_id}`,
      cell: ({ row }) => (
        <SpaceBetween size="xxs">
          {row.original.active ? (
            <RouterLink
              to={`/flash/services/${encodeURIComponent(row.original.service_instance_id)}`}
            >
              {row.original.display_name}
            </RouterLink>
          ) : (
            <Box>{row.original.display_name}</Box>
          )}
          <Box color="text-body-secondary">
            {row.original.service_instance_id}
          </Box>
        </SpaceBetween>
      ),
    },
    {
      id: "state",
      header: "状態",
      accessorFn: (service) => (service.active ? "active" : "deleted"),
      cell: ({ row }) => (
        <Badge color={row.original.active ? "green" : "grey"}>
          {row.original.active ? "有効" : "今週削除済み"}
        </Badge>
      ),
    },
    {
      id: "ready",
      header: "現在",
      accessorFn: (service) => service.ready_replicas,
      cell: ({ row }) =>
        `${formatNumber(row.original.ready_replicas)} replicas · ${formatDecimal((row.original.ready_replicas * row.original.cpu_millis) / 1_000)} vCPU · ${formatDecimal((row.original.ready_replicas * row.original.memory_mib) / 1_024)} GiB`,
    },
    {
      id: "cpu",
      header: "CPU 実行時間",
      accessorFn: (service) => service.weekly_usage.cpu_millicore_seconds,
      cell: ({ row }) =>
        formatCpuHours(row.original.weekly_usage.cpu_millicore_seconds),
    },
    {
      id: "memory",
      header: "メモリ実行時間",
      accessorFn: (service) => service.weekly_usage.memory_mib_seconds,
      cell: ({ row }) =>
        formatMemoryHours(row.original.weekly_usage.memory_mib_seconds),
    },
    {
      id: "gpu",
      header: "GPU 実行時間",
      accessorFn: (service) => service.weekly_usage.gpu_seconds,
      cell: ({ row }) => formatGpuHours(row.original.weekly_usage.gpu_seconds),
    },
  ];
}

export function CostManagementPage() {
  const queryClient = useQueryClient();
  const { activeOrganization } = useActiveOrganization();
  const organizationId = activeOrganization.organization_id;
  const usage = useQuery(flashCostManagementQueryOptions(organizationId));

  if (usage.isPending) {
    return <PageLoading label="コスト管理情報を読み込んでいます" />;
  }
  if (usage.isError) {
    return (
      <ErrorState
        description="Flashの使用量を取得できませんでした。"
        onRetry={() =>
          void queryClient.invalidateQueries({
            queryKey: ["organizations", organizationId, "flash", "usage"],
          })
        }
      />
    );
  }

  const data = usage.data;
  return (
    <SpaceBetween size="l">
      <PageHeader
        title="コスト管理"
        description={`今週の割当時間を確認します。${formatUnixTime(data.week_ends_at)} にリセットされます。`}
        actions={
          <Button
            iconName="refresh"
            onClick={() => void usage.refetch()}
            loading={usage.isFetching}
          >
            更新
          </Button>
        }
      />
      <RuntimeUsagePanel usage={data.usage} limits={data.limits} />
      <CurrentAllocationPanel current={data.current} />
      <DataTable
        columns={serviceColumns()}
        data={data.services}
        getRowId={(service) => service.service_instance_id}
        searchPlaceholder="サービス名またはIDで検索"
        emptyTitle="今週のFlash使用量はありません"
        emptyDescription="Flashサービスを起動するとここに割当時間が表示されます。"
        mobileVisibleColumns={["service", "cpu", "memory"]}
      />
      <Container header={<Header variant="h2">計測方法</Header>}>
        <SpaceBetween size="xs">
          <Box>
            CPUは割当vCPU × 時間、メモリは割当GiB × 時間、GPUは割当枚数 × 時間で計測します。
          </Box>
          <Box color="text-body-secondary">
            Readyレプリカだけを30秒間隔で加算します。scale-to-zero中は加算しません。削除したサービスの今週分もリセットまで保持します。
          </Box>
        </SpaceBetween>
      </Container>
    </SpaceBetween>
  );
}

function ownerColumns(): ColumnDef<OwnerFlashCostTenant, unknown>[] {
  return [
    {
      id: "organization",
      header: "クラウドアカウント",
      accessorFn: (tenant) =>
        `${tenant.organization.name} ${tenant.organization.slug} ${tenant.organization.id}`,
      cell: ({ row }) => (
        <SpaceBetween size="xxs">
          <Box fontWeight="bold">{row.original.organization.name}</Box>
          <Box color="text-body-secondary">{row.original.organization.slug}</Box>
        </SpaceBetween>
      ),
    },
    {
      id: "cpu",
      header: "CPU 実行時間",
      accessorFn: (tenant) => tenant.usage.cpu_millicore_seconds,
      cell: ({ row }) => (
        <SpaceBetween size="xxs">
          <Box>{formatCpuHours(row.original.usage.cpu_millicore_seconds)}</Box>
          <Box color="text-body-secondary">
            上限 {formatCpuHours(row.original.limits.max_weekly_cpu_millicore_seconds)}
          </Box>
        </SpaceBetween>
      ),
    },
    {
      id: "memory",
      header: "メモリ実行時間",
      accessorFn: (tenant) => tenant.usage.memory_mib_seconds,
      cell: ({ row }) => (
        <SpaceBetween size="xxs">
          <Box>{formatMemoryHours(row.original.usage.memory_mib_seconds)}</Box>
          <Box color="text-body-secondary">
            上限 {formatMemoryHours(row.original.limits.max_weekly_memory_mib_seconds)}
          </Box>
        </SpaceBetween>
      ),
    },
    {
      id: "gpu",
      header: "GPU 実行時間",
      accessorFn: (tenant) => tenant.usage.gpu_seconds,
      cell: ({ row }) => (
        <SpaceBetween size="xxs">
          <Box>{formatGpuHours(row.original.usage.gpu_seconds)}</Box>
          <Box color="text-body-secondary">
            上限 {formatGpuHours(row.original.limits.max_weekly_gpu_seconds)}
          </Box>
        </SpaceBetween>
      ),
    },
    {
      id: "current",
      header: "現在の割当",
      accessorFn: (tenant) => tenant.current.ready_replicas,
      cell: ({ row }) =>
        `${formatNumber(row.original.current.active_services)} services · ${formatNumber(row.original.current.ready_replicas)} replicas · ${formatDecimal(row.original.current.cpu_millis / 1_000)} vCPU · ${formatDecimal(row.original.current.memory_mib / 1_024)} GiB · ${formatNumber(row.original.current.gpus)} GPU`,
    },
  ];
}

export function OwnerCostManagementPage() {
  const usage = useQuery(ownerFlashCostManagementQueryOptions);
  const columns = useMemo(ownerColumns, []);

  if (usage.isPending) {
    return <PageLoading label="全アカウントの使用量を読み込んでいます" />;
  }
  if (usage.isError) {
    return (
      <ErrorState
        description="全アカウントのFlash使用量を取得できませんでした。"
        onRetry={() => void usage.refetch()}
      />
    );
  }

  const data = usage.data;
  return (
    <SpaceBetween size="l">
      <PageHeader
        title="コスト管理"
        description={`全クラウドアカウントの今週のFlash割当時間です。${formatUnixTime(data.week_ends_at)} にリセットされます。`}
        actions={
          <Button
            iconName="refresh"
            onClick={() => void usage.refetch()}
            loading={usage.isFetching}
          >
            更新
          </Button>
        }
      />
      <Container header={<Header variant="h2">全体使用量</Header>}>
        <ColumnLayout columns={3} variant="text-grid">
          <div>
            <Box variant="awsui-key-label">CPU 実行時間</Box>
            <Box fontSize="heading-l">{formatCpuHours(data.usage.cpu_millicore_seconds)}</Box>
          </div>
          <div>
            <Box variant="awsui-key-label">メモリ実行時間</Box>
            <Box fontSize="heading-l">{formatMemoryHours(data.usage.memory_mib_seconds)}</Box>
          </div>
          <div>
            <Box variant="awsui-key-label">GPU 実行時間</Box>
            <Box fontSize="heading-l">{formatGpuHours(data.usage.gpu_seconds)}</Box>
          </div>
        </ColumnLayout>
      </Container>
      <CurrentAllocationPanel current={data.current} />
      <DataTable
        columns={columns}
        data={data.tenants}
        getRowId={(tenant) => tenant.organization.id}
        searchPlaceholder="アカウント名、slug、IDで検索"
        emptyTitle="クラウドアカウントがありません"
        emptyDescription="登録済みのクラウドアカウントはありません。"
        mobileVisibleColumns={["organization", "cpu", "memory"]}
      />
    </SpaceBetween>
  );
}
