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
  OwnerAccount,
  ResourceQuotaLimits,
  ResourceQuotaTenant,
  UserLoginEvent,
} from "@/lib/api-types";
import { formatDateTime, formatNumber } from "@/lib/utils";

const quotaQueryKey = ["owner", "resource-quotas"] as const;
const accountQueryKey = ["owner", "accounts"] as const;
const QUOTA_BOUNDS = {
  flow: {
    maxServices: 10_000,
    maxRoomsPerService: 1_000_000,
    maxTotalRooms: 100_000_000,
    maxParticipantsPerService: 100_000,
    maxRequestsPerSecond: 1_000,
    maxBurst: 5_000,
    maxDeveloperCredentialsPerService: 10_000,
  },
  flash: {
    minReplicasPerService: 1,
    maxReplicasPerService: 100,
    minCpuMillisPerVm: 10,
    maxCpuMillisPerVm: 4_000,
    minMemoryMibPerVm: 16,
    maxMemoryMibPerVm: 8_128,
    minDiskGibPerVm: 1,
    maxDiskGibPerVm: 10,
    maxServices: 10_000,
    maxTotalReplicas: 100_000,
    maxTotalCpuMillis: 100_000_000,
    maxTotalMemoryMib: 1_048_576,
    maxTotalDiskGib: 1_000_000,
  },
  registry: {
    maxStorageGib: 10_240,
    maxCredentials: 1_000,
  },
} as const;

function authenticationMethodLabel(method: UserLoginEvent["authentication_method"]) {
  return method === "oidc" ? "Keycloak (OIDC)" : "ローカル";
}

function accountAuthenticationLabel(account: OwnerAccount) {
  const methods = [];
  if (account.external_identities.length > 0) methods.push("Keycloak (OIDC)");
  if (account.has_local_password) methods.push("ローカル");
  return methods.join(" / ") || "未設定";
}

function clampInteger(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}

function boundedInteger(value: string, fallback: number, min: number, max: number) {
  const parsed = Number(value);
  return value.trim() !== "" && Number.isSafeInteger(parsed)
    ? clampInteger(parsed, min, max)
    : fallback;
}

