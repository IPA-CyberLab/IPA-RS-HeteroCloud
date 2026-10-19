import Alert from "@cloudscape-design/components/alert";
import Box from "@cloudscape-design/components/box";
import Button from "@cloudscape-design/components/button";
import Container from "@cloudscape-design/components/container";
import FormField from "@cloudscape-design/components/form-field";
import Header from "@cloudscape-design/components/header";
import Input from "@cloudscape-design/components/input";
import Modal from "@cloudscape-design/components/modal";
import Select from "@cloudscape-design/components/select";
import SpaceBetween from "@cloudscape-design/components/space-between";
import StatusIndicator from "@cloudscape-design/components/status-indicator";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { ColumnDef } from "@tanstack/react-table";
import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { DataTable } from "@/components/shared/data-table";
import { api, getApiErrorMessage } from "@/lib/api-client";
import type { FlashBackup } from "@/lib/api-types";
import { projectsQueryOptions } from "@/lib/queries";
import { formatDateTime } from "@/lib/utils";

const phases: Record<FlashBackup["phase"], string> = {
  pending: "受付済み", creating: "保存中", ready: "復元可能", error: "保存失敗", deleting: "削除中",
};

export function FlashBackupsPanel({ organizationId, serviceId }: { organizationId: string; serviceId?: string }) {
  const navigate = useNavigate();
  const client = useQueryClient();
  const key = ["organizations", organizationId, "flash", "backups", serviceId ?? "all"];
  const query = useQuery({ queryKey: key, queryFn: ({ signal }) => api.flash.backups.list(organizationId, serviceId, signal), refetchInterval: 5000 });
  const projects = useQuery(projectsQueryOptions(organizationId));
  const [name, setName] = useState("");
  const [createRequestId, setCreateRequestId] = useState(() => crypto.randomUUID());
  const [selected, setSelected] = useState<FlashBackup | null>(null);
  const [operation, setOperation] = useState<"restore" | "delete">("restore");
  const [restoreName, setRestoreName] = useState("");
  const [projectId, setProjectId] = useState("");
  const [restoreRequestId, setRestoreRequestId] = useState(() => crypto.randomUUID());
  const invalidate = () => client.invalidateQueries({ queryKey: ["organizations", organizationId, "flash", "backups"] });
  const create = useMutation({
    mutationFn: () => api.flash.backups.create(organizationId, serviceId!, name.trim(), createRequestId),
    onSuccess: () => { setName(""); setCreateRequestId(crypto.randomUUID()); void invalidate(); },
  });
  const remove = useMutation({ mutationFn: (id: string) => api.flash.backups.delete(organizationId, id),
    onSuccess: () => { setSelected(null); void invalidate(); } });
  const restore = useMutation({
    mutationFn: () => api.flash.backups.restore(organizationId, selected!.id, { name: restoreName.trim(), project_id: projectId, request_id: restoreRequestId }),
    onSuccess: (service) => { setSelected(null); navigate(`/flash/services/${service.id}`); },
  });
  const choose = (backup: FlashBackup, next: "restore" | "delete") => {
    remove.reset(); restore.reset(); setSelected(backup); setOperation(next);
    setRestoreName(`${backup.source_name.slice(0, 48)}-restore`);
    setProjectId(projects.data?.items.some(p => p.id === backup.project_id) ? backup.project_id : projects.data?.items[0]?.id ?? "");
    setRestoreRequestId(crypto.randomUUID());
  };
  const columns: ColumnDef<FlashBackup, unknown>[] = [
    { accessorKey: "name", header: "名前" },
    { accessorKey: "source_name", header: "保存元サービス" },
    { accessorKey: "phase", header: "状態", cell: ({ row }) => <StatusIndicator type={row.original.phase === "ready" ? "success" : row.original.phase === "error" ? "error" : "loading"}>{phases[row.original.phase]}</StatusIndicator> },
    { accessorKey: "volume_size_bytes", header: "対象ディスク容量", cell: ({ row }) => `${(row.original.volume_size_bytes / 1024 ** 3).toLocaleString("ja-JP", { maximumFractionDigits: 2 })} GiB` },
    { accessorKey: "created_at", header: "受付日時", cell: ({ row }) => formatDateTime(row.original.created_at) },
    { id: "actions", header: "操作", cell: ({ row }) => <SpaceBetween direction="horizontal" size="s">
      <Button disabled={row.original.phase !== "ready" || row.original.delete_requested} onClick={() => choose(row.original, "restore")}>復元</Button>
      <Button disabled={row.original.delete_requested} onClick={() => choose(row.original, "delete")}>削除</Button>
    </SpaceBetween> },
  ];
  const error = create.error || query.error;
  const modalError = restore.error || remove.error;
  const busy = restore.isPending || remove.isPending;
  const projectOptions = projects.data?.items.map(p => ({ label: p.name, value: p.id })) ?? [];
  return <Container header={<Header variant="h2" actions={<Button loading={query.isFetching} onClick={() => void query.refetch()}>更新</Button>}>永続ディスクのバックアップ</Header>}>
    <SpaceBetween size="m">
      <Box>/root の内容を保存します。サービスを削除してもバックアップは残ります。環境変数、シークレット接続、追加マウントは復元対象に含みません。</Box>
      {serviceId && <>
        <Box>稼働中の保存はクラッシュ整合性です。データベースなど、アプリケーション側で書き込みの停止が必要なデータは、サービスを停止してから保存してください。</Box>
        <SpaceBetween direction="horizontal" size="s">
          <FormField label="バックアップ名">
            <Input ariaLabel="バックアップ名" value={name} disabled={create.isPending} onChange={({ detail }) => { setName(detail.value); setCreateRequestId(crypto.randomUUID()); }} />
          </FormField>
          <Button variant="primary" disabled={!name.trim()} loading={create.isPending} onClick={() => create.mutate()}>バックアップを作成</Button>
        </SpaceBetween>
      </>}
      {error && <Alert type="error">{getApiErrorMessage(error)}</Alert>}
      {create.isSuccess && <Alert type="success">バックアップを受け付けました。「復元可能」になるまでお待ちください。</Alert>}
      {query.isPending && <StatusIndicator type="loading">バックアップを読み込み中</StatusIndicator>}
      {query.data && <DataTable columns={columns} data={query.data.items} getRowId={row => row.id} emptyTitle="バックアップはありません" emptyDescription="サービス詳細からバックアップを作成できます。" />}
      {query.data?.items.some(b => b.status.provider_unavailable) && <Alert type="warning">バックアップ保存先の状態を確認できません。表示は最後に取得した状態です。復元時には保存先への接続を再確認します。</Alert>}
      {query.data?.items.some(b => b.phase === "error") && <Alert type="error">保存に失敗したバックアップがあります。失敗したバックアップは削除し、保存元と容量を確認して再作成してください。</Alert>}
      <Modal visible={Boolean(selected)} onDismiss={() => !busy && setSelected(null)} header={operation === "restore" ? "バックアップから復元" : "バックアップを削除"}
        footer={<SpaceBetween direction="horizontal" size="s">
          <Button disabled={busy} onClick={() => setSelected(null)}>キャンセル</Button>
          <Button variant="primary" loading={busy} disabled={operation === "restore" && (!restoreName.trim() || !projectId)} onClick={() => operation === "restore" ? restore.mutate() : selected && remove.mutate(selected.id)}>{operation === "restore" ? "新しいサービスへ復元" : "バックアップを削除"}</Button>
        </SpaceBetween>}>
        <SpaceBetween size="m">
          <Box>バックアップ: {selected?.name}</Box>
          {operation === "restore" ? <>
            <Box>新しいディスクと停止状態の非公開サービスを作成します。元サービスと元ディスクは保持されます。復元後にネットワーク、GPU、タスクロール、シークレット接続を設定してから起動してください。</Box>
            <FormField label="復元先サービス名"><Input ariaLabel="復元先サービス名" value={restoreName} disabled={busy} onChange={({ detail }) => { setRestoreName(detail.value); setRestoreRequestId(crypto.randomUUID()); }} /></FormField>
            <FormField label="プロジェクト"><Select ariaLabel="復元先プロジェクト" selectedOption={projectOptions.find(p => p.value === projectId) ?? null} options={projectOptions} disabled={busy} onChange={({ detail }) => { setProjectId(detail.selectedOption.value ?? ""); setRestoreRequestId(crypto.randomUUID()); }} /></FormField>
          </> : <Box>このバックアップを完全に削除します。保存元や復元済みのディスクは保持されます。</Box>}
          {modalError && <Alert type="error">{getApiErrorMessage(modalError)}</Alert>}
        </SpaceBetween>
      </Modal>
    </SpaceBetween>
  </Container>;
}
