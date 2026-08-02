import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "@/lib/api-client";
import type {
  RealtimeAccessContext,
  RealtimeDeveloperCredential,
  RealtimeDeveloperCredentialSecret,
} from "@/lib/api-types";
import { DeveloperCredentialsSection } from "./developer-credentials-section";

const organizationId = "0198a117-0d8c-70e2-a457-a83c253b9f21";
const serviceId = "0198a121-ffbd-70c2-a3c8-c65516d7b8fb";

const developerCredential: RealtimeDeveloperCredential = {
  id: "0198a122-1ad4-7fc0-aa10-eaa98c3d9786",
  name: "production-backend",
  prefix: "hcf_0123456789abcdef",
  permissions: ["flow.room.join", "flow.signal.connect"],
  expires_at: "2099-08-01T09:00:00Z",
  last_used_at: null,
  revoked_at: null,
  created_at: "2026-08-01T09:00:00Z",
};

const revokedDeveloperCredential: RealtimeDeveloperCredential = {
  ...developerCredential,
  id: "0198a122-f161-7464-8587-ee1d06b3f833",
  name: "retired-backend",
  prefix: "hcf_fedcba9876543210",
  revoked_at: "2026-08-02T09:00:00Z",
};

const secret: RealtimeDeveloperCredentialSecret = {
  ...developerCredential,
  credential: `hcf_0123456789abcdef_${"A".repeat(43)}`,
  mint_endpoint:
    "https://heterocloud.example.com/api/v1/flow/v1/access-credentials",
};

const activeContext: RealtimeAccessContext = {
  context_id: "0198a123-85d1-7d71-b6fd-a04b255c50ef",
  credential_id: developerCredential.id,
  principal_id: "0198a124-328e-7aad-b374-4237a4de904a",
  permissions: ["flow.room.join"],
  issued_at: "2026-08-02T09:00:00Z",
  expires_at: "2099-08-02T09:05:00Z",
  revoked_at: null,
};

const revokedContext: RealtimeAccessContext = {
  ...activeContext,
  context_id: "0198a123-e18d-794a-a3ad-690416423c54",
  principal_id: "0198a124-f424-7ac0-864f-bfe49451e322",
  revoked_at: "2026-08-02T09:01:00Z",
};

function renderSection(children: ReactNode = null, disabled = false) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <DeveloperCredentialsSection
        organizationId={organizationId}
        serviceId={serviceId}
        disabled={disabled}
      />
      {children}
    </QueryClientProvider>,
  );
}

function closeSecretDialog() {
  const closeButton = screen
    .getAllByRole("button", { name: "閉じる" })
    .find((button) => button.textContent === "閉じる");
  if (!closeButton) throw new Error("secret dialog close button was not found");
  return closeButton;
}

