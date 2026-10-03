import { render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { App } from "@/app/app";
import { ApiError, api } from "@/lib/api-client";

function anonymousSession() {
  return vi.spyOn(api.auth, "session").mockRejectedValue(
    new ApiError("Authentication is required.", { status: 401, code: "unauthorized" }),
  );
}

describe("public site and console entry points", () => {
  it("does not require a session when the console document is restored at the public root", async () => {
    window.history.replaceState(null, "", "/?site=public");
    const session = anonymousSession();
    render(<App />);

    expect(await screen.findByRole("link", { name: "HeteroCloudの紹介へ" })).toHaveAttribute("href", "/?site=public");
    expect(session).not.toHaveBeenCalled();
    expect(window.location.pathname).toBe("/");
  });

  it("still requires a session at the console entry point", async () => {
    window.history.replaceState(null, "", "/console");
    anonymousSession();
    render(<App />);

    await waitFor(() => expect(window.location.pathname).toBe("/login"));
    expect(await screen.findByRole("heading", { name: "ログイン" })).toBeInTheDocument();
  });
});
