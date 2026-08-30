import Alert from "@cloudscape-design/components/alert";
import Box from "@cloudscape-design/components/box";
import Button from "@cloudscape-design/components/button";
import ColumnLayout from "@cloudscape-design/components/column-layout";
import Container from "@cloudscape-design/components/container";
import Header from "@cloudscape-design/components/header";
import KeyValuePairs from "@cloudscape-design/components/key-value-pairs";
import Modal from "@cloudscape-design/components/modal";
import Select from "@cloudscape-design/components/select";
import SpaceBetween from "@cloudscape-design/components/space-between";
import StatusIndicator, {
  type StatusIndicatorProps,
} from "@cloudscape-design/components/status-indicator";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { ColumnDef } from "@tanstack/react-table";
import { type FormEvent, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { DataTable } from "@/components/shared/data-table";
import { ErrorState } from "@/components/shared/error-state";
import { FormError } from "@/components/shared/form-error";
import { PageHeader } from "@/components/shared/page-header";
import { PageLoading } from "@/components/shared/page-loading";
import { RouterLink } from "@/components/shared/router-link";
import { StatusBadge } from "@/components/shared/status-badge";
import { useActiveOrganization } from "@/features/organizations/organization-context";
import { api, getApiErrorMessage } from "@/lib/api-client";
import type { FlashPort } from "@/lib/api-types";
import {
  flashQuotaQueryOptions,
  flashServiceQueryOptions,
  projectsQueryOptions,
  registryImagesQueryOptions,
} from "@/lib/queries";
import { formatDateTime, formatNumber } from "@/lib/utils";
import { FlashEndpoints } from "./flash-endpoints";
import {
  FlashServiceForm,
  flashFormFromService,
  flashFormValidationError,
  flashSpecFromForm,
  type FlashServiceFormValue,
} from "./flash-service-form";
import {
  FlashWebShell,
  type FlashShellConnectionState,
} from "./flash-web-shell";
import {
  flashExposureLabel,
  flashProviderStatus,
  flashProtocolLabel,
  flashServiceEndpoints,
  readyReplicas,
} from "./flash-service-utils";

const portColumns: ColumnDef<FlashPort, unknown>[] = [
  { accessorKey: "name", header: "名前" },
  {
    accessorKey: "protocol",
    header: "プロトコル",
    cell: ({ getValue }) => flashProtocolLabel(getValue<FlashPort["protocol"]>()),
  },
  { accessorKey: "container_port", header: "コンテナポート" },
  { accessorKey: "service_port", header: "サービスポート" },
];

const shellStatuses: Record<
  FlashShellConnectionState,
  { type: StatusIndicatorProps.Type; label: string }
> = {
  connecting: { type: "loading", label: "接続中" },
  connected: { type: "success", label: "接続済み" },
  closed: { type: "stopped", label: "切断済み" },
  error: { type: "error", label: "接続エラー" },
};

export function FlashServiceDetailPage() {
  const { serviceId = "" } = useParams<{ serviceId: string }>();
  const { activeOrganization } = useActiveOrganization();
  const organizationId = activeOrganization.organization_id;
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const service = useQuery({
    ...flashServiceQueryOptions(organizationId, serviceId),
    enabled: Boolean(serviceId),
  });
  const quota = useQuery(flashQuotaQueryOptions(organizationId));
  const projects = useQuery(projectsQueryOptions(organizationId));
  const [editOpen, setEditOpen] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [shellOpen, setShellOpen] = useState(false);
  const [shellPod, setShellPod] = useState<string | null>(null);
  const [shellSession, setShellSession] = useState(0);
  const [shellState, setShellState] =
    useState<FlashShellConnectionState>("closed");
  const [editForm, setEditForm] = useState<FlashServiceFormValue | null>(null);
  const registryImages = useQuery({
    ...registryImagesQueryOptions(organizationId),
    enabled: editOpen,
  });
  const containers = useQuery({
    queryKey: [
      "organizations",
      organizationId,
      "flash",
      "services",
      serviceId,
      "containers",
    ],
    queryFn: ({ signal }) =>
      api.flash.services.listContainers(organizationId, serviceId, signal),
    enabled:
      Boolean(serviceId) && shellOpen && service.data?.state === "ready",
    refetchInterval: shellOpen ? 10_000 : false,
  });
  const updateService = useMutation({
    mutationFn: (value: FlashServiceFormValue) =>
      api.flash.services.update(organizationId, serviceId, {
        name: value.name.trim(),
        spec: flashSpecFromForm(value, service.data?.spec.metadata ?? {}),
      }),
    onSuccess: async (updated) => {
      queryClient.setQueryData(
        flashServiceQueryOptions(organizationId, serviceId).queryKey,
        updated,
      );
      setEditOpen(false);
      await queryClient.invalidateQueries({
        queryKey: ["organizations", organizationId, "flash", "services"],
      });
    },
  });
  const deleteService = useMutation({
    mutationFn: () => api.flash.services.delete(organizationId, serviceId),
    onSuccess: async () => {
      await queryClient.invalidateQueries({
        queryKey: ["organizations", organizationId, "flash", "services"],
      });
      navigate("/flash/services", { replace: true });
    },
  });

  if (!serviceId) {
    return (
      <ErrorState
        title="サービスを指定してください"
        description="サービスIDがURLに含まれていません。"
      />
    );
  }
  if (service.isPending || projects.isPending || quota.isPending) {
    return <PageLoading label="Flashサービス詳細を読み込んでいます" />;
  }
  if (service.isError || projects.isError || quota.isError) {
    return (
      <ErrorState
        title="Flashサービスを取得できませんでした"
        description="サービスが存在しないか、参照権限がありません。"
        onRetry={() => {
          void service.refetch();
          void projects.refetch();
          void quota.refetch();
        }}
      />
    );
  }

  const item = service.data;
  const endpoints = flashServiceEndpoints(item.status);
  const projectName =
    projects.data.items.find((project) => project.id === item.project_id)?.name ??
    item.project_id;
  const disabled = item.state === "deleting";
  const providerStatus = flashProviderStatus(item.status);
  const statusMessage =
    typeof providerStatus.message === "string" ? providerStatus.message : null;
  const validationError = editForm
    ? flashFormValidationError(editForm, quota.data)
    : null;
  const environmentKeys = Object.keys(item.spec.env);
  const runningContainers = (containers.data?.items ?? []).filter(
    (container) => container.phase === "Running" && container.ready,
  );
  const selectedPod =
    runningContainers.find((container) => container.name === shellPod)?.name ??
    runningContainers[0]?.name ??
    null;
  const shellStatus = shellStatuses[shellState];
  const submitEdit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (editForm && !validationError) updateService.mutate(editForm);
  };
  const openEditor = () => {
    setEditForm(flashFormFromService(item, registryImages.data?.items));
    updateService.reset();
    setEditOpen(true);
  };
  const allowedSources = item.spec.exposure.allowed_source_cidrs ?? [];
  const deniedSources = item.spec.exposure.denied_source_cidrs ?? [];

  return (
    <SpaceBetween size="l">
      <RouterLink to="/flash/services">Flash</RouterLink>
      <PageHeader
        title={item.name}
        description={`サービスID: ${item.id}`}
        actions={
          <SpaceBetween direction="horizontal" size="xs">
            <Button
              variant="icon"
              iconName="refresh"
              ariaLabel="更新"
              onClick={() => void service.refetch()}
            />
            <Button
              iconName="script"
              disabled={disabled || item.state !== "ready"}
              onClick={() => {
                setShellPod(null);
                setShellSession(0);
                setShellState("closed");
                setShellOpen(true);
              }}
            >
              Web Shell
            </Button>
            <Button
              iconName="edit"
              disabled={disabled}
              onClick={openEditor}
            >
              編集
            </Button>
            <Button
              iconName="remove"
              disabled={disabled}
              onClick={() => {
                deleteService.reset();
                setDeleteOpen(true);
              }}
            >
              削除
            </Button>
          </SpaceBetween>
        }
      />
      <Container>
        <ColumnLayout columns={3} variant="text-grid">
          {[
            ["稼働レプリカ", readyReplicas(item)],
            ["要求レプリカ", item.spec.replicas],
            ["エンドポイント", endpoints.length],
          ].map(([label, value]) => (
            <div key={label}>
              <Box variant="awsui-key-label">{label}</Box>
              <Box variant="awsui-value-large">{formatNumber(Number(value))}</Box>
            </div>
          ))}
        </ColumnLayout>
      </Container>
      {statusMessage ? <Alert type={item.state === "error" ? "error" : "info"}>{statusMessage}</Alert> : null}
      <Container header={<Header variant="h2">エンドポイント</Header>}>
        <FlashEndpoints endpoints={endpoints} />
      </Container>
      <ColumnLayout columns={2}>
        <Container header={<Header variant="h2">サービス設定</Header>}>
          <KeyValuePairs
            columns={2}
            items={[
              { label: "状態", value: <StatusBadge status={item.state} /> },
              { label: "プロジェクト", value: projectName },
              { label: "リージョン", value: item.spec.region },
              { label: "CPU", value: `${formatNumber(item.spec.cpu_millis)} millicores` },
              { label: "メモリ", value: `${formatNumber(item.spec.memory_mib)} MiB` },
              { label: "ディスク上限（イメージ込み）", value: `${formatNumber(item.spec.ephemeral_storage_gib)} GiB` },
              { label: "接続", value: flashExposureLabel(item.spec.exposure) },
              {
                label: "許可IP / CIDR",
                value: allowedSources.length ? (
                  <Box variant="code">{allowedSources.join(", ")}</Box>
                ) : (
                  "すべて"
                ),
              },
              {
                label: "拒否IP / CIDR",
                value: deniedSources.length ? (
                  <Box variant="code">{deniedSources.join(", ")}</Box>
                ) : (
                  "-"
                ),
              },
              { label: "更新日時", value: formatDateTime(item.updated_at) },
            ]}
          />
        </Container>
        <Container header={<Header variant="h2">コンテナ</Header>}>
          <KeyValuePairs
            columns={1}
            items={[
              { label: "イメージ", value: <Box variant="code">{item.spec.image}</Box> },
              { label: "Command", value: item.spec.command.length ? <Box variant="code">{item.spec.command.join(" ")}</Box> : "-" },
              { label: "Args", value: item.spec.args.length ? <Box variant="code">{item.spec.args.join(" ")}</Box> : "-" },
              { label: "環境変数", value: environmentKeys.length ? environmentKeys.join(", ") : "-" },
            ]}
          />
        </Container>
      </ColumnLayout>
      <Container header={<Header variant="h2">エンドポイント設定</Header>}>
        <DataTable
          columns={portColumns}
          data={item.spec.ports}
          getRowId={(port) => `${port.protocol}-${port.name}-${port.service_port}`}
          mobileVisibleColumns={["name", "protocol", "service_port"]}
          searchPlaceholder="名前、プロトコル、ポート番号で検索"
          emptyTitle="エンドポイントがありません"
          emptyDescription="編集画面からエンドポイントを追加してください。"
        />
      </Container>
      <Modal
        visible={shellOpen}
        onDismiss={() => {
          setShellSession(0);
          setShellOpen(false);
        }}
        size="max"
        header={`${item.name} Web Shell`}
        footer={
          <Box float="right">
            <Button
              onClick={() => {
                setShellSession(0);
                setShellOpen(false);
              }}
            >
              閉じる
            </Button>
          </Box>
        }
      >
        <SpaceBetween size="m">
          <SpaceBetween direction="horizontal" size="xs" alignItems="end">
            <Box>
              <Box variant="awsui-key-label">コンテナ</Box>
              <Select
                ariaLabel="コンテナ"
                selectedOption={
                  selectedPod
                    ? { value: selectedPod, label: selectedPod }
                    : null
                }
                options={runningContainers.map((container) => ({
                  value: container.name,
                  label: container.name,
                }))}
                placeholder={containers.isPending ? "取得中" : "稼働中のコンテナなし"}
                loadingText="コンテナを取得しています"
                statusType={containers.isPending ? "loading" : "finished"}
                disabled={containers.isPending || runningContainers.length === 0}
                onChange={({ detail }) => {
                  setShellSession(0);
                  setShellState("closed");
                  setShellPod(detail.selectedOption.value ?? null);
                }}
              />
            </Box>
            <Button
              variant="primary"
              iconName="script"
              disabled={!selectedPod || containers.isPending}
              onClick={() => {
                setShellState("connecting");
                setShellSession((session) => session + 1);
              }}
            >
              {shellSession > 0 ? "再接続" : "接続"}
            </Button>
            <Button
              disabled={shellSession === 0}
              onClick={() => {
                setShellSession(0);
                setShellState("closed");
              }}
            >
              切断
            </Button>
            <StatusIndicator type={shellStatus.type}>
              {shellStatus.label}
            </StatusIndicator>
          </SpaceBetween>
          {containers.isError ? (
            <Alert type="error">コンテナ一覧を取得できませんでした。</Alert>
          ) : null}
          {shellSession > 0 && selectedPod ? (
            <FlashWebShell
              key={`${selectedPod}-${shellSession}`}
              url={api.flash.services.execWebSocketUrl(
                organizationId,
                serviceId,
                selectedPod,
              )}
              onStateChange={setShellState}
            />
          ) : null}
        </SpaceBetween>
      </Modal>
      <Modal
        visible={editOpen}
        onDismiss={() => setEditOpen(false)}
        size="max"
        header="Flashサービスを編集"
        footer={
          <Box float="right">
            <SpaceBetween direction="horizontal" size="xs">
              <Button onClick={() => setEditOpen(false)}>キャンセル</Button>
              <Button
                variant="primary"
                loading={updateService.isPending}
                disabled={!editForm || Boolean(validationError)}
                onClick={() => editForm && updateService.mutate(editForm)}
              >
                変更を保存
              </Button>
            </SpaceBetween>
          </Box>
        }
      >
        {editForm ? (
          <FlashServiceForm
            value={editForm}
            onChange={setEditForm}
            onSubmit={submitEdit}
            disabled={updateService.isPending}
            projectLocked
            registryImages={registryImages.data?.items}
            registryImagesStatus={
              registryImages.isError
                ? "error"
                : registryImages.isPending
                  ? "loading"
                  : "finished"
            }
            quota={quota.data}
          >
            <FormError
              message={
                updateService.isError
                  ? getApiErrorMessage(updateService.error)
                  : validationError
              }
            />
          </FlashServiceForm>
        ) : null}
      </Modal>
      <Modal
        visible={deleteOpen}
        onDismiss={() => setDeleteOpen(false)}
        header="Flashサービスを削除"
        footer={
          <Box float="right">
            <SpaceBetween direction="horizontal" size="xs">
              <Button onClick={() => setDeleteOpen(false)}>キャンセル</Button>
              <Button
                variant="primary"
                iconName="remove"
                loading={deleteService.isPending}
                onClick={() => deleteService.mutate()}
              >
                削除する
              </Button>
            </SpaceBetween>
          </Box>
        }
      >
        <SpaceBetween size="l">
          <Alert type="warning" header="この操作は取り消せません">
            {item.name} のコンテナ、Service、公開エンドポイントを削除します。
          </Alert>
          <FormError
            message={
              deleteService.isError
                ? getApiErrorMessage(deleteService.error)
                : null
            }
          />
        </SpaceBetween>
      </Modal>
    </SpaceBetween>
  );
}
