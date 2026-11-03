import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { SettingsPage } from "./settings-page";

const { setActiveOrganizationId } = vi.hoisted(() => ({
  setActiveOrganizationId: vi.fn(),
}));

vi.mock("@/features/auth/session", () => ({
  useSession: () => ({
    data: {
      user: {
        id: "user-1",
        display_name: "Example User",
        email: "user@example.com",
        status: "active",
        created_at: "2026-08-22T05:00:00Z",
      },
    },
  }),
}));

vi.mock("@/features/organizations/organization-context", () => {
  const memberships = [
    { organization_id: "org-1", organization_slug: "first", organization_name: "First", principal_id: "principal-1", role: "owner" },
    { organization_id: "org-2", organization_slug: "second", organization_name: "Second", principal_id: "principal-2", role: "member" },
  ];
  return {
    useActiveOrganization: () => ({
      activeOrganization: memberships[0],
      memberships,
      setActiveOrganizationId,
    }),
  };
});

describe("SettingsPage", () => {
  it("shows account details and switches organizations without implementation-status cards", async () => {
    const user = userEvent.setup();
    render(<SettingsPage />);

    expect(screen.getByText("Example User")).toBeInTheDocument();
    expect(screen.getByText("user@example.com")).toBeInTheDocument();
    expect(screen.queryByText("セッション保護")).not.toBeInTheDocument();
    expect(screen.queryByText(/HttpOnly Cookie|Origin \+ CSRF/)).not.toBeInTheDocument();
    expect(screen.queryByText("有効")).not.toBeInTheDocument();

    await user.selectOptions(screen.getByLabelText("現在の組織"), "org-2");
    expect(setActiveOrganizationId).toHaveBeenCalledWith("org-2");
  });
});
