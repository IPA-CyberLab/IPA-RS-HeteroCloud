import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { api } from "@/lib/api-client";
import { AccessCredentialDialog } from "./access-credential-dialog";

describe("AccessCredentialDialog", () => {
  it("メトリクス参照を含む短期認証情報を発行し、閉じた後は秘密値を破棄する", async () => {
    const user = userEvent.setup();
    const issue = vi
      .spyOn(api.realtime.services, "issueAccessCredential")
      .mockResolvedValue({
        context_id: "0198a122-ffbd-70c2-a3c8-c65516d7b8fb",
        organization_id: "0198a117-0d8c-70e2-a457-a83c253b9f21",
        project_id: "0198a11b-b519-7177-b6fd-bb131b5fb9e6",
        service_instance_id: "0198a121-ffbd-70c2-a3c8-c65516d7b8fb",
        principal_id: "0198a118-073f-79e4-9ca4-0c1c2501c031",
        issued_at: 1_754_038_800,
        expires_at: 1_754_042_400,
        headers: {
          "x-flow-principal": "one-time-principal",
          "x-flow-timestamp": "1754038800",
          "x-flow-signature": "one-time-signature",
        },
        endpoints: ["https://flow.example.com"],
        rate_limit: {
          requests_per_second: 20,
          burst: 40,
        },
      });
    const queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false },
        mutations: { retry: false },
      },
    });

    render(
      <QueryClientProvider client={queryClient}>
        <AccessCredentialDialog
          organizationId="0198a117-0d8c-70e2-a457-a83c253b9f21"
          serviceId="0198a121-ffbd-70c2-a3c8-c65516d7b8fb"
          serviceName="realtime-production"
        />
      </QueryClientProvider>,
    );

    await user.click(
      screen.getByRole("button", { name: "テスト用短期アクセス" }),
    );
    expect(
      screen.getByRole("heading", { name: "短期アクセスを手動発行" }),
    ).toBeInTheDocument();
    expect(screen.getByText("メトリクス参照")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "発行" }));

    expect(await screen.findByText("one-time-signature")).toBeInTheDocument();
    expect(screen.getByText("20 RPS / burst 40")).toBeInTheDocument();
    expect(issue).toHaveBeenCalledWith(
      "0198a117-0d8c-70e2-a457-a83c253b9f21",
      "0198a121-ffbd-70c2-a3c8-c65516d7b8fb",
      expect.objectContaining({
          expires_in_seconds: 300,
        permissions: expect.arrayContaining(["flow.metrics.read"]),
      }),
    );

    const closeButton = screen
      .getAllByRole("button", { name: "閉じる" })
      .find((button) => button.textContent === "閉じる");
    expect(closeButton).toBeDefined();
    await user.click(closeButton!);
    await waitFor(() => {
      expect(screen.queryByText("one-time-signature")).not.toBeInTheDocument();
    });

    await user.click(
      screen.getByRole("button", { name: "テスト用短期アクセス" }),
    );
    expect(screen.queryByText("one-time-signature")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "発行" })).toBeInTheDocument();
  });
});
