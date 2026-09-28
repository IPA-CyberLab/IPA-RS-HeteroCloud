import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "@/lib/api-client";
import type { FlashService } from "@/lib/api-types";
import { SecretManagerPage } from "./secret-manager-page";

vi.mock("@/features/organizations/organization-context", () => ({
  useActiveOrganization: () => ({
    activeOrganization: { organization_id: "organization-1" },
  }),
}));

const service = {
  id: "00000000-0000-0000-0000-000000000001",
  name: "my-container",
  state: "ready",
  spec: { secret_files: {} },
} as FlashService;

function renderPage() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter>
        <SecretManagerPage />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

describe("SecretManagerPage", () => {
  beforeEach(() => {
    vi.spyOn(api.flash.services, "list").mockResolvedValue({ items: [service] });
    vi.spyOn(api.flash.services, "listSecrets").mockResolvedValue({ items: ["database-url"] });
    vi.spyOn(api.secretManager, "link").mockResolvedValue({
      url: "http://secrets.heteronetwork.internal:21444/ui/vault/auth?with=oidc/",
    });
  });

  it("shows a user's container secrets in the console and keeps OpenBao as an OIDC link", async () => {
    renderPage();

    expect(await screen.findByText("my-container")).toBeInTheDocument();
    expect(await screen.findByText("database-url")).toBeInTheDocument();
    expect(api.flash.services.listSecrets).toHaveBeenCalledWith(
      "organization-1", service.id, expect.anything(),
    );
    expect(screen.getByRole("button", { name: "コンテナへ接続" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "名前" })).toBeInTheDocument();
    expect(screen.getByLabelText("値")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "OpenBaoにOIDCでサインイン" }))
      .toHaveAttribute("href", "http://secrets.heteronetwork.internal:21444/ui/vault/auth?with=oidc/");
  });
});
