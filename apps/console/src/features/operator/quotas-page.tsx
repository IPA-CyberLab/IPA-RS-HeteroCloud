import Badge from "@cloudscape-design/components/badge";
import Box from "@cloudscape-design/components/box";
import Button from "@cloudscape-design/components/button";
import ColumnLayout from "@cloudscape-design/components/column-layout";
import Container from "@cloudscape-design/components/container";
import FormField from "@cloudscape-design/components/form-field";
import Header from "@cloudscape-design/components/header";
import Input from "@cloudscape-design/components/input";
import Modal from "@cloudscape-design/components/modal";
import SpaceBetween from "@cloudscape-design/components/space-between";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { ColumnDef } from "@tanstack/react-table";
import { useMemo, useState } from "react";
import { DataTable } from "@/components/shared/data-table";
import { ErrorState } from "@/components/shared/error-state";
import { FormError } from "@/components/shared/form-error";
import { PageHeader } from "@/components/shared/page-header";
import { PageLoading } from "@/components/shared/page-loading";
import { api, getApiErrorMessage } from "@/lib/api-client";
import type {
  FlashQuotaLimits,
  FlowQuotaLimits,
  ResourceQuotaLimits,
  ResourceQuotaTenant,
} from "@/lib/api-types";
import { formatNumber } from "@/lib/utils";

const quotaQueryKey = ["owner", "resource-quotas"] as const;

function positiveInteger(value: string, fallback: number) {
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) && parsed > 0 ? parsed : fallback;
}

function NumberField({
  label,
  value,
  onChange,
  description,
}: {
  label: string;
  value: number;
  onChange: (value: number) => void;
  description?: string;
}) {
  return (
    <FormField label={label} description={description}>
      <Input
        type="number"
        inputMode="numeric"
        step={1}
        nativeInputAttributes={{ min: 1 }}
        value={String(value)}
        onChange={({ detail }) => onChange(positiveInteger(detail.value, value))}
      />
    </FormField>
  );
}

function QuotaEditor({
  value,
  onChange,
}: {
  value: ResourceQuotaLimits;
  onChange: (value: ResourceQuotaLimits) => void;
}) {
  const flow = <Key extends keyof FlowQuotaLimits>(key: Key, next: number) =>
    onChange({ ...value, flow: { ...value.flow, [key]: next } });
  const flash = <Key extends keyof FlashQuotaLimits>(key: Key, next: number) =>
    onChange({ ...value, flash: { ...value.flash, [key]: next } });

  return (
    <SpaceBetween size="l">
      <Container header={<Header variant="h3">Flow</Header>}>
        <ColumnLayout columns={3} variant="text-grid">
          <NumberField label="サービス数" value={value.flow.max_services} onChange={(next) => flow("max_services", next)} />
          <NumberField label="1サービスのルーム数" value={value.flow.max_rooms_per_service} onChange={(next) => flow("max_rooms_per_service", next)} />
          <NumberField label="合計ルーム数" value={value.flow.max_total_rooms} onChange={(next) => flow("max_total_rooms", next)} />
          <NumberField label="1サービスの同時参加者" value={value.flow.max_participants_per_service} onChange={(next) => flow("max_participants_per_service", next)} />
          <NumberField label="IPあたりRPS" value={value.flow.max_rate_limit_requests_per_second} onChange={(next) => flow("max_rate_limit_requests_per_second", next)} />
          <NumberField label="IPあたりバースト" value={value.flow.max_rate_limit_burst} onChange={(next) => flow("max_rate_limit_burst", next)} />
          <NumberField label="開発者認証情報 / サービス" value={value.flow.max_developer_credentials_per_service} onChange={(next) => flow("max_developer_credentials_per_service", next)} />
        </ColumnLayout>
      </Container>
      <Container header={<Header variant="h3">Flash</Header>}>
        <ColumnLayout columns={3} variant="text-grid">
          <NumberField label="サービス数" value={value.flash.max_services} onChange={(next) => flash("max_services", next)} />
          <NumberField label="レプリカ / サービス" value={value.flash.max_replicas_per_service} onChange={(next) => flash("max_replicas_per_service", next)} />
          <NumberField label="CPU / VM (millicores)" value={value.flash.max_cpu_millis_per_vm} onChange={(next) => flash("max_cpu_millis_per_vm", next)} />
          <NumberField label="メモリ / VM (MiB)" value={value.flash.max_memory_mib_per_vm} onChange={(next) => flash("max_memory_mib_per_vm", next)} />
          <NumberField label="ディスク / VM (GiB)" value={value.flash.max_disk_gib_per_vm} onChange={(next) => flash("max_disk_gib_per_vm", next)} />
          <NumberField label="合計レプリカ" value={value.flash.max_total_replicas} onChange={(next) => flash("max_total_replicas", next)} />
          <NumberField label="合計CPU (millicores)" value={value.flash.max_total_cpu_millis} onChange={(next) => flash("max_total_cpu_millis", next)} />
          <NumberField label="合計メモリ (MiB)" value={value.flash.max_total_memory_mib} onChange={(next) => flash("max_total_memory_mib", next)} />
          <NumberField label="合計ディスク (GiB)" value={value.flash.max_total_disk_gib} onChange={(next) => flash("max_total_disk_gib", next)} />
        </ColumnLayout>
      </Container>
      <Container header={<Header variant="h3">コンテナレジストリ</Header>}>
        <ColumnLayout columns={3} variant="text-grid">
          <NumberField
            label="保存容量 / テナント (GiB)"
            value={value.registry.storage_gib}
            onChange={(next) =>
              onChange({ ...value, registry: { ...value.registry, storage_gib: next } })
            }
          />
          <NumberField
            label="認証情報数 / テナント"
            value={value.registry.max_credentials}
            onChange={(next) =>
              onChange({ ...value, registry: { ...value.registry, max_credentials: next } })
            }
          />
        </ColumnLayout>
      </Container>
    </SpaceBetween>
  );
}

