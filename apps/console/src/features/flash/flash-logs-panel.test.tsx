import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { api } from "@/lib/api-client";
import { FlashLogsPanel } from "./flash-logs-panel";

afterEach(() => vi.restoreAllMocks());

it("shows a selected replica's logs as text and allows the previous container", async () => {
  const getLogs = vi.spyOn(api.flash.services, "getLogs").mockImplementation(async (_org, _service, pod, previous) => ({
    items: [{ name: "pod-a", phase: "Running", ready: true }, { name: "pod-b", phase: "Failed", ready: false }],
    pod: pod ?? null, previous: previous ?? false, text: pod ? `<script>${pod}</script>` : null, truncated: Boolean(previous),
  }));
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(<QueryClientProvider client={client}><FlashLogsPanel organizationId="org" serviceId="service" /></QueryClientProvider>);
  expect(await screen.findByText("<script>pod-a</script>")).toBeInTheDocument();
  fireEvent.change(screen.getByLabelText("ログを表示するコンテナ"), { target: { value: "pod-b" } });
  expect(await screen.findByText("<script>pod-b</script>")).toBeInTheDocument();
  fireEvent.click(screen.getByLabelText("前回終了したコンテナのログ"));
  expect(await screen.findByText("ログの一部のみを表示しています。")).toBeInTheDocument();
  await waitFor(() => expect(getLogs).toHaveBeenCalledWith("org", "service", "pod-b", true, expect.any(AbortSignal)));
  expect(document.querySelector("script")).toBeNull();
});
