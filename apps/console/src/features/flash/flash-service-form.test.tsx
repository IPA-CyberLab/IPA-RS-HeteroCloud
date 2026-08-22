import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import type { RegistryImage } from "@/lib/api-types";
import {
  defaultFlashServiceFormValue,
  FlashServiceForm,
  flashFormValidationError,
  flashRegistryImageOptions,
  flashSpecFromForm,
  parseFlashEnvironment,
  type FlashServiceFormValue,
} from "./flash-service-form";

vi.mock("@/components/shared/resource-selectors", () => ({
  ProjectSelector: () => <div>Project selector</div>,
}));

function FormHarness() {
  const [value, setValue] = useState<FlashServiceFormValue>({
    ...defaultFlashServiceFormValue,
    projectId: "project-1",
    name: "game-server",
    image: "ghcr.io/example/game-server:v1",
  });
  return (
    <FlashServiceForm
      value={value}
      onChange={setValue}
      onSubmit={(event) => event.preventDefault()}
    >
      <button type="submit">保存</button>
    </FlashServiceForm>
  );
}

const registryImage: RegistryImage = {
  reference: "registry.example.com/hc-tenant/game/server:v2",
  repository: "game/server",
  tag: "v2",
  digest: "sha256:0123456789abcdef",
  size_bytes: 128 * 1024 * 1024,
  pushed_at: "2026-08-22T12:00:00Z",
};

function RegistryImageFormHarness() {
  const [value, setValue] = useState<FlashServiceFormValue>({
    ...defaultFlashServiceFormValue,
    projectId: "project-1",
    name: "game-server",
  });
  return (
    <>
      <FlashServiceForm
        value={value}
        onChange={setValue}
        onSubmit={(event) => event.preventDefault()}
        registryImages={[registryImage]}
      >
        <button type="submit">保存</button>
      </FlashServiceForm>
      <output data-testid="selected-image">{value.image}</output>
    </>
  );
}

describe("FlashServiceForm", () => {
  it("自動公開ポートとリソース上限を適用する", () => {
    render(<FormHarness />);

    expect(screen.queryByText(/gVisor/)).not.toBeInTheDocument();
    expect(screen.queryByText("実行ランタイム")).not.toBeInTheDocument();
    expect(screen.getByRole("spinbutton", { name: "レプリカ" })).toHaveValue(1);
    expect(screen.getByRole("spinbutton", { name: "CPU" })).toHaveValue(500);
    expect(screen.getByRole("spinbutton", { name: "メモリ" })).toHaveValue(512);
    expect(screen.getByRole("button", { name: /udpのプロトコル/ })).toHaveTextContent("UDP");
    expect(screen.queryByRole("spinbutton", { name: "サービスポート" })).not.toBeInTheDocument();
    expect(screen.getByRole("spinbutton", { name: "CPU" })).toHaveAttribute("max", "4000");
    expect(screen.getByRole("spinbutton", { name: "メモリ" })).toHaveAttribute("max", "8128");

    fireEvent.change(screen.getByRole("spinbutton", { name: "レプリカ" }), {
      target: { value: "4" },
    });
    expect(screen.getByRole("spinbutton", { name: "レプリカ" })).toHaveValue(4);
  });

  it("Flash Registryと直接入力からコンテナイメージを指定できる", () => {
    render(<RegistryImageFormHarness />);

    expect(flashRegistryImageOptions([registryImage])[0]).toMatchObject({
      value: registryImage.reference,
      label: "game/server:v2",
    });
    expect(
      screen.getByRole("button", { name: /Flash Registryイメージ/ }),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByText("直接入力"));
    const manualInput = screen.getByPlaceholderText(
      "ghcr.io/example/game-server:v1",
    );
    fireEvent.change(manualInput, {
      target: { value: "ghcr.io/example/manual:v1" },
    });
    expect(screen.getByTestId("selected-image")).toHaveTextContent(
      "ghcr.io/example/manual:v1",
    );
  });

  it("環境変数、command、argsをAPI specへ変換する", () => {
    const value: FlashServiceFormValue = {
      ...defaultFlashServiceFormValue,
      projectId: "project-1",
      name: "game-server",
      image: "ghcr.io/example/game-server:v1",
      environment: "GAME_MODE=production\nEMPTY=\nTOKEN=a=b",
      command: "/app/server\n--foreground",
      args: "--listen\n0.0.0.0:7777",
    };

    expect(flashFormValidationError(value)).toBeNull();
    expect(flashSpecFromForm(value)).toMatchObject({
      env: { GAME_MODE: "production", EMPTY: "", TOKEN: "a=b" },
      command: ["/app/server", "--foreground"],
      args: ["--listen", "0.0.0.0:7777"],
      exposure: { type: "public", traffic_mode: "forwarded" },
      ports: [
        {
          name: "udp",
          protocol: "udp",
          container_port: 7777,
        },
      ],
    });
  });

  it("不正または重複した環境変数とポート名を拒否する", () => {
    expect(parseFlashEnvironment("INVALID LINE").error).toContain("KEY=value");
    expect(parseFlashEnvironment("PORT=1\nPORT=2").error).toContain("重複");

    const value: FlashServiceFormValue = {
      ...defaultFlashServiceFormValue,
      projectId: "project-1",
      name: "game-server",
      image: "ghcr.io/example/game-server:v1",
      ports: [
        ...defaultFlashServiceFormValue.ports,
        { ...defaultFlashServiceFormValue.ports[0] },
      ],
    };
    expect(flashFormValidationError(value)).toContain("ポート名 udp");
  });

  it("内部公開を転送モードへ固定する", () => {
    const value: FlashServiceFormValue = {
      ...defaultFlashServiceFormValue,
      projectId: "project-1",
      name: "internal-service",
      image: "ghcr.io/example/internal:v1",
      exposureType: "internal",
      trafficMode: "direct",
    };

    expect(flashFormValidationError(value)).toContain("転送モード");
    expect(flashSpecFromForm(value).exposure).toEqual({
      type: "internal",
      traffic_mode: "forwarded",
    });
  });
});