export function OwnerQuotasPage() {
  const queryClient = useQueryClient();
  const quotas = useQuery({
    queryKey: quotaQueryKey,
    queryFn: ({ signal }) => api.owner.quotas.overview(signal),
  });
  const [defaults, setDefaults] = useState<ResourceQuotaLimits | null>(null);
  const [editing, setEditing] = useState<ResourceQuotaTenant | null>(null);
  const [tenantLimits, setTenantLimits] = useState<ResourceQuotaLimits | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  const refresh = async () => {
    await queryClient.invalidateQueries({ queryKey: quotaQueryKey });
  };
  const saveDefaults = useMutation({
    mutationFn: (limits: ResourceQuotaLimits) => api.owner.quotas.updateDefaults(limits),
    onSuccess: async (limits) => {
      setDefaults(limits);
      setMessage("既定のハードリミットを更新しました。");
      await refresh();
    },
  });
  const saveTenant = useMutation({
    mutationFn: ({ id, limits }: { id: string; limits: ResourceQuotaLimits }) =>
      api.owner.quotas.updateOrganization(id, limits),
    onSuccess: async () => {
      setEditing(null);
      setTenantLimits(null);
      await refresh();
    },
  });
  const clearTenant = useMutation({
    mutationFn: api.owner.quotas.clearOrganization,
    onSuccess: refresh,
  });

  const columns = useMemo<ColumnDef<ResourceQuotaTenant, unknown>[]>(
    () => [
      {
        id: "organization",
        header: "テナント",
        accessorFn: (tenant) => `${tenant.organization.name} ${tenant.organization.slug}`,
        cell: ({ row }) => (
          <SpaceBetween size="xxs">
            <Box fontWeight="bold">{row.original.organization.name}</Box>
            <Box color="text-body-secondary">{row.original.organization.slug}</Box>
          </SpaceBetween>
        ),
      },
      {
        id: "source",
        header: "設定",
        accessorFn: (tenant) => (tenant.override_limits ? "個別" : "既定"),
        cell: ({ row }) => (
          <Badge color={row.original.override_limits ? "blue" : "grey"}>
            {row.original.override_limits ? "個別" : "既定"}
          </Badge>
        ),
      },
      {
        id: "flow",
        header: "Flow",
        accessorFn: (tenant) => tenant.usage.flow_services,
        cell: ({ row }) => `${formatNumber(row.original.usage.flow_services)} サービス / ${formatNumber(row.original.usage.flow_configured_rooms)} ルーム`,
      },
      {
        id: "flash",
        header: "Flash",
        accessorFn: (tenant) => tenant.usage.flash_services,
        cell: ({ row }) => `${formatNumber(row.original.usage.flash_services)} サービス / ${formatNumber(row.original.usage.flash_replicas)} VM / ${formatNumber(row.original.usage.flash_disk_gib)} GiB`,
      },
      {
        id: "registry",
        header: "レジストリ上限",
        accessorFn: (tenant) => tenant.effective_limits.registry.storage_gib,
        cell: ({ row }) => `${formatNumber(row.original.effective_limits.registry.storage_gib)} GiB`,
      },
      {
        id: "actions",
        header: "操作",
        enableSorting: false,
        cell: ({ row }) => (
          <SpaceBetween direction="horizontal" size="xs">
            <Button
              iconName="edit"
              variant="icon"
              ariaLabel={`${row.original.organization.name}の上限を編集`}
              onClick={() => {
                setEditing(row.original);
                setTenantLimits(structuredClone(row.original.effective_limits));
              }}
            />
            <Button
              iconName="refresh"
              variant="icon"
              disabled={!row.original.override_limits || clearTenant.isPending}
              ariaLabel={`${row.original.organization.name}を既定値へ戻す`}
              onClick={() => clearTenant.mutate(row.original.organization.id)}
            />
          </SpaceBetween>
        ),
      },
    ],
    [clearTenant],
  );

  if (quotas.isPending) return <PageLoading label="所有者設定を読み込んでいます" />;
  if (quotas.isError) {
    return <ErrorState description="所有者設定を取得できませんでした。" onRetry={() => void quotas.refetch()} />;
  }
  const currentDefaults = defaults ?? quotas.data.defaults;

  return (
    <SpaceBetween size="l">
      <PageHeader
        title="所有者設定"
        description="あなた専用の内部画面でFlow、Flash、コンテナレジストリの既定値と組織別ハードリミットを管理します。"
        actions={
          <Button
            variant="primary"
            loading={saveDefaults.isPending}
            onClick={() => saveDefaults.mutate(currentDefaults)}
          >
            既定値を保存
          </Button>
        }
      />
      {message ? <Box color="text-status-success">{message}</Box> : null}
      {saveDefaults.isError ? <FormError message={getApiErrorMessage(saveDefaults.error)} /> : null}
      <QuotaEditor value={currentDefaults} onChange={setDefaults} />
      <Container
        header={
          <Header variant="h2" description="個別設定を削除すると直ちに既定値へ戻ります。">
            テナント別上限
          </Header>
        }
      >
        <DataTable
          columns={columns}
          data={quotas.data.tenants}
          getRowId={(tenant) => tenant.organization.id}
          searchPlaceholder="テナント名またはslugで検索"
          emptyTitle="テナントがありません"
          emptyDescription="登録済みテナントはありません。"
        />
      </Container>
      <Modal
        visible={editing !== null && tenantLimits !== null}
        size="max"
        header={editing ? `${editing.organization.name} のハードリミット` : "テナント上限"}
        onDismiss={() => {
          setEditing(null);
          setTenantLimits(null);
        }}
        footer={
          <Box float="right">
            <SpaceBetween direction="horizontal" size="xs">
              <Button onClick={() => setEditing(null)}>キャンセル</Button>
              <Button
                variant="primary"
                loading={saveTenant.isPending}
                disabled={!editing || !tenantLimits}
                onClick={() => {
                  if (editing && tenantLimits) {
                    saveTenant.mutate({ id: editing.organization.id, limits: tenantLimits });
                  }
                }}
              >
                個別上限を保存
              </Button>
            </SpaceBetween>
          </Box>
        }
      >
        <SpaceBetween size="l">
          {saveTenant.isError ? <FormError message={getApiErrorMessage(saveTenant.error)} /> : null}
          {tenantLimits ? <QuotaEditor value={tenantLimits} onChange={setTenantLimits} /> : null}
        </SpaceBetween>
      </Modal>
    </SpaceBetween>
  );
}
