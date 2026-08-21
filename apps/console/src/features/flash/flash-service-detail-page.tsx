import Alert from "@cloudscape-design/components/alert";
import Box from "@cloudscape-design/components/box";
import Button from "@cloudscape-design/components/button";
import ColumnLayout from "@cloudscape-design/components/column-layout";
import Container from "@cloudscape-design/components/container";
import Header from "@cloudscape-design/components/header";
import KeyValuePairs from "@cloudscape-design/components/key-value-pairs";
import Modal from "@cloudscape-design/components/modal";
import SpaceBetween from "@cloudscape-design/components/space-between";
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
  flashServiceQueryOptions,
  projectsQueryOptions,
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
  const projects = useQuery(projectsQueryOptions(organizationId));
  const [editOpen, setEditOpen] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [editForm, setEditForm] = useState<FlashServiceFormValue | null>(null);
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
  if (service.isPending || projects.isPending) {
    return <PageLoading label="Flashサービス詳細を読み込んでいます" />;
  }
  if (service.isError || projects.isError) {
    return (
      <ErrorState
        title="Flashサービスを取得できませんでした"
        description="サービスが存在しないか、参照権限がありません。"
        onRetry={() => {
          void service.refetch();
          void projects.refetch();
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
  const validationError = editForm ? flashFormValidationError(editForm) : null;
  const environmentKeys = Object.keys(item.spec.env);
  const submitEdit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (editForm && !validationError) updateService.mutate(editForm);
  };

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
              iconName="edit"
              disabled={disabled}
              onClick={() => {
                setEditForm(flashFormFromService(item));
                updateService.reset();
                setEditOpen(true);
              }}
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
        <ColumnLayout columns={4} variant="text-grid">
          {[
            ["稼働レプリカ", readyReplicas(item)],
            ["要求レプリカ", item.spec.replicas],
            ["エンドポイント", endpoints.length],
            ["世代", item.generation],
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
              { label: "ランタイム", value: "gVisor（強制）" },
              { label: "CPU", value: `${formatNumber(item.spec.cpu_millis)} millicores` },
              { label: "メモリ", value: `${formatNumber(item.spec.memory_mib)} MiB` },
              { label: "接続", value: flashExposureLabel(item.spec.exposure) },
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
      <Container header={<Header variant="h2">ポート</Header>}>
        <DataTable
          columns={portColumns}
          data={item.spec.ports}
          getRowId={(port) => `${port.protocol}-${port.name}-${port.service_port}`}
          mobileVisibleColumns={["name", "protocol", "service_port"]}
          searchPlaceholder="名前、プロトコル、ポート番号で検索"
          emptyTitle="ポートがありません"
          emptyDescription="編集画面からポートを追加してください。"
        />
      </Container>
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
