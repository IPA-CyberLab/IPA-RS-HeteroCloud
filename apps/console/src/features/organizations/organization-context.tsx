import {
  createContext,
  type ReactNode,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";
import { Outlet } from "react-router-dom";
import { ErrorState } from "@/components/shared/error-state";
import { useSession } from "@/features/auth/session";
import type { Membership } from "@/lib/api-types";

const STORAGE_KEY = "heterocloud.active-organization";

interface OrganizationContextValue {
  activeOrganization: Membership;
  memberships: Membership[];
  setActiveOrganizationId: (organizationId: string) => void;
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
  const session = useSession().data;
  const memberships = session?.memberships ?? [];
  const [selectedId, setSelectedId] = useState<string | null>(
    readStoredOrganizationId,
  );
  const activeOrganization =
    memberships.find((membership) => membership.organization_id === selectedId) ??
    memberships[0];

  useEffect(() => {
    if (!activeOrganization) return;
    try {
      window.localStorage.setItem(
        STORAGE_KEY,
        activeOrganization.organization_id,
      );
    } catch {
      // Storage can be disabled; selection still remains valid for this tab.
    }
  }, [activeOrganization]);

  const value = useMemo<OrganizationContextValue | null>(
    () =>
      activeOrganization
        ? {
            activeOrganization,
            memberships,
            setActiveOrganizationId: setSelectedId,
          }
        : null,
    [activeOrganization, memberships],
  );

  if (!value) {
    return (
      <div className="auth-page">
        <ErrorState
          title="所属組織がありません"
          description="このアカウントには利用可能な組織メンバーシップがありません。管理者へ確認してください。"
        />
      </div>
    );
  }

  return (
    <OrganizationContext.Provider value={value}>
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