describe("DeveloperCredentialsSection", () => {
  beforeEach(() => {
    vi.spyOn(
      api.realtime.services,
      "listDeveloperCredentials",
    ).mockResolvedValue({ items: [] });
    vi.spyOn(api.realtime.services, "listAccessContexts").mockResolvedValue({
      items: [],
    });
  });

  it("開発者認証情報を作成し、秘密値を一度だけ表示する", async () => {
    const user = userEvent.setup();
    const create = vi
      .spyOn(api.realtime.services, "createDeveloperCredential")
      .mockResolvedValue(secret);
    renderSection();

    await user.click(
      await screen.findByRole("button", { name: "開発者認証情報を作成" }),
    );
    await user.type(screen.getByLabelText("名前"), "production-backend");
    await user.clear(screen.getByLabelText("有効期間（日）"));
    await user.type(screen.getByLabelText("有効期間（日）"), "120");
    await user.click(screen.getByRole("button", { name: "作成" }));

    expect(await screen.findByText(secret.credential)).toBeInTheDocument();
    expect(create).toHaveBeenCalledWith(organizationId, serviceId, {
      name: "production-backend",
      expires_in_days: 120,
      permissions: expect.arrayContaining([
        "flow.room.join",
        "flow.signal.connect",
      ]),
    });

    await user.click(closeSecretDialog());
    await waitFor(() =>
      expect(screen.queryByText(secret.credential)).not.toBeInTheDocument(),
    );
    await user.click(
      screen.getByRole("button", { name: "開発者認証情報を作成" }),
    );
    expect(screen.queryByText(secret.credential)).not.toBeInTheDocument();
  });

  it("作成時の秘密値と短期アクセス発行curlをコピーできる", async () => {
    const user = userEvent.setup();
    const writeText = vi.spyOn(navigator.clipboard, "writeText");
    vi.spyOn(api.realtime.services, "createDeveloperCredential").mockResolvedValue(
      secret,
    );
    renderSection();

    await user.click(
      await screen.findByRole("button", { name: "開発者認証情報を作成" }),
    );
    await user.type(screen.getByLabelText("名前"), "copy-test");
    await user.click(screen.getByRole("button", { name: "作成" }));
    await screen.findByText(secret.credential);

    await user.click(screen.getByRole("button", { name: "秘密値をコピー" }));
    expect(writeText).toHaveBeenLastCalledWith(secret.credential);

    await user.click(screen.getByRole("button", { name: "curl例をコピー" }));
    const curl = String(writeText.mock.calls.at(-1)?.[0]);
    expect(curl).toContain(secret.mint_endpoint);
    expect(curl).toContain(`Authorization: Bearer ${secret.credential}`);
    expect(curl).toContain(
      '"principal_id": "0198a118-073f-79e4-9ca4-0c1c2501c031"',
    );
    expect(curl).toContain('"expires_in_seconds": 300');
    expect(curl).toContain('"flow.room.join"');
    expect(curl).toContain(`--request DELETE '${secret.mint_endpoint}/{context_id}'`);
  });

  it("開発者認証情報を確認付きでローテーション・失効する", async () => {
    const user = userEvent.setup();
    vi.mocked(api.realtime.services.listDeveloperCredentials).mockResolvedValue({
      items: [developerCredential, revokedDeveloperCredential],
    });
    const rotate = vi
      .spyOn(api.realtime.services, "rotateDeveloperCredential")
      .mockResolvedValue({
        ...secret,
        credential: `hcf_0011223344556677_${"B".repeat(43)}`,
      });
    const revoke = vi
      .spyOn(api.realtime.services, "revokeDeveloperCredential")
      .mockResolvedValue(undefined);
    renderSection();

    await screen.findByText("production-backend");
    expect(
      screen.getByRole("button", { name: "retired-backendをローテーション" }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "retired-backendを失効" }),
    ).toBeDisabled();

    await user.click(
      screen.getByRole("button", { name: "production-backendをローテーション" }),
    );
    expect(
      screen.getByRole("heading", { name: "認証情報をローテーション" }),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "ローテーションする" }),
    );
    expect(
      await screen.findByText(`hcf_0011223344556677_${"B".repeat(43)}`),
    ).toBeInTheDocument();
    expect(rotate).toHaveBeenCalledWith(
      organizationId,
      serviceId,
      developerCredential.id,
    );
    await user.click(closeSecretDialog());

    await user.click(
      screen.getByRole("button", { name: "production-backendを失効" }),
    );
    expect(
      screen.getByRole("heading", { name: "開発者認証情報を失効" }),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "失効する" }));
    await waitFor(() =>
      expect(revoke).toHaveBeenCalledWith(
        organizationId,
        serviceId,
        developerCredential.id,
      ),
    );
  });

  it("発行済み短期アクセスを確認付きで失効する", async () => {
    const user = userEvent.setup();
    vi.mocked(api.realtime.services.listAccessContexts).mockResolvedValue({
      items: [activeContext, revokedContext],
    });
    const revoke = vi
      .spyOn(api.realtime.services, "revokeAccessContext")
      .mockResolvedValue(undefined);
    renderSection();

    await screen.findByText(activeContext.principal_id);
    expect(
      screen.getByRole("button", {
        name: `${revokedContext.principal_id}の短期アクセスを失効`,
      }),
    ).toBeDisabled();
    await user.click(
      screen.getByRole("button", {
        name: `${activeContext.principal_id}の短期アクセスを失効`,
      }),
    );
    expect(
      screen.getByRole("heading", { name: "短期アクセスを失効" }),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "失効する" }));

    await waitFor(() =>
      expect(revoke).toHaveBeenCalledWith(
        organizationId,
        serviceId,
        activeContext.context_id,
      ),
    );
  });

  it("発行済み短期アクセスを10件ずつページ表示する", async () => {
    const user = userEvent.setup();
    const accessContexts = Array.from({ length: 12 }, (_, index) => ({
      ...activeContext,
      context_id: `context-${index + 1}`,
      principal_id: `principal-${index + 1}`,
    }));
    vi.mocked(api.realtime.services.listAccessContexts).mockResolvedValue({
      items: accessContexts,
    });
    renderSection();

    expect(await screen.findByText("principal-1")).toBeInTheDocument();
    expect(screen.getByText("principal-10")).toBeInTheDocument();
    expect(screen.queryByText("principal-11")).not.toBeInTheDocument();
    expect(screen.getByText("1–10 / 12")).toBeInTheDocument();
    expect(screen.getByText("1 / 2")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "前のページ" })).toBeDisabled();

    await user.click(screen.getByRole("button", { name: "次のページ" }));

    expect(screen.queryByText("principal-1")).not.toBeInTheDocument();
    expect(screen.getByText("principal-11")).toBeInTheDocument();
    expect(screen.getByText("principal-12")).toBeInTheDocument();
    expect(screen.getByText("11–12 / 12")).toBeInTheDocument();
    expect(screen.getByText("2 / 2")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "次のページ" })).toBeDisabled();
  });

  it("サービスが準備未完了でも既存認証情報を失効できる", async () => {
    vi.mocked(api.realtime.services.listDeveloperCredentials).mockResolvedValue({
      items: [developerCredential],
    });
    vi.mocked(api.realtime.services.listAccessContexts).mockResolvedValue({
      items: [activeContext],
    });
    renderSection(null, true);

    await screen.findByText("production-backend");
    expect(
      screen.getByRole("button", { name: "開発者認証情報を作成" }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "production-backendをローテーション" }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "production-backendを失効" }),
    ).toBeEnabled();
    expect(
      screen.getByRole("button", {
        name: `${activeContext.principal_id}の短期アクセスを失効`,
      }),
    ).toBeEnabled();
  });
});
