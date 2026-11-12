import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { api, ApiError } from "@/lib/api-client";
import { IdentityCenterPage } from "./identity-center-page";

const switchIdentityCenter = vi.fn(async () => undefined);
vi.mock("@/features/organizations/organization-context", () => ({
  useActiveOrganization: () => ({ activeOrganization: { organization_id: "management", organization_name: "Management", role: "owner" }, switchIdentityCenter, exitRole: vi.fn(), activeRole: null }),
}));
const org = { id: "cloud", name: "Cloud", management_account_id: "management", management_account_number: "123456789012", root_id: "root", current_account_id: "management", current_account_number: "123456789012" };
const set = { id: "set-1", name: "Read", document: { version: "2026-07-31", statements: [{ effect: "Allow", actions: ["project:List"], resources: ["*"] }] }, session_duration_seconds: 3600, created_at: "2026-01-01", updated_at: "2026-01-01" };
const assignment = { id: "assignment-1", permission_set_id: "set-1", permission_set_name: "Read", account_id: "account-1", account_number: "123456789013", account_name: "Target", role_principal_id: "role-1", role_arn: "arn:heterocloud:iam::123456789013:role/Read", subject_type: "user" as const, subject_id: "user-1", session_duration_seconds: 3600, created_at: "2026-01-01" };
function show() { return render(<QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}><MemoryRouter><IdentityCenterPage /></MemoryRouter></QueryClientProvider>); }

describe("Identity Center", () => {
  beforeEach(() => {
    vi.restoreAllMocks(); switchIdentityCenter.mockClear();
    vi.spyOn(api.organizations, "describeCloudOrganization").mockResolvedValue(org);
    vi.spyOn(api.identityCenter, "myAssignments").mockResolvedValue({ items: [assignment] });
    vi.spyOn(api.identityCenter, "permissionSets").mockResolvedValue({ items: [set] });
    vi.spyOn(api.identityCenter, "groups").mockResolvedValue({ items: [{ id: "group-1", name: "Developers", users: [], created_at: "2026-01-01" }] });
    vi.spyOn(api.identityCenter, "users").mockResolvedValue({ items: [{ id: "user-1", display_name: "Member", email: "member@example.test" }] });
    vi.spyOn(api.identityCenter, "assignments").mockResolvedValue({ items: [assignment] });
    vi.spyOn(api.organizations, "tree").mockResolvedValue({ organization: org, units: [], accounts: [{ id: "account-1", account_number: "123456789013", name: "Target", slug: "target", parent_id: "root", is_management_account: false, created_at: "2026-01-01" }] });
  });
  it("lets an assigned user switch accounts without exposing administrative controls", async () => {
    vi.spyOn(api.identityCenter, "permissionSets").mockRejectedValue(new ApiError("Forbidden", { status: 403 }));
    show();
    await userEvent.click(await screen.findByRole("button", { name: "切り替え" }));
    expect(switchIdentityCenter).toHaveBeenCalledWith("management", "assignment-1");
    expect(screen.queryByRole("button", { name: "権限セットを作成" })).not.toBeInTheDocument();
  });
  it("creates a read-only permission set using the visual form", async () => {
    const create = vi.spyOn(api.identityCenter, "createPermissionSet").mockResolvedValue(set);
    show();
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: "権限セットを作成" }));
    await user.type(screen.getByLabelText("名前"), "Readers");
    await user.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => expect(create).toHaveBeenCalledWith("management", expect.objectContaining({ name: "Readers", session_duration_seconds: 3600, document: expect.objectContaining({ Version: "2012-10-17" }) })));
  });
  it("edits a simple permission set visually and preserves its actions", async () => {
    const update = vi.spyOn(api.identityCenter, "updatePermissionSet").mockResolvedValue(set);
    show();
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: "編集" }));
    expect(screen.getByLabelText("権限")).toHaveValue("custom");
    await user.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => expect(update).toHaveBeenCalledWith("management", "set-1", expect.objectContaining({ document: { Version: "2012-10-17", Statement: [{ Effect: "Allow", Action: ["project:List"], Resource: ["*"] }] } })));
  });
});
