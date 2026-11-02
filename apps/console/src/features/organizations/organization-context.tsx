import {
  createContext,
  type ReactNode,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";
import { Outlet, useLocation } from "react-router-dom";
import { useQueryClient } from "@tanstack/react-query";
import { Alert, Button } from "@/components/ui";
import { ErrorState } from "@/components/shared/error-state";
import { PageLoading } from "@/components/shared/page-loading";
import { clearAuthReturnPath } from "@/features/auth/auth-return";
import { useSession } from "@/features/auth/session";
import { api, getApiErrorMessage } from "@/lib/api-client";
import type { Membership, Session } from "@/lib/api-types";

const STORAGE_KEY = "heterocloud.active-organization";
const ROLE_SIGNAL_KEY = "heterocloud.role-transition";

interface OrganizationContextValue {
  activeOrganization: Membership;
  memberships: Membership[];
  setActiveOrganizationId: (organizationId: string) => void;
  activeRole?: Session["active_role"];
  switchRole: (input: { role_arn: string; session_name: string; duration_seconds: number; session_policy?: Record<string, unknown> }) => Promise<void>;
  switchIdentityCenter: (managementAccountId: string, assignmentId: string) => Promise<void>;
  exitRole: () => Promise<void>;
}

const OrganizationContext = createContext<OrganizationContextValue | null>(null);

function readStoredOrganizationId(): string | null {
  try {
    return window.localStorage.getItem(STORAGE_KEY);
  } catch {
    return null;
  }
}

export function OrganizationProvider({ children }: { children?: ReactNode }) {
  const location = useLocation();
  const session = useSession().data;
  const client = useQueryClient();
  const activeRole = session?.active_role;
  const roleMembership = useMemo<Membership | null>(() => activeRole ? {
    organization_id: activeRole.organization_id, organization_slug: activeRole.account_number,
    organization_name: `${activeRole.account_name} / ${activeRole.role_arn.split("/").at(-1) ?? "Role"}`,
    principal_id: activeRole.principal_id, role: "member",
  } : null, [activeRole]);
  const memberships = roleMembership ? [roleMembership] : session?.memberships ?? [];
  const [selectedId, setSelectedId] = useState<string | null>(
    readStoredOrganizationId,
  );
  const [roleTransition, setRoleTransition] = useState(false);
  const [roleError, setRoleError] = useState<string | null>(null);
  const activeOrganization =
    roleMembership ?? memberships.find((membership) => membership.organization_id === selectedId) ??
    memberships[0];

  const clearAccountQueries = async () => {
    const filter = { predicate: (query: { queryKey: readonly unknown[] }) => query.queryKey[0] !== "auth" };
    await client.cancelQueries(filter);
    client.removeQueries(filter);
  };
  const transition = async (operation: () => Promise<unknown>) => {
    setRoleTransition(true);
    setRoleError(null);
    try {
      await clearAccountQueries();
      await operation();
      await clearAccountQueries();
      await client.invalidateQueries({ queryKey: ["auth", "session"] });
    } catch (error) {
      await clearAccountQueries();
      await client.invalidateQueries({ queryKey: ["auth", "session"] });
      setRoleError(getApiErrorMessage(error));
      throw error;
    } finally {
      try { window.localStorage.setItem(ROLE_SIGNAL_KEY, `${Date.now()}-${Math.random()}`); } catch { /* Storage can be disabled. */ }
      setRoleTransition(false);
    }
  };
  const switchRole = (input: { role_arn: string; session_name: string; duration_seconds: number; session_policy?: Record<string, unknown> }) =>
    transition(() => api.iam.roles.switchConsole(activeOrganization.organization_id, input));
  const switchIdentityCenter = (managementAccountId: string, assignmentId: string) =>
    transition(() => api.identityCenter.switchConsole(managementAccountId, assignmentId));
  const exitRole = () => transition(() => api.auth.exitRole());

  useEffect(() => {
    if (!activeOrganization || activeRole) return;
    try {
      window.localStorage.setItem(
        STORAGE_KEY,
        activeOrganization.organization_id,
      );
    } catch {
      // Storage can be disabled; selection still remains valid for this tab.
    }
  }, [activeOrganization, activeRole]);

  useEffect(() => {
    if (!activeRole?.active) return;
    const delay = Math.max(0, new Date(activeRole.expires_at).getTime() - Date.now() + 50);
    const timer = window.setTimeout(() => void client.invalidateQueries({ queryKey: ["auth", "session"] }), Math.min(delay, 2_147_483_647));
    return () => window.clearTimeout(timer);
  }, [activeRole, client]);

  useEffect(() => {
    const onStorage = (event: StorageEvent) => {
      if (event.key !== ROLE_SIGNAL_KEY) return;
      setRoleTransition(true);
      void (async () => {
        try { await clearAccountQueries(); await client.invalidateQueries({ queryKey: ["auth", "session"] }); }
        finally { setRoleTransition(false); }
      })();
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, [client]);

  useEffect(() => {
    if (!["/", "/console", "/console/"].includes(location.pathname)) clearAuthReturnPath();
  }, [location.pathname]);

  const value = useMemo<OrganizationContextValue | null>(
    () =>
      activeOrganization
        ? {
            activeOrganization,
            memberships,
            setActiveOrganizationId: setSelectedId,
            activeRole,
            switchRole,
            switchIdentityCenter,
            exitRole,
          }
        : null,
    [activeOrganization, memberships, activeRole, client],
  );

  if (!value) {
    return (
      <div className="auth-page">
        <ErrorState
          title="利用可能なアカウントがありません"
          description="アカウント管理者へ確認してください。"
        />
      </div>
    );
  }

  if (roleTransition) return <PageLoading label="ロールを切り替えています" />;
  if (activeRole && !activeRole.active) {
    return <div className="auth-page"><ErrorState title="ロールセッションが終了しました"
      description="セッションの期限切れ、失効、またはロールの無効化により操作できません。元のアカウントに戻ってください。" />
      {roleError ? <Alert type="error">{roleError}</Alert> : null}
      <Button variant="primary" onClick={() => void exitRole().catch(() => undefined)}>元のアカウントに戻る</Button></div>;
  }

  return (
    <OrganizationContext.Provider value={value}>
      {roleError ? <Alert type="error">{roleError}</Alert> : null}
      {children ?? <Outlet />}
    </OrganizationContext.Provider>
  );
}

export function useActiveOrganization() {
  const context = useContext(OrganizationContext);
  if (!context) {
    throw new Error("useActiveOrganization must be used within OrganizationProvider");
  }
  return context;
}
