import AppLayout from "@cloudscape-design/components/app-layout";
import BreadcrumbGroup from "@cloudscape-design/components/breadcrumb-group";
import Flashbar, { type FlashbarProps } from "@cloudscape-design/components/flashbar";
import SideNavigation from "@cloudscape-design/components/side-navigation";
import TopNavigation, {
  type TopNavigationProps,
} from "@cloudscape-design/components/top-navigation";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { Outlet, useLocation, useNavigate } from "react-router-dom";
import { navigationItems, routeTitles } from "@/components/layout/navigation";
import { useSession } from "@/features/auth/session";
import { useActiveOrganization } from "@/features/organizations/organization-context";
import { api, getApiErrorMessage } from "@/lib/api-client";

const layoutLabels = {
  navigation: "ナビゲーション",
  navigationToggle: "ナビゲーションを開く",
  navigationClose: "ナビゲーションを閉じる",
  notifications: "通知",
  tools: "ヘルプ",
  toolsToggle: "ヘルプを開く",
  toolsClose: "ヘルプを閉じる",
};

function routeTitle(pathname: string) {
  if (pathname.startsWith("/flow/services/")) return "Flowサービス詳細";
  if (pathname.startsWith("/flash/services/")) return "Flashサービス詳細";
  if (pathname.startsWith("/syouyu/buckets/")) return "Syouyuバケット詳細";
  return routeTitles[pathname] ?? "HeteroCloud";
}

function breadcrumbs(pathname: string, ownerConsole: boolean) {
  if (ownerConsole) {
    return [{ text: "HeteroCloud Owner", href: "/overview" }, { text: "全アカウント管理", href: pathname }];
  }
  const items = [{ text: "HeteroCloud", href: "/overview" }];
  if (pathname.startsWith("/iam/")) {
    items.push({ text: "IAM", href: "/iam/principals" });
  } else if (pathname.startsWith("/flow/")) {
    items.push({ text: "Flow", href: "/flow/services" });
  } else if (pathname.startsWith("/flash/")) {
    items.push({ text: "Flash", href: "/flash/services" });
  } else if (pathname.startsWith("/registry")) {
    items.push({ text: "Flash Registry", href: "/registry" });
  } else if (pathname.startsWith("/syouyu/")) {
    items.push({ text: "Syouyu", href: "/syouyu/buckets" });
  } else if (pathname.startsWith("/owner/")) {
    items.push({ text: "所有者管理", href: "/owner/quotas" });
  }
  items.push({ text: routeTitle(pathname), href: pathname });
  return items;
}

export function AppShell() {
  const [navigationOpen, setNavigationOpen] = useState(true);
  const [logoutError, setLogoutError] = useState<string | null>(null);
  const location = useLocation();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const session = useSession().data!;
  const { activeOrganization, memberships, setActiveOrganizationId } =
    useActiveOrganization();

  const logout = useMutation({
    mutationFn: api.auth.logout,
    onSuccess: () => {
      queryClient.clear();
      navigate("/login", { replace: true });
    },
    onError: (error) => setLogoutError(getApiErrorMessage(error)),
  });

  const notifications = useMemo<FlashbarProps.MessageDefinition[]>(
    () =>
      logoutError
        ? [
            {
              type: "error",
              content: logoutError,
              dismissible: true,
              onDismiss: () => setLogoutError(null),
              id: "logout-error",
            },
          ]
        : [],
    [logoutError],
  );

  const contentType =
    location.pathname === "/overview"
      ? "dashboard"
      : location.pathname === "/settings" || location.pathname.startsWith("/owner/")
        ? "form"
        : "table";

  const accountUtility: TopNavigationProps.Utility = {
    type: "menu-dropdown",
    text: session.user.display_name,
    description: session.user.email,
    iconName: "user-profile",
    ariaLabel: "アカウントメニュー",
    items: session.owner_console
      ? [{ id: "logout", text: "ログアウト", iconName: "exit-full-screen" }]
      : [
          { id: "settings", text: "アカウント設定", iconName: "settings" },
          { id: "logout", text: "ログアウト", iconName: "exit-full-screen" },
        ],
    onItemClick: ({ detail }) => {
      if (detail.id === "settings") navigate("/settings");
      if (detail.id === "logout" && !logout.isPending) logout.mutate();
    },
  };
  const organizationUtility: TopNavigationProps.Utility = {
    type: "menu-dropdown",
    text: activeOrganization.organization_name,
    iconName: "folder",
    ariaLabel: "操作対象の組織",
    items: memberships.map((membership) => ({
      id: `organization:${membership.organization_id}`,
      text: membership.organization_name,
      iconName:
        membership.organization_id === activeOrganization.organization_id
          ? "status-positive"
          : undefined,
    })),
    onItemClick: ({ detail }) => {
      if (detail.id.startsWith("organization:")) {
        setActiveOrganizationId(detail.id.slice("organization:".length));
      }
    },
  };

  return (
    <div className="cloudscape-console">
      <header id="hcloud-header" className="cloudscape-console__header">
        <TopNavigation
          identity={{
            href: "/overview",
            title: session.owner_console ? "HeteroCloud Owner" : "HeteroCloud",
            onFollow: (event) => {
              event.preventDefault();
              navigate("/overview");
            },
          }}
          utilities={session.owner_console ? [accountUtility] : [organizationUtility, accountUtility]}
          i18nStrings={{
            overflowMenuTriggerText: "その他",
            overflowMenuTitleText: "メニュー",
          }}
        />
      </header>

      <AppLayout
        headerSelector="#hcloud-header"
        navigationOpen={navigationOpen}
        onNavigationChange={({ detail }) => setNavigationOpen(detail.open)}
        navigation={
          <SideNavigation
            header={{
              href: "/overview",
              text: session.owner_console ? "サービス運営" : "管理コンソール",
            }}
            activeHref={location.pathname}
            items={navigationItems(session.owner_console)}
            onFollow={(event) => {
              if (!event.detail.external) {
                event.preventDefault();
                navigate(event.detail.href);
              }
            }}
          />
        }
        breadcrumbs={
          <BreadcrumbGroup
            items={breadcrumbs(location.pathname, session.owner_console)}
            onFollow={(event) => {
              event.preventDefault();
              navigate(event.detail.href);
            }}
          />
        }
        notifications={
          notifications.length > 0 ? <Flashbar items={notifications} /> : null
        }
        content={<Outlet />}
        contentType={contentType}
        toolsHide
        ariaLabels={layoutLabels}
      />
    </div>
  );
}
