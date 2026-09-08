import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import type { FlashQuotaLimits, RegistryImage } from "@/lib/api-types";
import {
  defaultFlashQuotaLimits,
  defaultFlashServiceFormValue,
  FlashServiceForm,
  flashFormValidationError,
  flashFormFromService,
  flashRegistryImageOptions,
  flashSpecFromForm,
  parseFlashDestinationCidrs,
  parseFlashEnvironment,
  parseFlashSourceCidrs,
  type FlashServiceFormValue,
} from "./flash-service-form";

vi.mock("@/components/shared/resource-selectors", () => ({
  ProjectSelector: () => <div>Project selector</div>,
}));

function FormHarness({ quota }: { quota?: FlashQuotaLimits }) {
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
      quota={quota}
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

const untaggedRegistryImage: RegistryImage = {
  ...registryImage,
  reference: "registry.example.com/hc-tenant/game/server@sha256:fedcba9876543210",
  tag: null,
  digest: "sha256:fedcba9876543210",
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
    expect(
      screen.getByRole("button", { name: "エンドポイントを追加" }),
    ).toBeInTheDocument();

    fireEvent.change(screen.getByRole("spinbutton", { name: "レプリカ" }), {
      target: { value: "4" },
    });
    expect(screen.getByRole("spinbutton", { name: "レプリカ" })).toHaveValue(4);
  });

  it("Ownerが設定したアカウント別VM上限を入力へ反映する", () => {
    const quota = {
      ...defaultFlashQuotaLimits,
      max_cpu_millis_per_vm: 8_000,
      max_memory_mib_per_vm: 16_384,
      max_disk_gib_per_vm: 20,
      max_total_cpu_millis: 40_000,
      max_total_memory_mib: 65_536,
      max_total_disk_gib: 200,
    };
    render(<FormHarness quota={quota} />);

    expect(screen.getByRole("spinbutton", { name: "CPU" })).toHaveAttribute(
      "max",
      "8000",
    );
    expect(screen.getByRole("spinbutton", { name: "メモリ" })).toHaveAttribute(
      "max",
      "16384",
    );
    expect(
      screen.getByRole("spinbutton", { name: "ディスク上限" }),
    ).toHaveAttribute("max", "20");
    expect(
      flashFormValidationError(
        {
          ...defaultFlashServiceFormValue,
          projectId: "project-1",
          name: "large-vm",
          image: "ghcr.io/example/large:v1",
          ephemeralStorageGib: 20,
        },
        quota,
      ),
    ).toBeNull();
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

  it("タグなしartifactをFlashの実行候補に含めない", () => {
    expect(
      flashRegistryImageOptions([registryImage, untaggedRegistryImage]),
    ).toHaveLength(1);
  });

  it("環境変数、command、argsをAPI specへ変換する", () => {
    const value: FlashServiceFormValue = {
      ...defaultFlashServiceFormValue,
      projectId: "project-1",
      name: "game-server",
      image: "ghcr.io/example/game-server:v1",
      environment: "GAME_MODE=production\nEMPTY=\nTOKEN=a=b",
      processMode: "custom",
      command: "/app/server\n--foreground",
      args: "--listen\n0.0.0.0:7777",
      allowedSourceCidrs: "203.0.113.10\n2001:db8::/48",
      deniedSourceCidrs: "198.51.100.0/24",
      egressMode: "restricted",
      allowSameOrganization: true,
      allowedDestinationCidrs: "8.8.8.8/32\n2606:4700:4700::1111/128",
      deniedDestinationCidrs: "8.8.8.9/32",
    };

    expect(flashFormValidationError(value)).toBeNull();
    expect(flashSpecFromForm(value)).toMatchObject({
      env: { GAME_MODE: "production", EMPTY: "", TOKEN: "a=b" },
      command: ["/app/server", "--foreground"],
      args: ["--listen", "0.0.0.0:7777"],
      exposure: {
        type: "public",
        traffic_mode: "forwarded",
        allowed_source_cidrs: ["203.0.113.10", "2001:db8::/48"],
        denied_source_cidrs: ["198.51.100.0/24"],
      },
      egress: {
        mode: "restricted",
        allow_same_organization: true,
        allowed_destination_cidrs: ["8.8.8.8/32", "2606:4700:4700::1111/128"],
        denied_destination_cidrs: ["8.8.8.9/32"],
      },
      ports: [
        {
          name: "udp",
          protocol: "udp",
          container_port: 7777,
        },
      ],
    });
  });

  it("編集フォームで最後のエンドポイントまで追加・削除できる", () => {
    render(<FormHarness />);

    const add = screen.getByRole("button", { name: "エンドポイントを追加" });
    fireEvent.click(add);
    expect(screen.getAllByRole("button", { name: /を削除/ })).toHaveLength(2);

    fireEvent.click(screen.getByRole("button", { name: "port-2を削除" }));
    expect(screen.getAllByRole("button", { name: /を削除/ })).toHaveLength(1);

    fireEvent.click(screen.getByRole("button", { name: "udpを削除" }));
    expect(screen.queryByRole("button", { name: /を削除/ })).not.toBeInTheDocument();

    const withoutEndpoints: FlashServiceFormValue = {
      ...defaultFlashServiceFormValue,
      projectId: "project-1",
      name: "private-worker",
      image: "ghcr.io/example/worker:v1",
      ports: [],
    };
    expect(flashFormValidationError(withoutEndpoints)).toBeNull();
    expect(flashSpecFromForm(withoutEndpoints).ports).toEqual([]);
  });

  it("Web Shell待機モードを常駐プロセスへ変換する", () => {
    const spec = flashSpecFromForm({
      ...defaultFlashServiceFormValue,
      projectId: "project-1",
      name: "workspace",
      image: "ubuntu:24.04",
      processMode: "workspace",
    });

    expect(spec.command).toEqual(["/bin/sh", "-c"]);
    expect(spec.args).toEqual([
      "trap 'exit 0' TERM INT; while :; do sleep 3600 & wait $!; done",
    ]);
  });

  it("不正または重複した環境変数とポート名を拒否する", () => {
    expect(parseFlashEnvironment("INVALID LINE").error).toContain("KEY=value");
    expect(parseFlashEnvironment("PORT=1\nPORT=2").error).toContain("重複");
    expect(parseFlashSourceCidrs("203.0.113.1\ninvalid").error).toContain(
      "有効なIPv4 / IPv6",
    );
    expect(parseFlashDestinationCidrs("10.250.0.1/32", true).error).toContain(
      "保護された内部ネットワーク",
    );

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

  it("送信アクセスは公開インターネットを既定にして内部通信を拒否する", () => {
    const spec = flashSpecFromForm({
      ...defaultFlashServiceFormValue,
      projectId: "project-1",
      name: "isolated-worker",
      image: "ubuntu:24.04",
    });

    expect(spec.egress).toEqual({
      mode: "internet",
      allow_same_organization: false,
      allowed_destination_cidrs: [],
      denied_destination_cidrs: [],
    });
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
      endpoint_mode: "ip",
      traffic_mode: "forwarded",
      allowed_source_cidrs: [],
      denied_source_cidrs: [],
    });
  });
});

