import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it, vi } from "vitest";
import { api } from "@/lib/api-client";
import { CliAuthorizePage } from "./cli-authorize-page";

describe("CLI authorization", () => {
  it("組織と確認コードを表示してCLIを承認する", async () => {
    const get = vi.spyOn(api.auth.cliDevice, "get").mockResolvedValue({
      user_code: "ABCD-EFGH-JKLM",
      organization: {
        organization_id: "0199a117-0d8c-70e2-a457-a83c253b9f21",
        organization_slug: "example",
        organization_name: "Example Organization",
        principal_id: "0199a117-0d8c-70e2-a457-a83c253b9f22",
        role: "member",
      },
      expires_at: "2026-09-23T15:00:00Z",
    });
    const approve = vi.spyOn(api.auth.cliDevice, "approve").mockResolvedValue();
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
    const user = userEvent.setup();

    render(
      <QueryClientProvider client={queryClient}>
        <MemoryRouter initialEntries={["/cli/authorize?user_code=ABCD-EFGH-JKLM"]}>
          <CliAuthorizePage />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    expect(await screen.findByText("ABCD-EFGH-JKLM")).toBeInTheDocument();
    expect(screen.getByText("Example Organization (example)")).toBeInTheDocument();
    expect(get).toHaveBeenCalledWith("ABCD-EFGH-JKLM", expect.any(AbortSignal));
    await user.click(screen.getByRole("button", { name: "このCLIを承認" }));
    expect(approve).toHaveBeenCalledWith("ABCD-EFGH-JKLM");
    expect(await screen.findByText("CLIを承認しました")).toBeInTheDocument();
  });
});
