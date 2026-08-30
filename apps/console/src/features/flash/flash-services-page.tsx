import Box from "@cloudscape-design/components/box";
import Button from "@cloudscape-design/components/button";
import ColumnLayout from "@cloudscape-design/components/column-layout";
import Container from "@cloudscape-design/components/container";
import Modal from "@cloudscape-design/components/modal";
import SpaceBetween from "@cloudscape-design/components/space-between";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { ColumnDef } from "@tanstack/react-table";
import { type FormEvent, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { DataTable } from "@/components/shared/data-table";
import { ErrorState } from "@/components/shared/error-state";
import { FormError } from "@/components/shared/form-error";
import { PageHeader } from "@/components/shared/page-header";
import { PageLoading } from "@/components/shared/page-loading";
import { StatusBadge } from "@/components/shared/status-badge";
import { useActiveOrganization } from "@/features/organizations/organization-context";
import { api, getApiErrorMessage } from "@/lib/api-client";
import type { FlashService } from "@/lib/api-types";
import {
  flashQuotaQueryOptions,
  flashServicesQueryOptions,
  projectsQueryOptions,
  registryImagesQueryOptions,
} from "@/lib/queries";
import { formatDateTime, formatNumber } from "@/lib/utils";
import {
  defaultFlashServiceFormValue,
  FlashServiceForm,
  flashFormValidationError,
  flashSpecFromForm,
  type FlashServiceFormValue,
} from "./flash-service-form";
import {
  flashExposureLabel,
  flashServiceEndpoints,
  readyReplicas,
} from "./flash-service-utils";

export function FlashServicesPage() {
  const { activeOrganization } = useActiveOrganization();
  const organizationId = activeOrganization.organization_id;
  const services = useQuery(flashServicesQueryOptions(organizationId));
  const quota = useQuery(flashQuotaQueryOptions(organizationId));
  const projects = useQuery(projectsQueryOptions(organizationId));
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [createOpen, setCreateOpen] = useState(false);
  const [form, setForm] = useState<FlashServiceFormValue>(
    defaultFlashServiceFormValue,
  );
  const registryImages = useQuery({
    ...registryImagesQueryOptions(organizationId),
    enabled: createOpen,
  });
  const createService = useMutation({
    mutationFn: (value: FlashServiceFormValue) =>
      api.flash.services.create(organizationId, {
        project_id: value.projectId,
        name: value.name.trim(),
        spec: flashSpecFromForm(value),
      }),
    onSuccess: async (created) => {
      setCreateOpen(false);
      setForm(defaultFlashServiceFormValue);
      await queryClient.invalidateQueries({
        queryKey: ["organizations", organizationId, "flash", "services"],
      });
      navigate(`/flash/services/${created.id}`);
    },
  });
  const projectNames = useMemo(
    () =>
      new Map(
        (projects.data?.items ?? []).map((project) => [project.id, project.name]),
      ),
    [projects.data],
  );
  const columns = useMemo<ColumnDef<FlashService, unknown>[]>(
    () => [
      {
        accessorKey: "name",
        header: "サービス",
        cell: ({ row }) => (
          <SpaceBetween size="xxs">
            <Box fontWeight="bold">{row.original.name}</Box>
            <Box variant="code" className="mobile-hidden">
              {row.original.id}
            </Box>
          </SpaceBetween>
        ),
      },
      {
        id: "project",
        accessorFn: (service) =>
          projectNames.get(service.project_id) ?? service.project_id,
        header: "プロジェクト",
        cell: ({ row }) =>
          projectNames.get(row.original.project_id) ?? (
            <Box variant="code">{row.original.project_id}</Box>
          ),
      },
      {
        accessorKey: "state",
        header: "状態",
        cell: ({ getValue }) => (
          <StatusBadge status={getValue<FlashService["state"]>()} />
        ),
      },
      {
        id: "image",
        header: "イメージ",
        accessorFn: (service) => service.spec.image,
        cell: ({ row }) => <Box variant="code">{row.original.spec.image}</Box>,
      },
      {
        id: "replicas",
        header: "レプリカ",
        accessorFn: (service) => service.spec.replicas,
        cell: ({ row }) =>
          `${formatNumber(readyReplicas(row.original))} / ${formatNumber(row.original.spec.replicas)}`,
      },
      {
        id: "exposure",
        header: "接続",
        accessorFn: (service) => flashExposureLabel(service.spec.exposure),
      },
      {
        id: "ports",
        header: "ポート",
        accessorFn: (service) =>
          service.spec.ports
            .map((port) => `${port.protocol.toUpperCase()}/${port.service_port}`)
            .join(", "),
      },
      {
        accessorKey: "updated_at",
        header: "更新日時",
        cell: ({ getValue }) => formatDateTime(getValue<string>()),
      },
    ],
    [projectNames],
  );
  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!flashFormValidationError(form, quota.data)) createService.mutate(form);
  };

  if (services.isPending || projects.isPending || quota.isPending) {
    return <PageLoading label="Flashを読み込んでいます" />;
  }
  if (services.isError || projects.isError || quota.isError) {
    return (
      <ErrorState
        description="Flashサービスまたはプロジェクト一覧を取得できませんでした。"
        onRetry={() => {
          void services.refetch();
          void projects.refetch();
          void quota.refetch();
        }}
      />
    );
  }

  const serviceItems = services.data.items;
  const readyCount = serviceItems.filter((service) => service.state === "ready").length;
  const replicaCount = serviceItems.reduce(
    (total, service) => total + readyReplicas(service),
    0,
  );
  const endpointCount = serviceItems.reduce(
    (total, service) => total + flashServiceEndpoints(service.status).length,
    0,
  );
  const validationError = flashFormValidationError(form, quota.data);

  return (
    <SpaceBetween size="l">
      <PageHeader
        title="Flash"
        description={`${activeOrganization.organization_name} のコンテナサービスを管理します。`}
        actions={
          <SpaceBetween direction="horizontal" size="xs">
            <Button
              variant="icon"
              iconName="refresh"
              ariaLabel="更新"
              onClick={() =>
                void queryClient.invalidateQueries({
                  queryKey: ["organizations", organizationId, "flash", "services"],
                })
              }
            />
            <Button
              variant="primary"
              iconName="add-plus"
              onClick={() => {
                createService.reset();
                setCreateOpen(true);
              }}
            >
              サービスを作成
            </Button>
          </SpaceBetween>
        }
      />
      <Container>
        <ColumnLayout columns={3} variant="text-grid">
          {[
            ["準備完了", readyCount],
            ["稼働レプリカ", replicaCount],
            ["エンドポイント", endpointCount],
          ].map(([label, value]) => (
            <div key={label}>
              <Box variant="awsui-key-label">{label}</Box>
              <Box variant="awsui-value-large">{formatNumber(Number(value))}</Box>
            </div>
          ))}
        </ColumnLayout>
      </Container>
      <DataTable
        columns={columns}
        data={serviceItems}
        getRowId={(service) => service.id}
        onRowClick={(service) => navigate(`/flash/services/${service.id}`)}
        getRowAriaLabel={(service) => `${service.name}の詳細を開く`}
        mobileVisibleColumns={["name", "state", "replicas"]}
        searchPlaceholder="名前、プロジェクト、イメージ、状態で検索"
        emptyTitle="Flashサービスがありません"
        emptyDescription="コンテナサービスを作成してください。"
      />
      <Modal
        visible={createOpen}
        onDismiss={() => setCreateOpen(false)}
        size="max"
        header="Flashサービスを作成"
        footer={
          <Box float="right">
            <SpaceBetween direction="horizontal" size="xs">
              <Button onClick={() => setCreateOpen(false)}>キャンセル</Button>
              <Button
                variant="primary"
                loading={createService.isPending}
                disabled={Boolean(validationError)}
                onClick={() => createService.mutate(form)}
              >
                作成
              </Button>
            </SpaceBetween>
          </Box>
        }
      >
        <FlashServiceForm
          value={form}
          onChange={setForm}
          onSubmit={submit}
          disabled={createService.isPending}
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
              createService.isError
                ? getApiErrorMessage(createService.error)
                : validationError
            }
          />
        </FlashServiceForm>
      </Modal>
    </SpaceBetween>
  );
}
