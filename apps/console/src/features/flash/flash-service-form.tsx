import Button from "@cloudscape-design/components/button";
import ColumnLayout from "@cloudscape-design/components/column-layout";
import FormField from "@cloudscape-design/components/form-field";
import Header from "@cloudscape-design/components/header";
import Input from "@cloudscape-design/components/input";
import SegmentedControl from "@cloudscape-design/components/segmented-control";
import Select from "@cloudscape-design/components/select";
import SpaceBetween from "@cloudscape-design/components/space-between";
import Textarea from "@cloudscape-design/components/textarea";
import type { FormEvent, ReactNode } from "react";
import { ProjectSelector } from "@/components/shared/resource-selectors";
import type {
  FlashExposure,
  FlashPortInput,
  FlashPortProtocol,
  FlashServiceSpec,
  FlashServiceSpecInput,
} from "@/lib/api-types";

export interface FlashServiceFormValue {
  projectId: string;
  name: string;
  region: string;
  image: string;
  replicas: number;
  cpuMillis: number;
  memoryMib: number;
  ports: FlashPortInput[];
  exposureType: FlashExposure["type"];
  trafficMode: FlashExposure["traffic_mode"];
  environment: string;
  command: string;
  args: string;
}

export const defaultFlashServiceFormValue: FlashServiceFormValue = {
  projectId: "",
  name: "",
  region: "heteronet-global",
  image: "",
  replicas: 1,
  cpuMillis: 500,
  memoryMib: 512,
  ports: [
    {
      name: "udp",
      protocol: "udp",
      container_port: 7777,
    },
  ],
  exposureType: "public",
  trafficMode: "forwarded",
  environment: "",
  command: "",
  args: "",
};

const regions = [
  { value: "heteronet-global", label: "HeteroNet Global" },
  { value: "heteronet-jp", label: "HeteroNet Japan" },
];
const protocols = [
  { value: "udp", label: "UDP" },
  { value: "tcp", label: "TCP" },
];

function boundedInteger(value: string, min: number, max: number, fallback: number) {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) return fallback;
  return Math.min(max, Math.max(min, Math.trunc(parsed)));
}

function lines(value: string): string[] {
  return value
    .split("\n")
    .map((item) => item.trim())
    .filter(Boolean);
}

export function parseFlashEnvironment(value: string): {
  env: Record<string, string>;
  error: string | null;
} {
  const env: Record<string, string> = {};
  for (const [index, line] of value.split("\n").entries()) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    const separator = trimmed.indexOf("=");
    const key = separator >= 0 ? trimmed.slice(0, separator).trim() : "";
    if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(key)) {
      return {
        env: {},
        error: `${index + 1}行目を KEY=value 形式で入力してください。`,
      };
    }
    if (Object.hasOwn(env, key)) {
      return { env: {}, error: `${key} が重複しています。` };
    }
    env[key] = trimmed.slice(separator + 1);
  }
  return { env, error: null };
}

export function flashFormValidationError(
  value: FlashServiceFormValue,
): string | null {
  if (!value.projectId) return "プロジェクトを選択してください。";
  if (!value.name.trim()) return "サービス名を入力してください。";
  if (!value.image.trim() || /\s/.test(value.image)) {
    return "コンテナイメージを入力してください。";
  }
  if (value.exposureType === "internal" && value.trafficMode !== "forwarded") {
    return "内部公開では転送モードを使用してください。";
  }
  if (value.ports.length === 0) return "ポートを1つ以上追加してください。";
  const names = new Set<string>();
  for (const port of value.ports) {
    if (!/^[a-z][a-z0-9-]{0,14}$/.test(port.name)) {
      return "ポート名は英小文字から始まる15文字以内の英数字とハイフンで入力してください。";
    }
    if (names.has(port.name)) return `ポート名 ${port.name} が重複しています。`;
    names.add(port.name);
  }
  return parseFlashEnvironment(value.environment).error;
}

export function flashSpecFromForm(
  value: FlashServiceFormValue,
  metadata: Record<string, unknown> = {},
): FlashServiceSpecInput {
  return {
    region: value.region,
    image: value.image.trim(),
    replicas: value.replicas,
    cpu_millis: value.cpuMillis,
    memory_mib: value.memoryMib,
    ports: value.ports,
    exposure: {
      type: value.exposureType,
      traffic_mode:
        value.exposureType === "internal" ? "forwarded" : value.trafficMode,
    },
    env: parseFlashEnvironment(value.environment).env,
    command: lines(value.command),
    args: lines(value.args),
    metadata,
  };
}

