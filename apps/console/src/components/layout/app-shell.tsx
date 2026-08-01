import { useMutation, useQueryClient } from "@tanstack/react-query";
import { ChevronDown, LogOut, Menu, UserRound, X } from "lucide-react";
import { useState } from "react";
import { Outlet, useLocation, useNavigate } from "react-router-dom";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Button } from "@/components/ui/button";
import { Sidebar } from "@/components/layout/sidebar";
import { HeteroCloudMark, routeTitles } from "@/components/layout/navigation";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { api, getApiErrorMessage } from "@/lib/api-client";
import { initials } from "@/lib/utils";
import { useSession } from "@/features/auth/session";
import { useActiveOrganization } from "@/features/organizations/organization-context";

export function AppShell() {
  const [mobileOpen, setMobileOpen] = useState(false);
  const [logoutError, setLogoutError] = useState<string | null>(null);
  const location = useLocation();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const sessionQuery = useSession();
  const session = sessionQuery.data!;
  const {
    activeOrganization,
    memberships,
    setActiveOrganizationId,
  } = useActiveOrganization();

  const logout = useMutation({
    mutationFn: api.auth.logout,
    onSuccess: () => {
      queryClient.clear();
      navigate("/login", { replace: true });
    },
    onError: (error) => setLogoutError(getApiErrorMessage(error)),
  });

  const title =
    routeTitles[location.pathname] ??
    (location.pathname.startsWith("/realtime/services/")
      ? "Flow / 詳細"
      : "HeteroCloud");
  return (
    <div className="min-h-screen bg-[#f7f8fa] text-zinc-900">
      <aside className="fixed inset-y-0 left-0 z-30 hidden w-64 lg:block">
        <Sidebar />
      </aside>

      {mobileOpen ? (
        <div className="fixed inset-0 z-50 lg:hidden">
          <button
            type="button"
            className="absolute inset-0 bg-black/50"
            aria-label="ナビゲーションを閉じる"
            onClick={() => setMobileOpen(false)}
          />
          <aside className="relative h-full w-[min(84vw,18rem)] shadow-2xl">
            <Sidebar onNavigate={() => setMobileOpen(false)} />
            <Button
              variant="sidebar"
              size="icon"
              className="absolute right-2 top-3"
              aria-label="ナビゲーションを閉じる"
              title="閉じる"
              onClick={() => setMobileOpen(false)}
            >
              <X />
            </Button>
          </aside>
        </div>
      ) : null}

      <div className="lg:pl-64">
        <header className="sticky top-0 z-20 flex h-16 items-center border-b border-zinc-200 bg-white/95 px-4 backdrop-blur-sm sm:px-6 lg:px-8">
          <Button
            variant="ghost"
            size="icon"
            className="mr-2 lg:hidden"
            aria-label="ナビゲーションを開く"
            title="メニュー"
            onClick={() => setMobileOpen(true)}
          >
            <Menu />
          </Button>

          <div className="mr-3 flex items-center gap-2 lg:hidden">
            <span className="flex size-7 items-center justify-center rounded-[5px] bg-emerald-600 text-white">
              <HeteroCloudMark className="size-4" />
            </span>
          </div>

          <div className="min-w-0 flex-1">
            <p className="truncate text-sm font-medium text-zinc-800">{title}</p>
            <p className="truncate text-xs text-zinc-500 sm:hidden">
              {activeOrganization.organization_name}
            </p>
          </div>

          <div className="mr-2 hidden w-56 sm:block">
            <Select
              value={activeOrganization.organization_id}
              onValueChange={setActiveOrganizationId}
            >
              <SelectTrigger aria-label="操作対象の組織">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {memberships.map((membership) => (
                  <SelectItem
                    key={membership.organization_id}
                    value={membership.organization_id}
                  >
                    {membership.organization_name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                variant="ghost"
                className="h-10 max-w-[14rem] px-2"
                aria-label="アカウントメニュー"
              >
                <span className="flex size-7 shrink-0 items-center justify-center rounded-full bg-zinc-200 text-xs font-semibold text-zinc-700">
                  {initials(session.user.display_name)}
                </span>
                <span className="hidden min-w-0 text-left sm:block">
                  <span className="block truncate text-xs font-medium">
                    {session.user.display_name}
                  </span>
                  <span className="block truncate text-xs font-normal text-zinc-500">
                    {session.user.email}
                  </span>
                </span>
                <ChevronDown className="hidden size-3.5 text-zinc-400 sm:block" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-64">
              <DropdownMenuLabel>
                {session.user.email}
              </DropdownMenuLabel>
              <DropdownMenuSeparator />
              <DropdownMenuItem onSelect={() => navigate("/settings")}>
                <UserRound />
                アカウント設定
              </DropdownMenuItem>
              <DropdownMenuItem
                disabled={logout.isPending}
                onSelect={() => logout.mutate()}
                className="text-red-700 focus:bg-red-50 focus:text-red-800"
              >
                <LogOut />
                {logout.isPending ? "ログアウト中" : "ログアウト"}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </header>

        {logoutError ? (
          <div className="border-b border-red-200 bg-red-50 px-4 py-2 text-center text-sm text-red-800">
            {logoutError}
          </div>
        ) : null}

        <main className="mx-auto w-full max-w-[1600px] p-4 sm:p-6 lg:p-8">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