export function normalizeResourceQuotaLimits(
  value: ResourceQuotaLimits,
): ResourceQuotaLimits {
  const flowRoomsPerService = clampInteger(
    value.flow.max_rooms_per_service,
    1,
    QUOTA_BOUNDS.flow.maxRoomsPerService,
  );
  const flowRequestsPerSecond = clampInteger(
    value.flow.max_rate_limit_requests_per_second,
    1,
    QUOTA_BOUNDS.flow.maxRequestsPerSecond,
  );
  const flashReplicasPerService = clampInteger(
    value.flash.max_replicas_per_service,
    QUOTA_BOUNDS.flash.minReplicasPerService,
    QUOTA_BOUNDS.flash.maxReplicasPerService,
  );
  const flashCpuMillisPerVm = clampInteger(
    value.flash.max_cpu_millis_per_vm,
    QUOTA_BOUNDS.flash.minCpuMillisPerVm,
    QUOTA_BOUNDS.flash.maxCpuMillisPerVm,
  );
  const flashMemoryMibPerVm = clampInteger(
    value.flash.max_memory_mib_per_vm,
    QUOTA_BOUNDS.flash.minMemoryMibPerVm,
    QUOTA_BOUNDS.flash.maxMemoryMibPerVm,
  );
  const flashDiskGibPerVm = clampInteger(
    value.flash.max_disk_gib_per_vm,
    QUOTA_BOUNDS.flash.minDiskGibPerVm,
    QUOTA_BOUNDS.flash.maxDiskGibPerVm,
  );

  return {
    flow: {
      max_services: clampInteger(value.flow.max_services, 1, QUOTA_BOUNDS.flow.maxServices),
      max_rooms_per_service: flowRoomsPerService,
      max_total_rooms: clampInteger(
        value.flow.max_total_rooms,
        flowRoomsPerService,
        QUOTA_BOUNDS.flow.maxTotalRooms,
      ),
      max_participants_per_service: clampInteger(
        value.flow.max_participants_per_service,
        1,
        QUOTA_BOUNDS.flow.maxParticipantsPerService,
      ),
      max_rate_limit_requests_per_second: flowRequestsPerSecond,
      max_rate_limit_burst: clampInteger(
        value.flow.max_rate_limit_burst,
        flowRequestsPerSecond,
        QUOTA_BOUNDS.flow.maxBurst,
      ),
      max_developer_credentials_per_service: clampInteger(
        value.flow.max_developer_credentials_per_service,
        1,
        QUOTA_BOUNDS.flow.maxDeveloperCredentialsPerService,
      ),
    },
    flash: {
      max_services: clampInteger(value.flash.max_services, 1, QUOTA_BOUNDS.flash.maxServices),
      max_replicas_per_service: flashReplicasPerService,
      max_cpu_millis_per_vm: flashCpuMillisPerVm,
      max_memory_mib_per_vm: flashMemoryMibPerVm,
      max_disk_gib_per_vm: flashDiskGibPerVm,
      max_total_replicas: clampInteger(
        value.flash.max_total_replicas,
        flashReplicasPerService,
        QUOTA_BOUNDS.flash.maxTotalReplicas,
      ),
      max_total_cpu_millis: clampInteger(
        value.flash.max_total_cpu_millis,
        flashCpuMillisPerVm,
        QUOTA_BOUNDS.flash.maxTotalCpuMillis,
      ),
      max_total_memory_mib: clampInteger(
        value.flash.max_total_memory_mib,
        flashMemoryMibPerVm,
        QUOTA_BOUNDS.flash.maxTotalMemoryMib,
      ),
      max_total_disk_gib: clampInteger(
        value.flash.max_total_disk_gib,
        flashDiskGibPerVm,
        QUOTA_BOUNDS.flash.maxTotalDiskGib,
      ),
    },
    registry: {
      storage_gib: clampInteger(
        value.registry.storage_gib,
        1,
        QUOTA_BOUNDS.registry.maxStorageGib,
      ),
      max_credentials: clampInteger(
        value.registry.max_credentials,
        1,
        QUOTA_BOUNDS.registry.maxCredentials,
      ),
    },
  };
}