describe("Flash autoscaling and domain publishing", () => {
  const value: FlashServiceFormValue = {
    ...defaultFlashServiceFormValue,
    projectId: "project-1", name: "autoscaled", image: "example/server:v1",
    scaleMode: "auto", replicas: 2, minReplicas: 2, maxReplicas: 5,
    endpointMode: "load_balancer", memoryTargetEnabled: true,
    environment: "MODE=production", command: "/app/server", args: "--verbose", processMode: "custom",
    allowedSourceCidrs: "203.0.113.0/24", deniedSourceCidrs: "203.0.113.1",
    egressMode: "restricted", allowedDestinationCidrs: "8.8.8.8", allowSameOrganization: true,
  };

  it("round-trips autoscaling, LB, and unrelated fields", () => {
    const spec = flashSpecFromForm(value, { owner: "test" });
    expect(spec.autoscaling).toEqual({ min_replicas: 2, max_replicas: 5, target_cpu_utilization_percent: 80, target_memory_utilization_percent: 80 });
    const form = flashFormFromService({ project_id: value.projectId, name: value.name,
      spec: { ...spec, ports: spec.ports.map((port) => ({ ...port, service_port: 30001 })) } });
    expect(flashSpecFromForm({ ...form, name: "renamed" }, spec.metadata)).toEqual(spec);
    expect(flashFormValidationError(form)).toBeNull();
    expect(flashSpecFromForm({ ...form, scaleMode: "fixed" })).not.toHaveProperty("autoscaling");
    expect(flashSpecFromForm({ ...form, cpuTargetEnabled: false }).autoscaling).toEqual({ min_replicas: 2, max_replicas: 5, target_memory_utilization_percent: 80 });
    const legacy = { ...spec, autoscaling: undefined, exposure: { ...spec.exposure, endpoint_mode: undefined }, ports: [] };
    expect(flashFormFromService({ project_id: "p", name: "n", spec: legacy })).toMatchObject({ scaleMode: "fixed", endpointMode: "ip" });
  });

  it.each([
    { minReplicas: 0 }, { minReplicas: 6 }, { maxReplicas: 101 }, { maxReplicas: 2.5 },
    { replicas: 1 }, { replicas: 6 }, { replicas: NaN },
    { cpuTargetEnabled: false, memoryTargetEnabled: false },
    { cpuTarget: 0 }, { cpuTarget: 101 }, { memoryTarget: 1.5 }, { memoryTarget: NaN },
    { exposureType: "internal" as const }, { trafficMode: "direct" as const },
  ])("rejects invalid contract values %j", (change) => {
    expect(flashFormValidationError({ ...value, ...change })).not.toBeNull();
  });

  it("accepts target boundaries and ignores disabled targets", () => {
    expect(flashFormValidationError({ ...value, cpuTarget: 1, memoryTarget: 100 })).toBeNull();
    expect(flashFormValidationError({ ...value, cpuTargetEnabled: false, cpuTarget: 0 })).toBeNull();
  });

  it("switches scale and publishing modes without losing other input", () => {
    render(<FormHarness />);
    fireEvent.click(screen.getByText("自動", { exact: true }));
    fireEvent.change(screen.getByRole("spinbutton", { name: "最小レプリカ" }), { target: { value: "2" } });
    expect(screen.getByRole("spinbutton", { name: "初期レプリカ" })).toHaveValue(2);
    expect(screen.getByRole("spinbutton", { name: "メモリ目標使用率 (%)" })).toBeDisabled();
    fireEvent.click(screen.getByRole("checkbox", { name: "メモリ目標使用率 (%)" }));
    expect(screen.getByRole("spinbutton", { name: "メモリ目標使用率 (%)" })).toBeEnabled();
    fireEvent.click(screen.getByText("ダイレクト", { exact: true }));
    fireEvent.click(screen.getByText("ドメイン (LB)", { exact: true }));
    expect(screen.getByRole("button", { name: "ダイレクト" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "転送" })).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(screen.getByText("内部", { exact: true }));
    fireEvent.click(screen.getByText("公開", { exact: true }));
    expect(screen.getByRole("button", { name: /^IP$/ })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("textbox", { name: "サービス名" })).toHaveValue("game-server");
  });
});
