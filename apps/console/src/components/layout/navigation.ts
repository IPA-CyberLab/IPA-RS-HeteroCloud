import type { SideNavigationProps } from "@/components/ui";

const baseNavigationItems: SideNavigationProps.Item[] = [
  { type: "link", text: "コンソールホーム", href: "/overview" },
  { type: "divider" },
  {
    type: "section",
    text: "開発ツール",
    items: [{ type: "link", text: "CLIセットアップ", href: "/cli" }],
  },
  {
    type: "section",
    text: "Flash",
    items: [
      { type: "link", text: "サービス", href: "/flash/services" },
      { type: "link", text: "イメージ", href: "/registry" },
      { type: "link", text: "バックアップ", href: "/flash/backups" },
      { type: "link", text: "通知", href: "/flash/alerts" },
      { type: "link", text: "コスト管理", href: "/cost-management" },
    ],
  },
  { type: "link", text: "VPC", href: "/vpc/networks" },
  { type: "link", text: "Flow", href: "/flow/services" },
  { type: "link", text: "Syouyu", href: "/syouyu/buckets" },
  {
    type: "section",
    text: "管理",
    items: [
      { type: "link", text: "組織", href: "/organizations" },
      { type: "link", text: "リソース上限", href: "/organizations/quotas" },
      { type: "link", text: "プロジェクト", href: "/projects" },
      { type: "link", text: "IAMプリンシパル", href: "/iam/principals" },
      { type: "link", text: "IAMポリシー", href: "/iam/policies" },
      { type: "link", text: "IAMバインディング", href: "/iam/bindings" },
      { type: "link", text: "監査ログ", href: "/audit-logs" },
      { type: "link", text: "設定", href: "/settings" },
    ],
  },
];

export function navigationItems(ownerConsole: boolean): SideNavigationProps.Item[] {
  if (!ownerConsole) return baseNavigationItems;
  return [
    { type: "link", text: "全アカウント管理", href: "/overview" },
    { type: "link", text: "コスト管理", href: "/owner/cost-management" },
    { type: "link", text: "GPU管理", href: "/owner/gpus" },
    { type: "link", text: "サービスの通知", href: "/owner/flash/alerts" },
  ];
}

export const routeTitles: Record<string, string> = {
  "/overview": "コンソールホーム",
  "/organizations": "組織",
  "/organizations/quotas": "リソース上限",
  "/projects": "プロジェクト",
  "/iam/principals": "プリンシパル",
  "/iam/policies": "ポリシー",
  "/iam/bindings": "バインディング",
  "/flow/services": "Flow",
  "/flash/services": "Flash",
  "/flash/backups": "Flashバックアップ",
  "/flash/alerts": "Flashの通知",
  "/owner/flash/alerts": "全サービスの通知",
  "/vpc/networks": "VPC",
  "/registry": "イメージ",
  "/cost-management": "コスト管理",
  "/syouyu/buckets": "Syouyu",
  "/audit-logs": "監査ログ",
  "/settings": "設定",
  "/cli": "CLIセットアップ",
  "/owner/quotas": "全アカウント管理",
  "/owner/gpus": "GPU管理",
  "/owner/cost-management": "コスト管理",
};
