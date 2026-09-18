import Badge from "@cloudscape-design/components/badge";
import Box from "@cloudscape-design/components/box";
import Button from "@cloudscape-design/components/button";
import FormField from "@cloudscape-design/components/form-field";
import Header from "@cloudscape-design/components/header";
import Modal from "@cloudscape-design/components/modal";
import Multiselect from "@cloudscape-design/components/multiselect";
import SegmentedControl from "@cloudscape-design/components/segmented-control";
import SpaceBetween from "@cloudscape-design/components/space-between";
import StatusIndicator from "@cloudscape-design/components/status-indicator";
import Table from "@cloudscape-design/components/table";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { ErrorState } from "@/components/shared/error-state";
import { FormError } from "@/components/shared/form-error";
import { PageHeader } from "@/components/shared/page-header";
import { PageLoading } from "@/components/shared/page-loading";
import { api, getApiErrorMessage } from "@/lib/api-client";
import type { GpuAccess, OwnerGpu } from "@/lib/api-types";
import {
  ownerAccountsQueryOptions,
  ownerGpusQueryOptions,
} from "@/lib/queries";
import { formatDateTime } from "@/lib/utils";

export function OwnerGpusPage() {
  const gpus = useQuery(ownerGpusQueryOptions);
  const accounts = useQuery(ownerAccountsQueryOptions);
  const queryClient = useQueryClient();
  const [selectedGpu, setSelectedGpu] = useState<OwnerGpu | null>(null);
  const [visibility, setVisibility] = useState<GpuAccess>("open");
  const [assignedUserIds, setAssignedUserIds] = useState<string[]>([]);

  const accountOptions = useMemo(
    () =>
      (accounts.data?.items ?? [])
        .filter((account) => account.user.status === "active")
        .map((account) => ({
          value: account.user.id,
          label: account.user.display_name,
          description: account.user.email,
        })),
    [accounts.data],
  );
  const accountNames = useMemo(
    () =>
      new Map(
        (accounts.data?.items ?? []).map((account) => [
          account.user.id,
          account.user.status === "active"
            ? account.user.display_name
            : `${account.user.display_name}（無効）`,
        ]),
      ),
    [accounts.data],
  );

  const updateAccess = useMutation({
    mutationFn: () => {
      if (!selectedGpu) throw new Error("GPUが選択されていません。");
      return api.owner.gpus.update(selectedGpu.id, {
        visibility,
        assigned_user_ids: visibility === "open" ? [] : assignedUserIds,
      });
    },
    onSuccess: async () => {
      setSelectedGpu(null);
      await queryClient.invalidateQueries({ queryKey: ["owner", "gpus"] });
    },
  });

  const openAccessEditor = (gpu: OwnerGpu) => {
    updateAccess.reset();
    setSelectedGpu(gpu);
    setVisibility(gpu.visibility);
    const activeUserIds = new Set(accountOptions.map((option) => option.value));
    setAssignedUserIds(gpu.assigned_user_ids.filter((id) => activeUserIds.has(id)));
  };

  if (gpus.isPending || accounts.isPending) {
    return <PageLoading label="GPUカタログを読み込んでいます" />;
  }
  if (gpus.isError || accounts.isError) {
    return (
      <ErrorState
        description="GPUカタログまたはユーザー一覧を取得できませんでした。"
        onRetry={() => {
          void gpus.refetch();
          void accounts.refetch();
        }}
      />
    );
  }

  return (
    <SpaceBetween size="l">
      <PageHeader
        title="GPU管理"
        description="Flash providerから同期された物理GPUの公開範囲を管理します。"
        actions={
          <Button iconName="refresh" onClick={() => void gpus.refetch()}>
            更新
          </Button>
        }
      />
      <Table
        variant="container"
        header={
          <Header
            variant="h2"
            counter={`(${gpus.data.items.length})`}
            description="識別情報とGPU種類はproviderが管理します。ここでは利用範囲だけを変更できます。"
          >
            GPUインベントリ
          </Header>
        }
        items={gpus.data.items}
        trackBy="id"
        columnDefinitions={[
          {
            id: "gpu",
            header: "GPU",
            cell: (gpu) => (
              <SpaceBetween size="xxs">
                <Box fontWeight="bold">{gpu.display_name}</Box>
                <Box variant="code">{gpu.gpu_type}</Box>
              </SpaceBetween>
            ),
          },
          {
            id: "management-id",
            header: "管理ID",
            cell: (gpu) => <Box variant="code">{gpu.management_id}</Box>,
          },
          {
            id: "visibility",
            header: "公開範囲",
            cell: (gpu) => (
              <Badge color={gpu.visibility === "open" ? "green" : "blue"}>
                {gpu.visibility === "open" ? "Open" : "Private"}
              </Badge>
            ),
          },
          {
            id: "availability",
            header: "状態",
            cell: (gpu) => (
              <StatusIndicator type={gpu.available ? "success" : "warning"}>
                {gpu.available ? "利用可能" : "使用中または異常"}
              </StatusIndicator>
            ),
          },
          {
            id: "users",
            header: "割り当てユーザー",
            cell: (gpu) =>
              gpu.visibility === "open"
                ? "すべてのユーザー"
                : gpu.assigned_user_ids
                    .map((id) => accountNames.get(id) ?? id)
                    .join(", ") || "未割り当て",
          },
          {
            id: "updated",
            header: "更新日時",
            cell: (gpu) => formatDateTime(gpu.updated_at),
          },
          {
            id: "actions",
            header: "",
            cell: (gpu) => (
              <Button ariaLabel={`${gpu.display_name}のアクセス設定`} onClick={() => openAccessEditor(gpu)}>
                アクセス設定
              </Button>
            ),
          },
        ]}
        empty={
          <Box textAlign="center" color="text-body-secondary" padding="l">
            providerから同期されたGPUはありません。
          </Box>
        }
      />

      <Modal
        visible={Boolean(selectedGpu)}
        onDismiss={() => setSelectedGpu(null)}
        header={`${selectedGpu?.display_name ?? "GPU"} のアクセス設定`}
        footer={
          <Box float="right">
            <SpaceBetween direction="horizontal" size="xs">
              <Button onClick={() => setSelectedGpu(null)}>キャンセル</Button>
              <Button
                variant="primary"
                loading={updateAccess.isPending}
                onClick={() => updateAccess.mutate()}
              >
                保存
              </Button>
            </SpaceBetween>
          </Box>
        }
      >
        <SpaceBetween size="l">
          <FormField
            label="公開範囲"
            description="Openはすべてのユーザー、Privateは選択したアクティブユーザーだけがFlashで利用できます。"
          >
            <SegmentedControl
              label="公開範囲"
              selectedId={visibility}
              options={[
                { id: "open", text: "Open" },
                { id: "private", text: "Private" },
              ]}
              onChange={({ detail }) => {
                const next = detail.selectedId as GpuAccess;
                setVisibility(next);
                if (next === "open") setAssignedUserIds([]);
              }}
            />
          </FormField>
          {visibility === "private" ? (
            <FormField
              label="割り当てユーザー"
              description="未割り当てのまま保存すると、このGPUはどのユーザーにも表示されません。"
            >
              <Multiselect
                ariaLabel="割り当てユーザー"
                selectedAriaLabel="選択済み"
                selectedOptions={accountOptions.filter((option) =>
                  assignedUserIds.includes(option.value),
                )}
                options={accountOptions}
                filteringType="auto"
                filteringPlaceholder="名前またはメールアドレスで検索"
                placeholder="ユーザーを選択"
                empty="割り当て可能なユーザーがいません"
                onChange={({ detail }) =>
                  setAssignedUserIds(
                    detail.selectedOptions.flatMap((option) =>
                      option.value ? [option.value] : [],
                    ),
                  )
                }
              />
            </FormField>
          ) : null}
          <FormError
            message={updateAccess.isError ? getApiErrorMessage(updateAccess.error) : null}
          />
        </SpaceBetween>
      </Modal>
    </SpaceBetween>
  );
}