function NumberField({
  label,
  value,
  onChange,
  description,
  min = 1,
  max = Number.MAX_SAFE_INTEGER,
}: {
  label: string;
  value: number;
  onChange: (value: number) => void;
  description?: string;
  min?: number;
  max?: number;
}) {
  return (
    <FormField label={label} description={description}>
      <Input
        type="number"
        inputMode="numeric"
        step={1}
        nativeInputAttributes={{ min, max }}
        value={String(value)}
        onChange={({ detail }) =>
          onChange(boundedInteger(detail.value, value, min, max))
        }
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
  const update = (next: ResourceQuotaLimits) =>
    onChange(normalizeResourceQuotaLimits(next));
  const flow = <Key extends keyof FlowQuotaLimits>(key: Key, next: number) =>
    update({ ...value, flow: { ...value.flow, [key]: next } });
  const flash = <Key extends keyof FlashQuotaLimits>(key: Key, next: number) =>
    update({ ...value, flash: { ...value.flash, [key]: next } });

  return (
    <SpaceBetween size="l">
      <Container header={<Header variant="h3">Flow</Header>}>
        <ColumnLayout columns={3} variant="text-grid">
          <NumberField label="サービス数" value={value.flow.max_services} max={QUOTA_BOUNDS.flow.maxServices} onChange={(next) => flow("max_services", next)} />
          <NumberField label="1サービスのルーム数" value={value.flow.max_rooms_per_service} max={QUOTA_BOUNDS.flow.maxRoomsPerService} onChange={(next) => flow("max_rooms_per_service", next)} />
          <NumberField label="合計ルーム数" value={value.flow.max_total_rooms} min={value.flow.max_rooms_per_service} max={QUOTA_BOUNDS.flow.maxTotalRooms} onChange={(next) => flow("max_total_rooms", next)} />
          <NumberField label="1サービスの同時参加者" value={value.flow.max_participants_per_service} max={QUOTA_BOUNDS.flow.maxParticipantsPerService} onChange={(next) => flow("max_participants_per_service", next)} />
          <NumberField label="IPあたりRPS" value={value.flow.max_rate_limit_requests_per_second} max={QUOTA_BOUNDS.flow.maxRequestsPerSecond} onChange={(next) => flow("max_rate_limit_requests_per_second", next)} />
          <NumberField label="IPあたりバースト" value={value.flow.max_rate_limit_burst} min={value.flow.max_rate_limit_requests_per_second} max={QUOTA_BOUNDS.flow.maxBurst} onChange={(next) => flow("max_rate_limit_burst", next)} />
          <NumberField label="開発者認証情報 / サービス" value={value.flow.max_developer_credentials_per_service} max={QUOTA_BOUNDS.flow.maxDeveloperCredentialsPerService} onChange={(next) => flow("max_developer_credentials_per_service", next)} />
        </ColumnLayout>
      </Container>
      <Container header={<Header variant="h3">Flash</Header>}>
        <ColumnLayout columns={3} variant="text-grid">
          <NumberField label="サービス数" value={value.flash.max_services} max={QUOTA_BOUNDS.flash.maxServices} onChange={(next) => flash("max_services", next)} />
          <NumberField label="レプリカ / サービス" value={value.flash.max_replicas_per_service} min={QUOTA_BOUNDS.flash.minReplicasPerService} max={QUOTA_BOUNDS.flash.maxReplicasPerService} onChange={(next) => flash("max_replicas_per_service", next)} />
          <NumberField label="CPU / VM (millicores)" value={value.flash.max_cpu_millis_per_vm} min={QUOTA_BOUNDS.flash.minCpuMillisPerVm} max={QUOTA_BOUNDS.flash.maxCpuMillisPerVm} onChange={(next) => flash("max_cpu_millis_per_vm", next)} />
          <NumberField label="メモリ / VM (MiB)" value={value.flash.max_memory_mib_per_vm} min={QUOTA_BOUNDS.flash.minMemoryMibPerVm} max={QUOTA_BOUNDS.flash.maxMemoryMibPerVm} onChange={(next) => flash("max_memory_mib_per_vm", next)} />
          <NumberField label="ディスク / VM (GiB)" value={value.flash.max_disk_gib_per_vm} min={QUOTA_BOUNDS.flash.minDiskGibPerVm} max={QUOTA_BOUNDS.flash.maxDiskGibPerVm} onChange={(next) => flash("max_disk_gib_per_vm", next)} />
          <NumberField label="合計レプリカ" value={value.flash.max_total_replicas} min={value.flash.max_replicas_per_service} max={QUOTA_BOUNDS.flash.maxTotalReplicas} onChange={(next) => flash("max_total_replicas", next)} />
          <NumberField label="合計CPU (millicores)" value={value.flash.max_total_cpu_millis} min={value.flash.max_cpu_millis_per_vm} max={QUOTA_BOUNDS.flash.maxTotalCpuMillis} onChange={(next) => flash("max_total_cpu_millis", next)} />
          <NumberField label="合計メモリ (MiB)" value={value.flash.max_total_memory_mib} min={value.flash.max_memory_mib_per_vm} max={QUOTA_BOUNDS.flash.maxTotalMemoryMib} onChange={(next) => flash("max_total_memory_mib", next)} />
          <NumberField label="合計ディスク (GiB)" value={value.flash.max_total_disk_gib} min={value.flash.max_disk_gib_per_vm} max={QUOTA_BOUNDS.flash.maxTotalDiskGib} onChange={(next) => flash("max_total_disk_gib", next)} />
        </ColumnLayout>
      </Container>
      <Container header={<Header variant="h3">Flash Registry</Header>}>
        <ColumnLayout columns={3} variant="text-grid">
          <NumberField
            label="保存容量 / テナント (GiB)"
            value={value.registry.storage_gib}
            max={QUOTA_BOUNDS.registry.maxStorageGib}
            onChange={(next) =>
              update({ ...value, registry: { ...value.registry, storage_gib: next } })
            }
          />
          <NumberField
            label="認証情報数 / テナント"
            value={value.registry.max_credentials}
            max={QUOTA_BOUNDS.registry.maxCredentials}
            onChange={(next) =>
              update({ ...value, registry: { ...value.registry, max_credentials: next } })
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
  const accounts = useQuery({
    queryKey: accountQueryKey,
    queryFn: ({ signal }) => api.owner.accounts.list(signal),
  });
  const [defaults, setDefaults] = useState<ResourceQuotaLimits | null>(null);
  const [editing, setEditing] = useState<ResourceQuotaTenant | null>(null);
  const [tenantLimits, setTenantLimits] = useState<ResourceQuotaLimits | null>(null);
  const [selectedAccount, setSelectedAccount] = useState<OwnerAccount | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const accountLogins = useQuery({
    queryKey: [...accountQueryKey, selectedAccount?.user.id, "logins"],
    queryFn: ({ signal }) =>
      api.owner.accounts.logins(selectedAccount?.user.id ?? "", 100, signal),
    enabled: selectedAccount !== null,
  });

  const refresh = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: quotaQueryKey }),
      queryClient.invalidateQueries({ queryKey: accountQueryKey }),
    ]);
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

  const accountColumns = useMemo<ColumnDef<OwnerAccount, unknown>[]>(
    () => [
      {
        id: "user",
        header: "登録ユーザー",
        accessorFn: (account) =>
          `${account.user.display_name} ${account.user.email} ${account.user.id}`,
        cell: ({ row }) => (
          <SpaceBetween size="xxs">
            <Box fontWeight="bold">{row.original.user.display_name}</Box>
            <Box color="text-body-secondary">{row.original.user.id}</Box>
          </SpaceBetween>
        ),
      },
      {
        id: "email",
        header: "メールアドレス",
        accessorFn: (account) => account.user.email,
      },
      {
        id: "status",
        header: "状態",
        accessorFn: (account) => account.user.status,
        cell: ({ row }) => (
          <Badge color={row.original.user.status === "active" ? "green" : "red"}>
            {row.original.user.status === "active" ? "有効" : "停止中"}
          </Badge>
        ),
      },
      {
        id: "authentication",
        header: "認証",
        accessorFn: accountAuthenticationLabel,
      },
      {
        id: "organizations",
        header: "クラウドアカウント",
        accessorFn: (account) =>
          account.memberships
            .map(
              (membership) =>
                `${membership.organization_name} ${membership.organization_slug}`,
            )
            .join(" "),
        cell: ({ row }) =>
          row.original.memberships
            .map((membership) => membership.organization_name)
            .join(", ") || "—",
      },
      {
        id: "last_login_ip",
        header: "最終ログインIP",
        accessorFn: (account) => account.last_login?.source_ip ?? "",
        cell: ({ row }) => row.original.last_login?.source_ip ?? "—",
      },
      {
        id: "last_login_at",
        header: "最終ログイン",
        accessorFn: (account) => account.last_login?.occurred_at ?? "",
        cell: ({ row }) => formatDateTime(row.original.last_login?.occurred_at),
      },
    ],
    [],
  );

  const loginColumns = useMemo<ColumnDef<UserLoginEvent, unknown>[]>(
    () => [
      {
        id: "occurred_at",
        header: "ログイン日時",
        accessorFn: (event) => event.occurred_at,
        cell: ({ row }) => formatDateTime(row.original.occurred_at),
      },
      {
        id: "source_ip",
        header: "IPアドレス",
        accessorFn: (event) => event.source_ip ?? "",
        cell: ({ row }) => row.original.source_ip ?? "—",
      },
      {
        id: "authentication_method",
        header: "認証方式",
        accessorFn: (event) => authenticationMethodLabel(event.authentication_method),
      },
    ],
    [],
  );

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
        header: "Flash Registry上限",
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
                setTenantLimits(
                  normalizeResourceQuotaLimits(row.original.effective_limits),
                );
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

  if (quotas.isPending || accounts.isPending) {
    return <PageLoading label="全アカウント情報を読み込んでいます" />;
  }
  if (quotas.isError || accounts.isError) {
    return (
      <ErrorState
        description="全アカウント情報を取得できませんでした。"
        onRetry={() => void refresh()}
      />
    );
  }
  const currentDefaults = normalizeResourceQuotaLimits(
    defaults ?? quotas.data.defaults,
  );
  const tenants = quotas.data.tenants;
  const registeredAccounts = accounts.data.items;
  const aggregate = tenants.reduce(
    (totals, tenant) => ({
      flowServices: totals.flowServices + tenant.usage.flow_services,
      configuredRooms: totals.configuredRooms + tenant.usage.flow_configured_rooms,
      flashServices: totals.flashServices + tenant.usage.flash_services,
      flashReplicas: totals.flashReplicas + tenant.usage.flash_replicas,
    }),
    { flowServices: 0, configuredRooms: 0, flashServices: 0, flashReplicas: 0 },
  );
  const customLimitCount = tenants.filter((tenant) => tenant.override_limits).length;

  return (
    <SpaceBetween size="l">
      <PageHeader
        title="全アカウント管理"
        description="HeteroCloudサービス全体の利用状況と、全クラウドアカウントに適用するハードリミットを管理します。"
        actions={
          <Button
            iconName="refresh"
            onClick={() => void refresh()}
          >
            更新
          </Button>
        }
      />
      <Container header={<Header variant="h2">サービス全体</Header>}>
        <ColumnLayout columns={6} variant="text-grid">
          {([
            ["登録ユーザー", registeredAccounts.length],
            ["クラウドアカウント", tenants.length],
            ["Flowサービス", aggregate.flowServices],
            ["設定済みルーム上限", aggregate.configuredRooms],
            ["Flashサービス", aggregate.flashServices],
            ["Flash VM", aggregate.flashReplicas],
          ] as const).map(([label, value]) => (
            <div key={label}>
              <Box variant="awsui-key-label">{label}</Box>
              <Box variant="awsui-value-large">{formatNumber(value)}</Box>
            </div>
          ))}
        </ColumnLayout>
      </Container>
      <Header
        variant="h2"
        counter={`(${formatNumber(registeredAccounts.length)})`}
        description="登録情報、認証方式、所属クラウドアカウント、最終ログインIPを確認できます。"
      >
        登録ユーザー
      </Header>
      <DataTable
        columns={accountColumns}
        data={registeredAccounts}
        getRowId={(account) => account.user.id}
        onRowClick={setSelectedAccount}
        getRowAriaLabel={(account) => `${account.user.display_name}の登録情報を表示`}
        searchPlaceholder="氏名、メール、ユーザーID、IPで検索"
        emptyTitle="登録ユーザーがいません"
        emptyDescription="HeteroCloudに登録されたユーザーはまだいません。"
        mobileVisibleColumns={["user", "status", "last_login_ip"]}
      />
      <Modal
        visible={selectedAccount !== null}
        size="max"
        header={
          selectedAccount
            ? `${selectedAccount.user.display_name} のアカウント情報`
            : "アカウント情報"
        }
        onDismiss={() => setSelectedAccount(null)}
        footer={
          <Box float="right">
            <Button onClick={() => setSelectedAccount(null)}>閉じる</Button>
          </Box>
        }
      >
        {selectedAccount ? (
          <SpaceBetween size="l">
            <Header variant="h3">登録情報</Header>
            <ColumnLayout columns={3} variant="text-grid">
              <div>
                <Box variant="awsui-key-label">ユーザーID</Box>
                <Box>{selectedAccount.user.id}</Box>
              </div>
              <div>
                <Box variant="awsui-key-label">氏名</Box>
                <Box>{selectedAccount.user.display_name}</Box>
              </div>
              <div>
                <Box variant="awsui-key-label">メールアドレス</Box>
                <Box>{selectedAccount.user.email}</Box>
              </div>
              <div>
                <Box variant="awsui-key-label">状態</Box>
                <Box>
                  {selectedAccount.user.status === "active" ? "有効" : "停止中"}
                </Box>
              </div>
              <div>
                <Box variant="awsui-key-label">登録日時</Box>
                <Box>{formatDateTime(selectedAccount.user.created_at)}</Box>
              </div>
              <div>
                <Box variant="awsui-key-label">記録済みログイン</Box>
                <Box>{formatNumber(selectedAccount.login_count)} 件</Box>
              </div>
            </ColumnLayout>

            <Header variant="h3">認証情報</Header>
            <ColumnLayout columns={2} variant="text-grid">
              <div>
                <Box variant="awsui-key-label">ローカル資格情報</Box>
                <Box>
                  {selectedAccount.has_local_password ? "登録あり" : "登録なし"}
                </Box>
              </div>
              {selectedAccount.external_identities.map((identity) => (
                <div key={`${identity.issuer}:${identity.subject}`}>
                  <SpaceBetween size="xxs">
                    <Box variant="awsui-key-label">Keycloak / OIDC</Box>
                    <Box>{identity.issuer}</Box>
                    <Box color="text-body-secondary">Subject: {identity.subject}</Box>
                    <Box color="text-body-secondary">
                      連携日時: {formatDateTime(identity.created_at)}
                    </Box>
                  </SpaceBetween>
                </div>
              ))}
            </ColumnLayout>

            <Header
              variant="h3"
              counter={`(${formatNumber(selectedAccount.memberships.length)})`}
            >
              所属クラウドアカウント
            </Header>
            {selectedAccount.memberships.length > 0 ? (
              <ColumnLayout columns={2} variant="text-grid">
                {selectedAccount.memberships.map((membership) => (
                  <div key={membership.organization_id}>
                    <Box variant="awsui-key-label">
                      {membership.organization_name}
                    </Box>
                    <Box>{membership.organization_slug}</Box>
                    <Box color="text-body-secondary">
                      {membership.role === "owner" ? "所有者" : "メンバー"} · {membership.organization_id}
                    </Box>
                  </div>
                ))}
              </ColumnLayout>
            ) : (
              <Box color="text-body-secondary">所属はありません。</Box>
            )}

            <Header
              variant="h3"
              description="成功したログインを新しい順に最大100件保存します。"
            >
              ログイン履歴
            </Header>
            {accountLogins.isPending ? (
              <PageLoading label="ログイン履歴を読み込んでいます" />
            ) : accountLogins.isError ? (
              <ErrorState
                description="ログイン履歴を取得できませんでした。"
                onRetry={() => void accountLogins.refetch()}
              />
            ) : (
              <DataTable
                columns={loginColumns}
                data={accountLogins.data.items}
                getRowId={(event) => String(event.id)}
                searchPlaceholder="IPアドレスまたは認証方式で検索"
                emptyTitle="ログイン履歴がありません"
                emptyDescription="次回のログインからIPアドレスが記録されます。"
                initialPageSize={10}
              />
            )}
          </SpaceBetween>
        ) : null}
      </Modal>
      {message ? <Box color="text-status-success">{message}</Box> : null}
      {saveDefaults.isError ? <FormError message={getApiErrorMessage(saveDefaults.error)} /> : null}
      <Header
        variant="h2"
        description="個別上書きのない全クラウドアカウントへ適用されます。"
        actions={
          <Button
            variant="primary"
            loading={saveDefaults.isPending}
            onClick={() => saveDefaults.mutate(currentDefaults)}
          >
            既定値を保存
          </Button>
        }
      >
        全アカウントの既定ハードリミット
      </Header>
      <QuotaEditor value={currentDefaults} onChange={setDefaults} />
      {clearTenant.isError ? <FormError message={getApiErrorMessage(clearTenant.error)} /> : null}
      <Header
        variant="h2"
        counter={`(${formatNumber(tenants.length)})`}
        description={`${formatNumber(customLimitCount)}件に個別上限を設定中です。個別設定を解除すると既定値へ戻ります。`}
      >
        クラウドアカウント別ハードリミット
      </Header>
      <DataTable
        columns={columns}
        data={tenants}
        getRowId={(tenant) => tenant.organization.id}
        onRowClick={(tenant) => {
          setEditing(tenant);
          setTenantLimits(normalizeResourceQuotaLimits(tenant.effective_limits));
        }}
        getRowAriaLabel={(tenant) => `${tenant.organization.name}の上限を編集`}
        searchPlaceholder="アカウント名またはIDで検索"
        emptyTitle="クラウドアカウントがありません"
        emptyDescription="登録済みクラウドアカウントはありません。"
      />
      <Modal
        visible={editing !== null && tenantLimits !== null}
        size="max"
        header={editing ? `${editing.organization.name} のハードリミット` : "アカウント上限"}
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
