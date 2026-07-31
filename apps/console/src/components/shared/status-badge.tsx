import { Badge, type BadgeProps } from "@/components/ui/badge";

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

const variants: Record<string, BadgeProps["variant"]> = {
  active: "success",
  running: "success",
  ready: "success",
  success: "success",
  allow: "success",
  pending: "info",
  provisioning: "info",
  updating: "info",
  invited: "info",
  suspended: "warning",
  degraded: "warning",
  stopped: "neutral",
  disabled: "neutral",
  deleting: "neutral",
  error: "danger",
  failed: "danger",
  denied: "danger",
  deny: "danger",
};

export function StatusBadge({ status }: { status: string }) {
  return (
    <Badge variant={variants[status] ?? "neutral"}>
      {labels[status] ?? status}
    </Badge>
  );
}
