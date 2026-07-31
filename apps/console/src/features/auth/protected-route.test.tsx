import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { describe, expect, it, vi } from "vitest";
import { ProtectedRoute } from "@/features/auth/protected-route";
import { ApiError, api } from "@/lib/api-client";

describe("ProtectedRoute", () => {
  it("未認証ならログインへ遷移する", async () => {
    vi.spyOn(api.auth, "session").mockRejectedValue(
      new ApiError("unauthorized", { status: 401, code: "UNAUTHORIZED" }),
    );
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });

    render(
      <QueryClientProvider client={queryClient}>
        <MemoryRouter initialEntries={["/overview"]}>
          <Routes>
            <Route element={<ProtectedRoute />}>
              <Route path="/overview" element={<div>保護された画面</div>} />
            </Route>
            <Route path="/login" element={<div>ログイン画面</div>} />
          </Routes>
        </MemoryRouter>
      </QueryClientProvider>,
    );

    expect(await screen.findByText("ログイン画面")).toBeInTheDocument();
    expect(screen.queryByText("保護された画面")).not.toBeInTheDocument();
  });
});
