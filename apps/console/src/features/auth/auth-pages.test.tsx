import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { describe, expect, it, vi } from "vitest";
import { LoginPage } from "@/features/auth/login-page";
import { RegisterPage } from "@/features/auth/register-page";
import { ApiError, api } from "@/lib/api-client";
import type { Session } from "@/lib/api-types";

const invitedSession: Session = {
  user: {
    id: "0198a3be-63f2-7fc4-b88b-95e087d8bf46",
    email: "member@example.com",
    display_name: "Invited Member",
    status: "active",
    created_at: "2026-07-31T09:00:00Z",
  },
  memberships: [
    {
      organization_id: "0198a3be-b69a-7b37-9ff2-934b8907685a",
      organization_slug: "example",
      organization_name: "Example",
      principal_id: "0198a3bf-011a-762e-92b4-9dd5865fd571",
      role: "member",
    },
  ],
  csrf_token: "csrf-token",
};

function renderRoute(path: string, element: React.ReactNode) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={[path]}>
        <Routes>
          <Route path="/login" element={element} />
          <Route path="/register" element={element} />
          <Route path="/overview" element={<div>概要画面</div>} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

function unauthenticatedSession() {
  return vi.spyOn(api.auth, "session").mockRejectedValue(
    new ApiError("Authentication is required.", {
      status: 401,
      code: "unauthorized",
    }),
  );
}

describe("認証画面", () => {
  it("Keycloakのログインと登録を主導線として表示し、ローカルログインも維持する", async () => {
    unauthenticatedSession();
    renderRoute("/login", <LoginPage />);

    expect(
      await screen.findByRole("heading", { name: "ログイン" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("link", { name: "Keycloakでログイン" }),
    ).toHaveAttribute("href", "/api/v1/auth/oidc/start");
    expect(
      screen.getByRole("link", { name: "アカウントを作成" }),
    ).toHaveAttribute(
      "href",
      "/api/v1/auth/oidc/start?intent=register",
    );
    expect(screen.getByLabelText("メールアドレス")).toBeInTheDocument();
    expect(screen.getByLabelText("パスワード")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "ログイン" })).toBeInTheDocument();
    expect(document.querySelector('a[href="/register"]')).not.toBeInTheDocument();
  });

  it("現在URLの任意クエリをOIDC開始URLへ渡さない", async () => {
    unauthenticatedSession();
    renderRoute(
      "/login?redirect_uri=https://attacker.example&intent=register",
      <LoginPage />,
    );

    expect(
      await screen.findByRole("link", { name: "Keycloakでログイン" }),
    ).toHaveAttribute("href", "/api/v1/auth/oidc/start");
    expect(
      screen.getByRole("link", { name: "アカウントを作成" }),
    ).toHaveAttribute(
      "href",
      "/api/v1/auth/oidc/start?intent=register",
    );
  });

  it("招待トークンがなければ登録フォームを表示しない", () => {
    unauthenticatedSession();
    renderRoute("/register", <RegisterPage />);

    expect(screen.getByText("招待リンクが必要です")).toBeInTheDocument();
    expect(screen.queryByLabelText("表示名")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "登録して参加" })).not.toBeInTheDocument();
  });

  it("サーバーログへ残るquery parameterの招待トークンを受け付けない", () => {
    unauthenticatedSession();
    renderRoute(
      "/register?invitation_code=must-not-enter-request-logs",
      <RegisterPage />,
    );

    expect(screen.getByText("招待リンクが必要です")).toBeInTheDocument();
    expect(screen.queryByLabelText("表示名")).not.toBeInTheDocument();
  });

  it("招待リンクのtokenを変更させず登録APIへ渡す", async () => {
    const user = userEvent.setup();
    unauthenticatedSession();
    const register = vi
      .spyOn(api.auth, "register")
      .mockResolvedValue(invitedSession);
    renderRoute(
      "/register#invitation_code=owner-issued-token",
      <RegisterPage />,
    );

    await user.type(screen.getByLabelText("表示名"), "Invited Member");
    await user.type(screen.getByLabelText("メールアドレス"), "member@example.com");
    await user.type(screen.getByLabelText("パスワード"), "valid-password-123");
    await user.type(
      screen.getByLabelText("パスワード（確認）"),
      "valid-password-123",
    );
    await user.click(screen.getByRole("button", { name: "登録して参加" }));

    expect(register.mock.calls[0][0]).toEqual({
      invitation_code: "owner-issued-token",
      email: "member@example.com",
      display_name: "Invited Member",
      password: "valid-password-123",
    });
    expect(await screen.findByText("概要画面")).toBeInTheDocument();
  });
});