export function flashFormFromService(
  service: {
    project_id: string;
    name: string;
    spec: FlashServiceSpec;
  },
): FlashServiceFormValue {
  return {
    projectId: service.project_id,
    name: service.name,
    region: service.spec.region,
    image: service.spec.image,
    replicas: service.spec.replicas,
    cpuMillis: service.spec.cpu_millis,
    memoryMib: service.spec.memory_mib,
    ports: service.spec.ports.map(({ name, protocol, container_port }) => ({
      name,
      protocol,
      container_port,
    })),
    exposureType: service.spec.exposure.type,
    trafficMode: service.spec.exposure.traffic_mode,
    environment: Object.entries(service.spec.env)
      .map(([key, envValue]) => `${key}=${envValue}`)
      .join("\n"),
    command: service.spec.command.join("\n"),
    args: service.spec.args.join("\n"),
  };
}

export function FlashServiceForm({
  value,
  onChange,
  onSubmit,
  disabled,
  projectLocked,
  children,
}: {
  value: FlashServiceFormValue;
  onChange: (value: FlashServiceFormValue) => void;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
  disabled?: boolean;
  projectLocked?: boolean;
  children: ReactNode;
}) {
  const update = <Key extends keyof FlashServiceFormValue>(
    key: Key,
    nextValue: FlashServiceFormValue[Key],
  ) => onChange({ ...value, [key]: nextValue });
  const updatePort = <Key extends keyof FlashPortInput>(
    index: number,
    key: Key,
    nextValue: FlashPortInput[Key],
  ) => {
    const ports = value.ports.map((port, portIndex) =>
      portIndex === index ? { ...port, [key]: nextValue } : port,
    );
    update("ports", ports);
  };
  const environmentError = parseFlashEnvironment(value.environment).error;

  return (
    <form onSubmit={onSubmit}>
      <SpaceBetween size="l">
        <ColumnLayout columns={2}>
          <FormField label="プロジェクト">
            <ProjectSelector
              value={value.projectId}
              onValueChange={(projectId) => update("projectId", projectId)}
              disabled={disabled || projectLocked}
            />
          </FormField>
          <FormField label="サービス名">
            <Input
              value={value.name}
              disabled={disabled}
              placeholder="game-server"
              onChange={({ detail }) => update("name", detail.value.slice(0, 120))}
            />
          </FormField>
        </ColumnLayout>
        <FormField label="コンテナイメージ">
          <Input
            value={value.image}
            disabled={disabled}
            placeholder="ghcr.io/example/game-server:v1"
            onChange={({ detail }) => update("image", detail.value.slice(0, 500))}
          />
        </FormField>
        <ColumnLayout columns={4}>
          <FormField label="リージョン">
            <Select
              ariaLabel="リージョン"
              selectedOption={regions.find((region) => region.value === value.region) ?? regions[0]}
              options={regions}
              disabled={disabled}
              onChange={({ detail }) => update("region", detail.selectedOption.value ?? regions[0].value)}
            />
          </FormField>
          <FormField label="レプリカ" constraintText="1〜100">
            <Input
              type="number"
              inputMode="numeric"
              step={1}
              nativeInputAttributes={{ min: 1, max: 100 }}
              value={String(value.replicas)}
              disabled={disabled}
              onChange={({ detail }) => update("replicas", boundedInteger(detail.value, 1, 100, value.replicas))}
            />
          </FormField>
          <FormField label="CPU" constraintText="10〜4,000 millicores">
            <Input
              type="number"
              inputMode="numeric"
              step={100}
              nativeInputAttributes={{ min: 10, max: 4_000 }}
              value={String(value.cpuMillis)}
              disabled={disabled}
              onChange={({ detail }) => update("cpuMillis", boundedInteger(detail.value, 10, 4_000, value.cpuMillis))}
            />
          </FormField>
          <FormField label="メモリ" constraintText="16〜8,128 MiB">
            <Input
              type="number"
              inputMode="numeric"
              step={64}
              nativeInputAttributes={{ min: 16, max: 8_128 }}
              value={String(value.memoryMib)}
              disabled={disabled}
              onChange={({ detail }) => update("memoryMib", boundedInteger(detail.value, 16, 8_128, value.memoryMib))}
            />
          </FormField>
        </ColumnLayout>
        <ColumnLayout columns={2}>
          <FormField label="公開範囲">
            <SegmentedControl
              selectedId={value.exposureType}
              options={[
                { id: "internal", text: "内部", disabled },
                { id: "public", text: "公開", disabled },
              ]}
              label="公開範囲"
              onChange={({ detail }) => {
                if (!disabled) {
                  const exposureType = detail.selectedId as FlashExposure["type"];
                  onChange({
                    ...value,
                    exposureType,
                    trafficMode:
                      exposureType === "internal" ? "forwarded" : value.trafficMode,
                  });
                }
              }}
            />
          </FormField>
          <FormField label="通信モード">
            <SegmentedControl
              selectedId={value.trafficMode}
              options={[
                { id: "forwarded", text: "転送", disabled },
                {
                  id: "direct",
                  text: "ダイレクト",
                  disabled: disabled || value.exposureType === "internal",
                  disabledReason:
                    value.exposureType === "internal"
                      ? "内部サービスは転送モードで動作します。"
                      : undefined,
                },
              ]}
              label="通信モード"
              onChange={({ detail }) => {
                if (!disabled) update("trafficMode", detail.selectedId as FlashExposure["traffic_mode"]);
              }}
            />
          </FormField>
        </ColumnLayout>
        <SpaceBetween size="m">
          <Header
            variant="h3"
            actions={
              <Button
                iconName="add-plus"
                formAction="none"
                disabled={disabled || value.ports.length >= 16}
                onClick={() =>
                  update("ports", [
                    ...value.ports,
                    {
                      name: `port-${value.ports.length + 1}`,
                      protocol: "udp",
                      container_port: 7777,
                    },
                  ])
                }
              >
                ポートを追加
              </Button>
            }
          >
            ポート
          </Header>
          {value.ports.map((port, index) => (
            <ColumnLayout columns={4} key={index}>
              <FormField label="名前">
                <Input
                  value={port.name}
                  disabled={disabled}
                  onChange={({ detail }) => updatePort(index, "name", detail.value.toLowerCase().slice(0, 15))}
                />
              </FormField>
              <FormField label="プロトコル">
                <Select
                  ariaLabel={`${port.name || index + 1}のプロトコル`}
                  selectedOption={protocols.find((protocol) => protocol.value === port.protocol) ?? protocols[0]}
                  options={protocols}
                  disabled={disabled}
                  onChange={({ detail }) => updatePort(index, "protocol", detail.selectedOption.value as FlashPortProtocol)}
                />
              </FormField>
              <FormField label="コンテナポート">
                <Input
                  type="number"
                  inputMode="numeric"
                  nativeInputAttributes={{ min: 1, max: 65_535 }}
                  value={String(port.container_port)}
                  disabled={disabled}
                  onChange={({ detail }) => updatePort(index, "container_port", boundedInteger(detail.value, 1, 65_535, port.container_port))}
                />
              </FormField>
              <FormField label="操作">
                <Button
                  variant="icon"
                  iconName="remove"
                  formAction="none"
                  ariaLabel={`${port.name || index + 1}を削除`}
                  disabled={disabled || value.ports.length === 1}
                  onClick={() => update("ports", value.ports.filter((_, portIndex) => portIndex !== index))}
                />
              </FormField>
            </ColumnLayout>
          ))}
        </SpaceBetween>
        <FormField
          label="環境変数"
          description="1行につき KEY=value"
          errorText={environmentError ?? undefined}
        >
          <Textarea
            value={value.environment}
            disabled={disabled}
            placeholder={"GAME_MODE=production\nLOG_LEVEL=info"}
            rows={4}
            onChange={({ detail }) => update("environment", detail.value)}
          />
        </FormField>
        <ColumnLayout columns={2}>
          <FormField label="Command" description="1行につき1要素">
            <Textarea
              value={value.command}
              disabled={disabled}
              placeholder="/app/server"
              rows={3}
              onChange={({ detail }) => update("command", detail.value)}
            />
          </FormField>
          <FormField label="Args" description="1行につき1要素">
            <Textarea
              value={value.args}
              disabled={disabled}
              placeholder={"--listen\n0.0.0.0:7777"}
              rows={3}
              onChange={({ detail }) => update("args", detail.value)}
            />
          </FormField>
        </ColumnLayout>
        {children}
      </SpaceBetween>
    </form>
  );
}
