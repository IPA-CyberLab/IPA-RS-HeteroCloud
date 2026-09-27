import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "@/lib/api-client";
import type { FlashService } from "@/lib/api-types";
import { FlashSecretsPanel } from "./flash-secrets-panel";

const service = {
  id: "00000000-0000-0000-0000-000000000001",
  name: "example",
  spec: { secret_files: {} },
} as FlashService;

describe("FlashSecretsPanel", () => {
  beforeEach(() => {
    vi.spyOn(api.flash.services, "listSecrets").mockResolvedValue({ items: ["database-url"] });
    vi.spyOn(api.flash.services, "putSecret").mockResolvedValue(undefined);
    vi.spyOn(api.flash.services, "update").mockResolvedValue(service);
  });

  it("registers a value without displaying it and attaches only its name", async () => {
    const user = userEvent.setup();
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
    render(
      <QueryClientProvider client={queryClient}>
        <FlashSecretsPanel organizationId="organization-1" service={service} disabled={false} />
      </QueryClientProvider>,
    );

    await screen.findByText("database-url");
    await user.type(screen.getByRole("textbox", { name: "名前" }), "api-key");
    await user.type(screen.getByLabelText("値"), "sensitive-test-value");
    await user.click(screen.getByRole("button", { name: "登録・更新" }));
    await waitFor(() => expect(api.flash.services.putSecret).toHaveBeenCalledWith(
      "organization-1", service.id, "api-key", "sensitive-test-value",
    ));
    await waitFor(() => expect(screen.queryByDisplayValue("sensitive-test-value")).not.toBeInTheDocument());
    expect(screen.queryByText("sensitive-test-value")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "コンテナへ接続" }));
    await waitFor(() => expect(api.flash.services.update).toHaveBeenCalledWith(
      "organization-1", service.id, {
        name: "example",
        spec: { secret_files: { "database-url": "database-url" } },
      },
    ));
  });
});
