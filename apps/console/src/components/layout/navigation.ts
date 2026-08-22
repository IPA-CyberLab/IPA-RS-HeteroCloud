import type { SideNavigationProps } from "@cloudscape-design/components/side-navigation";

const baseNavigationItems: SideNavigationProps.Item[] = [
  { type: "link", text: "概要", href: "/overview" },
  { type: "link", text: "組織", href: "/organizations" },
  { type: "link", text: "プロジェクト", href: "/projects" },
  { type: "divider" },
  {
    type: "section",
    text: "IAM",
    items: [
      { type: "link", text: "プリンシパル", href: "/iam/principals" },
      { type: "link", text: "ポリシー", href: "/iam/policies" },
      { type: "link", text: "バインディング", href: "/iam/bindings" },
    ],
  },
  {
    type: "section",
    text: "サービス",
    items: [
      { type: "link", text: "Flow", href: "/flow/services" },
      { type: "link", text: "Flash", href: "/flash/services" },
      { type: "link", text: "Flash Registry", href: "/registry" },
    ],
  },
  {
    type: "section",
    text: "運用",
    items: [
      { type: "link", text: "監査ログ", href: "/audit-logs" },
      { type: "link", text: "設定", href: "/settings" },
    ],
  },
];

export function navigationItems(ownerConsole: boolean): SideNavigationProps.Item[] {
  if (!ownerConsole) return baseNavigationItems;
  return [
    ...baseNavigationItems,
    { type: "divider" },
    {
      type: "section",
      text: "所有者管理",
      items: [{ type: "link", text: "ハードリミット", href: "/owner/quotas" }],
    },
  ];
}

export const routeTitles: Record<string, string> = {
  "/overview": "概要",
  "/organizations": "組織",
  "/projects": "プロジェクト",
  "/iam/principals": "プリンシパル",
  "/iam/policies": "ポリシー",
  "/iam/bindings": "バインディング",
  "/flow/services": "Flow",
  "/flash/services": "Flash",
  "/registry": "Flash Registry",
  "/audit-logs": "監査ログ",
  "/settings": "設定",
  "/owner/quotas": "所有者設定",
};
