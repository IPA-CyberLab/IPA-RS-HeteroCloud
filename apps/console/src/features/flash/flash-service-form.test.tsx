import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import {
  defaultFlashServiceFormValue,
  FlashServiceForm,
  flashFormValidationError,
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

describe("FlashServiceForm", () => {
  it("gVisorを強制表示し、既定のUDPポートとリソースを編集できる", () => {
    render(<FormHarness />);

    expect(screen.getByText("gVisor（強制）")).toBeInTheDocument();
    expect(screen.queryByRole("combobox", { name: /ランタイム/ })).not.toBeInTheDocument();
    expect(screen.getByRole("spinbutton", { name: "レプリカ" })).toHaveValue(1);
    expect(screen.getByRole("spinbutton", { name: "CPU" })).toHaveValue(500);
    expect(screen.getByRole("spinbutton", { name: "メモリ" })).toHaveValue(512);
    expect(screen.getByRole("button", { name: /udpのプロトコル/ })).toHaveTextContent("UDP");
    expect(screen.getAllByRole("spinbutton", { name: "サービスポート" })[0]).toHaveValue(7777);

    fireEvent.change(screen.getByRole("spinbutton", { name: "レプリカ" }), {
      target: { value: "4" },
    });
    expect(screen.getByRole("spinbutton", { name: "レプリカ" })).toHaveValue(4);
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
          service_port: 7777,
        },
      ],
    });
  });

  it("不正または重複した環境変数とポートを拒否する", () => {
    expect(parseFlashEnvironment("INVALID LINE").error).toContain("KEY=value");
    expect(parseFlashEnvironment("PORT=1\nPORT=2").error).toContain("重複");

    const value: FlashServiceFormValue = {
      ...defaultFlashServiceFormValue,
      projectId: "project-1",
      name: "game-server",
      image: "ghcr.io/example/game-server:v1",
      ports: [
        ...defaultFlashServiceFormValue.ports,
        { ...defaultFlashServiceFormValue.ports[0], name: "udp-2" },
      ],
    };
    expect(flashFormValidationError(value)).toContain("UDP 7777");
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
