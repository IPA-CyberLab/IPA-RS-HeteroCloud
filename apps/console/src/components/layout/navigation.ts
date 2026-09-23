import type { SideNavigationProps } from "@cloudscape-design/components/side-navigation";

const baseNavigationItems: SideNavigationProps.Item[] = [
  { type: "link", text: "コンソールホーム", href: "/overview" },
  { type: "divider" },
  {
    type: "section",
    text: "Flash",
    items: [
      { type: "link", text: "サービス", href: "/flash/services" },
      { type: "link", text: "イメージ", href: "/registry" },
    ],
  },
  {
    type: "section",
    text: "その他のサービス",
    items: [
      { type: "link", text: "Flow", href: "/flow/services" },
      { type: "link", text: "Syouyu", href: "/syouyu/buckets" },
    ],
  },
  {
    type: "section",
    text: "管理",
    items: [
      { type: "link", text: "組織", href: "/organizations" },
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
    { type: "link", text: "GPU管理", href: "/owner/gpus" },
  ];
}

export const routeTitles: Record<string, string> = {
  "/overview": "コンソールホーム",
  "/organizations": "組織",
  "/projects": "プロジェクト",
  "/iam/principals": "プリンシパル",
  "/iam/policies": "ポリシー",
  "/iam/bindings": "バインディング",
  "/flow/services": "Flow",
  "/flash/services": "Flash",
  "/registry": "イメージ",
  "/syouyu/buckets": "Syouyu",
  "/audit-logs": "監査ログ",
  "/settings": "設定",
  "/owner/quotas": "全アカウント管理",
  "/owner/gpus": "GPU管理",
};
