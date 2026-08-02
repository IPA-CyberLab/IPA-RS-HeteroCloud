import {
  Activity,
  Boxes,
  Building2,
  FileClock,
  Gauge,
  KeyRound,
  Link2,
  RadioTower,
  Settings,
  Users,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";

export interface NavigationItem {
  label: string;
  to: string;
  icon: LucideIcon;
}

export interface NavigationSection {
  label?: string;
  items: NavigationItem[];
}

export const navigationSections: NavigationSection[] = [
  {
    items: [
      { label: "概要", to: "/overview", icon: Gauge },
      { label: "組織", to: "/organizations", icon: Building2 },
      { label: "プロジェクト", to: "/projects", icon: Boxes },
    ],
  },
  {
    label: "IAM",
    items: [
      { label: "プリンシパル", to: "/iam/principals", icon: Users },
      { label: "ポリシー", to: "/iam/policies", icon: KeyRound },
      { label: "バインディング", to: "/iam/bindings", icon: Link2 },
    ],
  },
  {
    label: "サービス",
    items: [
      {
        label: "Flow",
        to: "/flow/services",
        icon: RadioTower,
      },
    ],
  },
  {
    label: "運用",
    items: [
      { label: "監査ログ", to: "/audit-logs", icon: FileClock },
      { label: "設定", to: "/settings", icon: Settings },
    ],
  },
];

export const routeTitles: Record<string, string> = {
  "/overview": "概要",
  "/organizations": "組織",
  "/projects": "プロジェクト",
  "/iam/principals": "IAM / プリンシパル",
  "/iam/policies": "IAM / ポリシー",
  "/iam/bindings": "IAM / バインディング",
  "/flow/services": "Flow",
  "/audit-logs": "監査ログ",
  "/settings": "設定",
};

export const HeteroCloudMark = Activity;
