import StatusIndicator, {
  type StatusIndicatorProps,
} from "@cloudscape-design/components/status-indicator";

const labels: Record<string, string> = {
  active: "有効",
  pending: "準備中",
  suspended: "停止中",
  deleting: "削除中",
  error: "エラー",
  invited: "招待済み",
  disabled: "無効",
  provisioning: "構築中",
  ready: "準備完了",
  updating: "更新中",
  running: "稼働中",
  degraded: "縮退",
  stopped: "停止",
  failed: "失敗",
  success: "成功",
  allow: "許可",
  deny: "拒否",
  denied: "拒否",
};

const types: Record<string, StatusIndicatorProps.Type> = {
  active: "success",
  running: "success",
  ready: "success",
  success: "success",
  allow: "success",
  pending: "pending",
  provisioning: "in-progress",
  updating: "in-progress",
  invited: "info",
  suspended: "warning",
  degraded: "warning",
  stopped: "stopped",
  disabled: "stopped",
  deleting: "in-progress",
  error: "error",
  failed: "error",
  denied: "error",
  deny: "error",
};

export function StatusBadge({ status }: { status: string }) {
  return (
    <StatusIndicator type={types[status] ?? "info"}>
      {labels[status] ?? status}
    </StatusIndicator>
  );
}
