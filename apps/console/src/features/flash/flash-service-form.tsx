import Button from "@cloudscape-design/components/button";
import ColumnLayout from "@cloudscape-design/components/column-layout";
import FormField from "@cloudscape-design/components/form-field";
import Header from "@cloudscape-design/components/header";
import Input from "@cloudscape-design/components/input";
import SegmentedControl from "@cloudscape-design/components/segmented-control";
import Select from "@cloudscape-design/components/select";
import SpaceBetween from "@cloudscape-design/components/space-between";
import Textarea from "@cloudscape-design/components/textarea";
import Toggle from "@cloudscape-design/components/toggle";
import ipaddr from "ipaddr.js";
import type { FormEvent, ReactNode } from "react";
import "./flash-service-form.css";
import { ProjectSelector } from "@/components/shared/resource-selectors";
import type {
  FlashExposure,
  FlashEgressMode,
  FlashPortInput,
  FlashPortProtocol,
  FlashQuotaLimits,
  FlashServiceSpec,
  FlashServiceSpecInput,
  RegistryImage,
} from "@/lib/api-types";

export type FlashImageSource = "registry" | "manual";
export type FlashProcessMode = "image" | "workspace" | "custom";

export interface FlashServiceFormValue {
  projectId: string;
  name: string;
  region: string;
  imageSource: FlashImageSource;
  image: string;
  replicas: number;
  scaleMode: "fixed" | "auto";
  minReplicas: number;
  maxReplicas: number;
  cpuTargetEnabled: boolean;
  memoryTargetEnabled: boolean;
  cpuTarget: number;
  memoryTarget: number;
  endpointMode: NonNullable<FlashExposure["endpoint_mode"]>;
  cpuMillis: number;
  memoryMib: number;
  ephemeralStorageGib: number;
  ports: FlashPortInput[];
  exposureType: FlashExposure["type"];
  trafficMode: FlashExposure["traffic_mode"];
  allowedSourceCidrs: string;
  deniedSourceCidrs: string;
  egressMode: FlashEgressMode;
  allowSameOrganization: boolean;
  allowedDestinationCidrs: string;
  deniedDestinationCidrs: string;
  environment: string;
  processMode: FlashProcessMode;
  command: string;
  args: string;
}

export const defaultFlashServiceFormValue: FlashServiceFormValue = {
  projectId: "",
  name: "",
  region: "heteronet-global",
  imageSource: "registry",
  image: "",
  replicas: 1,
  scaleMode: "fixed",
  minReplicas: 1,
  maxReplicas: 2,
  cpuTargetEnabled: true,
  memoryTargetEnabled: false,
  cpuTarget: 80,
  memoryTarget: 80,
  endpointMode: "ip",
  cpuMillis: 500,
  memoryMib: 512,
  ephemeralStorageGib: 10,
  ports: [
    {
      name: "udp",
      protocol: "udp",
      container_port: 7777,
    },
  ],
  exposureType: "public",
  trafficMode: "forwarded",
  allowedSourceCidrs: "",
  deniedSourceCidrs: "",
  egressMode: "internet",
  allowSameOrganization: false,
  allowedDestinationCidrs: "",
  deniedDestinationCidrs: "",
  environment: "",
  processMode: "image",
  command: "",
  args: "",
};

export const defaultFlashQuotaLimits: FlashQuotaLimits = {
  max_services: 100,
  max_replicas_per_service: 100,
  max_cpu_millis_per_vm: 4_000,
  max_memory_mib_per_vm: 8_128,
  max_disk_gib_per_vm: 10,
  max_total_replicas: 100,
  max_total_cpu_millis: 20_000,
  max_total_memory_mib: 32_768,
  max_total_disk_gib: 100,
};

const workspaceCommand = ["/bin/sh", "-c"];
const workspaceArgs = [
  "trap 'exit 0' TERM INT; while :; do sleep 3600 & wait $!; done",
];

const regions = [
  { value: "heteronet-global", label: "HeteroNet Global" },
  { value: "heteronet-jp", label: "HeteroNet Japan" },
];
const protocols = [
  { value: "udp", label: "UDP" },
  { value: "tcp", label: "TCP" },
];
const webSourceCidrError = "HTTP/HTTPS公開では受信許可・拒否IP / CIDRは未対応です。設定を削除するか、IP / ドメイン (LB) を選択してください。";
const protectedDestinationCidrs = [
  "::/128",
  "::1/128",
  "0.0.0.0/8",
  "10.0.0.0/8",
  "100.64.0.0/10",
  "127.0.0.0/8",
  "169.254.0.0/16",
  "172.16.0.0/12",
  "192.0.0.0/24",
  "192.88.99.0/24",
  "192.168.0.0/16",
  "198.18.0.0/15",
  "224.0.0.0/3",
  "fc00::/7",
  "fe80::/10",
  "ff00::/8",
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

function equalParts(left: string[], right: string[]): boolean {
  return left.length === right.length && left.every((part, index) => part === right[index]);
}

function formatImageSize(value: number): string {
  const units = ["B", "KiB", "MiB", "GiB"];
  let amount = Math.max(0, value);
  let unit = 0;
  while (amount >= 1024 && unit < units.length - 1) {
    amount /= 1024;
    unit += 1;
  }
  return `${amount.toLocaleString("ja-JP", { maximumFractionDigits: 1 })} ${units[unit]}`;
}

export function flashRegistryImageOptions(registryImages: RegistryImage[]) {
  return registryImages
    .filter((image) => image.tag !== null)
    .map((image) => ({
      value: image.reference,
      label: `${image.repository}:${image.tag}`,
      description: image.digest,
      labelTag: formatImageSize(image.size_bytes),
      filteringTags: [image.reference],
    }));
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

export function parseFlashSourceCidrs(value: string): {
  cidrs: string[];
  error: string | null;
} {
  const cidrs = value
    .split(/[\n,]/)
    .map((item) => item.trim())
    .filter(Boolean);
  if (cidrs.length > 64) {
    return { cidrs: [], error: "IP / CIDRは64件まで設定できます。" };
  }
  const seen = new Set<string>();
  for (const cidr of cidrs) {
    try {
      const [address, prefix] = cidr.includes("/")
        ? ipaddr.parseCIDR(cidr)
        : (() => {
            const parsed = ipaddr.parse(cidr);
            return [parsed, parsed.kind() === "ipv4" ? 32 : 128] as const;
          })();
      const key = `${address.kind()}:${address.toString()}/${prefix}`;
      if (seen.has(key)) {
        return { cidrs: [], error: `${cidr} が重複しています。` };
      }
      seen.add(key);
    } catch {
      return {
        cidrs: [],
        error: `${cidr} は有効なIPv4 / IPv6アドレスまたはCIDRではありません。`,
      };
    }
  }
  return { cidrs, error: null };
}

function parsedNetwork(value: string) {
  const [address, prefix] = value.includes("/")
    ? ipaddr.parseCIDR(value)
    : (() => {
        const address = ipaddr.parse(value);
        return [address, address.kind() === "ipv4" ? 32 : 128] as const;
      })();
  return { kind: address.kind(), bytes: address.toByteArray(), prefix };
}

function networksOverlap(left: string, right: string): boolean {
  const first = parsedNetwork(left);
  const second = parsedNetwork(right);
  if (first.kind !== second.kind) return false;
  let remaining = Math.min(first.prefix, second.prefix);
  for (let index = 0; remaining > 0; index += 1) {
    const bits = Math.min(8, remaining);
    const mask = (0xff << (8 - bits)) & 0xff;
    if ((first.bytes[index] & mask) !== (second.bytes[index] & mask)) return false;
    remaining -= bits;
  }
  return true;
}

export function parseFlashDestinationCidrs(
  value: string,
  rejectProtected = false,
): { cidrs: string[]; error: string | null } {
  const parsed = parseFlashSourceCidrs(value);
  if (parsed.error || !rejectProtected) return parsed;
  const protectedDestination = parsed.cidrs.find((cidr) =>
    protectedDestinationCidrs.some((protectedCidr) => networksOverlap(cidr, protectedCidr)),
  );
  if (protectedDestination) {
    return {
      cidrs: [],
      error: `${protectedDestination} は保護された内部ネットワークと重複しています。`,
    };
  }
  return parsed;
}

export function flashFormValidationError(
  value: FlashServiceFormValue,
  quota: FlashQuotaLimits = defaultFlashQuotaLimits,
): string | null {
  if (!value.projectId) return "プロジェクトを選択してください。";
  if (!value.name.trim()) return "サービス名を入力してください。";
  if (!value.image.trim() || /\s/.test(value.image)) {
    return "コンテナイメージを入力してください。";
  }
  if (!Number.isInteger(value.replicas) || value.replicas < 1 || value.replicas > quota.max_replicas_per_service) {
    return `レプリカは1〜${quota.max_replicas_per_service.toLocaleString("ja-JP")}で入力してください。`;
  }
  if (value.scaleMode === "auto") {
    if (!Number.isInteger(value.minReplicas) || !Number.isInteger(value.maxReplicas) ||
        value.minReplicas < 1 || value.maxReplicas < value.minReplicas ||
        value.maxReplicas > quota.max_replicas_per_service) {
      return `最小・最大レプリカは1〜${quota.max_replicas_per_service}で、最大を最小以上に設定してください。`;
    }
    if (value.replicas < value.minReplicas || value.replicas > value.maxReplicas) {
      return "レプリカは最小・最大レプリカの範囲内に設定してください。";
    }
    if (!value.cpuTargetEnabled && !value.memoryTargetEnabled) return "CPUまたはメモリの目標使用率を選択してください。";
    for (const [enabled, target] of [[value.cpuTargetEnabled, value.cpuTarget], [value.memoryTargetEnabled, value.memoryTarget]] as const) {
      if (enabled && (!Number.isInteger(target) || target < 1 || target > 100)) return "目標使用率は1〜100%で入力してください。";
    }
  }
  if (value.endpointMode !== "ip" && (value.exposureType !== "public" || value.trafficMode !== "forwarded")) {
    return "ドメイン公開では公開・転送モードを使用してください。";
  }
  if (value.endpointMode === "web") {
    if (value.ports.length !== 1 || value.ports[0].protocol !== "tcp") {
      return "HTTP/HTTPS公開にはTCPコンテナポートを1つだけ設定してください。";
    }
    const port = value.ports[0].container_port;
    if (!Number.isInteger(port) || port < 1 || port > 65_535) {
      return "コンテナポートは1〜65535で入力してください。";
    }
  }
  if (value.cpuMillis < 10 || value.cpuMillis > quota.max_cpu_millis_per_vm) {
    return `CPUは10〜${quota.max_cpu_millis_per_vm.toLocaleString("ja-JP")} millicoresで入力してください。`;
  }
  if (value.memoryMib < 16 || value.memoryMib > quota.max_memory_mib_per_vm) {
    return `メモリは16〜${quota.max_memory_mib_per_vm.toLocaleString("ja-JP")} MiBで入力してください。`;
  }
  if (
    value.ephemeralStorageGib < 1 ||
    value.ephemeralStorageGib > quota.max_disk_gib_per_vm
  ) {
    return `ディスク上限は1〜${quota.max_disk_gib_per_vm.toLocaleString("ja-JP")} GiBで入力してください。`;
  }
  if (value.exposureType === "internal" && value.trafficMode !== "forwarded") {
    return "内部公開では転送モードを使用してください。";
  }
  const names = new Set<string>();
  for (const port of value.ports) {
    if (!/^[a-z][a-z0-9-]{0,14}$/.test(port.name)) {
      return "ポート名は英小文字から始まる15文字以内の英数字とハイフンで入力してください。";
    }
    if (names.has(port.name)) return `ポート名 ${port.name} が重複しています。`;
    names.add(port.name);
  }
  const allowedSources = parseFlashSourceCidrs(value.allowedSourceCidrs);
  if (allowedSources.error) return `許可IP: ${allowedSources.error}`;
  const deniedSources = parseFlashSourceCidrs(value.deniedSourceCidrs);
  if (deniedSources.error) return `拒否IP: ${deniedSources.error}`;
  if (value.endpointMode === "web" && (allowedSources.cidrs.length || deniedSources.cidrs.length)) {
    return webSourceCidrError;
  }
  const allowedDestinations = parseFlashDestinationCidrs(
    value.allowedDestinationCidrs,
    true,
  );
  if (allowedDestinations.error) return `送信許可先: ${allowedDestinations.error}`;
  if (value.egressMode !== "restricted" && allowedDestinations.cidrs.length) {
    return "送信許可先CIDRは許可リストモードでのみ設定できます。";
  }
  const deniedDestinations = parseFlashDestinationCidrs(value.deniedDestinationCidrs);
  if (deniedDestinations.error) return `送信拒否先: ${deniedDestinations.error}`;
  return parseFlashEnvironment(value.environment).error;
}

export function flashSpecFromForm(
  value: FlashServiceFormValue,
  metadata: Record<string, unknown> = {},
): FlashServiceSpecInput {
  const [command, args] =
    value.processMode === "workspace"
      ? [workspaceCommand, workspaceArgs]
      : value.processMode === "custom"
        ? [lines(value.command), lines(value.args)]
        : [[], []];
  return {
    region: value.region,
    image: value.image.trim(),
    replicas: value.replicas,
    ...(value.scaleMode === "auto" ? { autoscaling: {
      min_replicas: value.minReplicas,
      max_replicas: value.maxReplicas,
      ...(value.cpuTargetEnabled ? { target_cpu_utilization_percent: value.cpuTarget } : {}),
      ...(value.memoryTargetEnabled ? { target_memory_utilization_percent: value.memoryTarget } : {}),
    } } : {}),
    cpu_millis: value.cpuMillis,
    memory_mib: value.memoryMib,
    ephemeral_storage_gib: value.ephemeralStorageGib,
    ports: value.ports,
    exposure: {
      type: value.exposureType,
      endpoint_mode: value.exposureType === "internal" ? "ip" : value.endpointMode,
      traffic_mode:
        value.exposureType === "internal" || value.endpointMode !== "ip" ? "forwarded" : value.trafficMode,
      allowed_source_cidrs: parseFlashSourceCidrs(value.allowedSourceCidrs).cidrs,
      denied_source_cidrs: parseFlashSourceCidrs(value.deniedSourceCidrs).cidrs,
    },
    egress: {
      mode: value.egressMode,
      allow_same_organization: value.allowSameOrganization,
      allowed_destination_cidrs: parseFlashDestinationCidrs(
        value.allowedDestinationCidrs,
        true,
      ).cidrs,
      denied_destination_cidrs: parseFlashDestinationCidrs(
        value.deniedDestinationCidrs,
      ).cidrs,
    },
    env: parseFlashEnvironment(value.environment).env,
    command,
    args,
    metadata,
  };
}

export function flashFormFromService(
  service: {
    project_id: string;
    name: string;
    spec: FlashServiceSpec;
  },
  registryImages: RegistryImage[] = [],
): FlashServiceFormValue {
  const egress = service.spec.egress ?? {
    mode: "internet" as const,
    allow_same_organization: false,
    allowed_destination_cidrs: [],
    denied_destination_cidrs: [],
  };
  const processMode: FlashProcessMode =
    service.spec.command.length === 0 && service.spec.args.length === 0
      ? "image"
      : equalParts(service.spec.command, workspaceCommand) &&
          equalParts(service.spec.args, workspaceArgs)
        ? "workspace"
        : "custom";
  return {
    projectId: service.project_id,
    name: service.name,
    region: service.spec.region,
    imageSource: registryImages.some(
      (image) => image.reference === service.spec.image,
    )
      ? "registry"
      : "manual",
    image: service.spec.image,
    replicas: service.spec.replicas,
    scaleMode: service.spec.autoscaling ? "auto" : "fixed",
    minReplicas: service.spec.autoscaling?.min_replicas ?? service.spec.replicas,
    maxReplicas: service.spec.autoscaling?.max_replicas ?? service.spec.replicas,
    cpuTargetEnabled: service.spec.autoscaling ? service.spec.autoscaling.target_cpu_utilization_percent !== undefined : true,
    memoryTargetEnabled: service.spec.autoscaling?.target_memory_utilization_percent !== undefined,
    cpuTarget: service.spec.autoscaling?.target_cpu_utilization_percent ?? 80,
    memoryTarget: service.spec.autoscaling?.target_memory_utilization_percent ?? 80,
    endpointMode: service.spec.exposure.endpoint_mode ?? "ip",
    cpuMillis: service.spec.cpu_millis,
    memoryMib: service.spec.memory_mib,
    ephemeralStorageGib: service.spec.ephemeral_storage_gib,
    ports: service.spec.ports.map(({ name, protocol, container_port }) => ({
      name,
      protocol,
      container_port,
    })),
    exposureType: service.spec.exposure.type,
    trafficMode: service.spec.exposure.traffic_mode,
    allowedSourceCidrs: (service.spec.exposure.allowed_source_cidrs ?? []).join("\n"),
    deniedSourceCidrs: (service.spec.exposure.denied_source_cidrs ?? []).join("\n"),
    egressMode: egress.mode,
    allowSameOrganization: egress.allow_same_organization,
    allowedDestinationCidrs: egress.allowed_destination_cidrs.join("\n"),
    deniedDestinationCidrs: egress.denied_destination_cidrs.join("\n"),
    environment: Object.entries(service.spec.env)
      .map(([key, envValue]) => `${key}=${envValue}`)
      .join("\n"),
    processMode,
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
  registryImages = [],
  registryImagesStatus = "finished",
  quota = defaultFlashQuotaLimits,
  children,
}: {
  value: FlashServiceFormValue;
  onChange: (value: FlashServiceFormValue) => void;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
  disabled?: boolean;
  projectLocked?: boolean;
  registryImages?: RegistryImage[];
  registryImagesStatus?: "loading" | "error" | "finished";
  quota?: FlashQuotaLimits;
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
  const allowedSources = parseFlashSourceCidrs(value.allowedSourceCidrs);
  const deniedSources = parseFlashSourceCidrs(value.deniedSourceCidrs);
  const allowedSourceError = allowedSources.error ??
    (value.endpointMode === "web" && allowedSources.cidrs.length ? webSourceCidrError : null);
  const deniedSourceError = deniedSources.error ??
    (value.endpointMode === "web" && deniedSources.cidrs.length ? webSourceCidrError : null);
  const allowedDestinationError = parseFlashDestinationCidrs(
    value.allowedDestinationCidrs,
    true,
  ).error;
  const deniedDestinationError = parseFlashDestinationCidrs(
    value.deniedDestinationCidrs,
  ).error;
  const registryImageOptions = flashRegistryImageOptions(registryImages);
  const selectedRegistryImage =
    registryImageOptions.find((option) => option.value === value.image) ?? null;

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
          <SpaceBetween size="xs">
            <SegmentedControl
              selectedId={value.imageSource}
              options={[
                { id: "registry", text: "Flash Registry", disabled },
                { id: "manual", text: "直接入力", disabled },
              ]}
              label="イメージの指定方法"
              onChange={({ detail }) => {
                if (disabled) return;
                const imageSource = detail.selectedId as FlashImageSource;
                onChange({
                  ...value,
                  imageSource,
                  image:
                    imageSource === "registry" && !selectedRegistryImage
                      ? ""
                      : value.image,
                });
              }}
            />
            {value.imageSource === "registry" ? (
              <Select
                ariaLabel="Flash Registryイメージ"
                selectedAriaLabel="選択済み"
                selectedOption={selectedRegistryImage}
                options={registryImageOptions}
                filteringType="auto"
                filteringPlaceholder="リポジトリまたはタグを検索"
                filteringAriaLabel="Flash Registryイメージを検索"
                placeholder="イメージを選択"
                statusType={registryImagesStatus}
                loadingText="Flash Registryを読み込んでいます"
                errorText="Flash Registryからイメージを取得できません"
                empty="Flash Registryにタグ付きイメージがありません"
                disabled={disabled}
                onChange={({ detail }) =>
                  update("image", detail.selectedOption.value ?? "")
                }
              />
            ) : (
              <Input
                value={value.image}
                disabled={disabled}
                placeholder="ghcr.io/example/game-server:v1"
                onChange={({ detail }) => update("image", detail.value.slice(0, 500))}
              />
            )}
          </SpaceBetween>
        </FormField>
        <ColumnLayout columns={2}>
          <FormField label="リージョン">
            <Select
              ariaLabel="リージョン"
              selectedOption={regions.find((region) => region.value === value.region) ?? regions[0]}
              options={regions}
              disabled={disabled}
              onChange={({ detail }) => update("region", detail.selectedOption.value ?? regions[0].value)}
            />
          </FormField>
          <FormField
            label={value.scaleMode === "auto" ? "初期レプリカ" : "レプリカ"}
            constraintText={`1〜${quota.max_replicas_per_service.toLocaleString("ja-JP")}（アカウント上限）`}
          >
            <Input
              type="number"
              inputMode="numeric"
              step={1}
              nativeInputAttributes={{ min: 1, max: quota.max_replicas_per_service }}
              value={String(value.replicas)}
              disabled={disabled}
              onChange={({ detail }) =>
                update(
                  "replicas",
                  boundedInteger(
                    detail.value,
                    1,
                    quota.max_replicas_per_service,
                    value.replicas,
                  ),
                )
              }
            />
          </FormField>
        </ColumnLayout>
        <SpaceBetween size="s">
          <FormField label="スケーリング">
            <SegmentedControl
              label="スケーリング"
              selectedId={value.scaleMode}
              options={[{ id: "fixed", text: "固定", disabled }, { id: "auto", text: "自動", disabled }]}
              onChange={({ detail }) => {
                if (disabled) return;
                const scaleMode = detail.selectedId as FlashServiceFormValue["scaleMode"];
                onChange({
                  ...value,
                  scaleMode,
                  minReplicas: Math.min(value.minReplicas, value.replicas),
                  maxReplicas: Math.min(quota.max_replicas_per_service, Math.max(value.maxReplicas, value.replicas)),
                });
              }}
            />
          </FormField>
          {value.scaleMode === "auto" ? (
              <div className="flash-scale-controls">
                {([["minReplicas", "最小レプリカ"], ["maxReplicas", "最大レプリカ"]] as const).map(([key, label]) => (
                  <FormField key={key} label={label}>
                    <Input type="number" inputMode="numeric" step={1}
                      nativeInputAttributes={{ min: 1, max: quota.max_replicas_per_service }}
                      value={String(value[key])} disabled={disabled}
                      onChange={({ detail }) => {
                        const next = boundedInteger(detail.value, 1, quota.max_replicas_per_service, value[key]);
                        onChange({ ...value, [key]: next, replicas: key === "minReplicas"
                          ? Math.max(value.replicas, next) : Math.min(value.replicas, next) });
                      }} />
                  </FormField>
                ))}
                {([["cpuTargetEnabled", "cpuTarget", "CPU目標使用率 (%)"], ["memoryTargetEnabled", "memoryTarget", "メモリ目標使用率 (%)"]] as const).map(([enabledKey, targetKey, label]) => (
                  <FormField key={targetKey} label={
                    <Toggle checked={value[enabledKey]} disabled={disabled}
                      onChange={({ detail }) => update(enabledKey, detail.checked)}>{label}</Toggle>
                  }>
                    <Input ariaLabel={label} type="number" inputMode="numeric" step={1}
                      nativeInputAttributes={{ min: 1, max: 100 }}
                      value={String(value[targetKey])} disabled={disabled || !value[enabledKey]}
                      onChange={({ detail }) => update(targetKey, boundedInteger(detail.value, 1, 100, value[targetKey]))} />
                  </FormField>
                ))}
              </div>
            ) : null}
        </SpaceBetween>
        <ColumnLayout columns={3}>
          <FormField
            label="CPU"
            constraintText={`10〜${quota.max_cpu_millis_per_vm.toLocaleString("ja-JP")} millicores`}
          >
            <Input
              type="number"
              inputMode="numeric"
              step={100}
              nativeInputAttributes={{ min: 10, max: quota.max_cpu_millis_per_vm }}
              value={String(value.cpuMillis)}
              disabled={disabled}
              onChange={({ detail }) =>
                update(
                  "cpuMillis",
                  boundedInteger(
                    detail.value,
                    10,
                    quota.max_cpu_millis_per_vm,
                    value.cpuMillis,
                  ),
                )
              }
            />
          </FormField>
          <FormField
            label="メモリ"
            constraintText={`16〜${quota.max_memory_mib_per_vm.toLocaleString("ja-JP")} MiB`}
          >
            <Input
              type="number"
              inputMode="numeric"
              step={64}
              nativeInputAttributes={{ min: 16, max: quota.max_memory_mib_per_vm }}
              value={String(value.memoryMib)}
              disabled={disabled}
              onChange={({ detail }) =>
                update(
                  "memoryMib",
                  boundedInteger(
                    detail.value,
                    16,
                    quota.max_memory_mib_per_vm,
                    value.memoryMib,
                  ),
                )
              }
            />
          </FormField>
          <FormField
            label="ディスク上限"
            constraintText={`イメージ込み 1〜${quota.max_disk_gib_per_vm.toLocaleString("ja-JP")} GiB`}
          >
            <Input
              type="number"
              inputMode="numeric"
              step={1}
              nativeInputAttributes={{ min: 1, max: quota.max_disk_gib_per_vm }}
              value={String(value.ephemeralStorageGib)}
              disabled={disabled}
              onChange={({ detail }) =>
                update(
                  "ephemeralStorageGib",
                  boundedInteger(
                    detail.value,
                    1,
                    quota.max_disk_gib_per_vm,
                    value.ephemeralStorageGib,
                  ),
                )
              }
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
                    endpointMode: exposureType === "internal" ? "ip" : value.endpointMode,
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
                  disabled: disabled || value.exposureType === "internal" || value.endpointMode !== "ip",
                  disabledReason:
                    value.exposureType === "internal"
                      ? "内部サービスは転送モードで動作します。"
                      : undefined,
                },
              ]}
              label="通信モード"
              onChange={({ detail }) => {
                if (!disabled && !(detail.selectedId === "direct" && (value.exposureType === "internal" || value.endpointMode !== "ip"))) update("trafficMode", detail.selectedId as FlashExposure["traffic_mode"]);
              }}
            />
          </FormField>
        </ColumnLayout>
        {value.exposureType === "public" ? (
          <FormField label="公開アドレス">
            <SegmentedControl label="公開アドレス" selectedId={value.endpointMode}
              options={[{ id: "ip", text: "IP", disabled }, { id: "load_balancer", text: "ドメイン (LB)", disabled }, { id: "web", text: "HTTP/HTTPS ドメイン", disabled }]}
              onChange={({ detail }) => {
                if (disabled) return;
                const endpointMode = detail.selectedId as FlashServiceFormValue["endpointMode"];
                onChange({ ...value, endpointMode, trafficMode: endpointMode !== "ip" ? "forwarded" : value.trafficMode });
              }} />
          </FormField>
        ) : null}
        <SpaceBetween size="m">
          <Header
            variant="h3"
            actions={
              <Button
                iconName="add-plus"
                formAction="none"
                disabled={disabled || value.ports.length >= (value.endpointMode === "web" ? 1 : 16)}
                onClick={() =>
                  update("ports", [
                    ...value.ports,
                    {
                      name: `port-${value.ports.length + 1}`,
                      protocol: value.endpointMode === "web" ? "tcp" : "udp",
                      container_port: 7777,
                    },
                  ])
                }
              >
                エンドポイントを追加
              </Button>
            }
          >
            エンドポイント
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
                  disabled={disabled}
                  onClick={() => update("ports", value.ports.filter((_, portIndex) => portIndex !== index))}
                />
              </FormField>
            </ColumnLayout>
          ))}
        </SpaceBetween>
        <ColumnLayout columns={2}>
          <FormField
            label="受信許可元IP / CIDR"
            constraintText={value.endpointMode === "web" ? "HTTP/HTTPS公開では未対応" : "空欄の場合はすべて許可。1行につき1件"}
            errorText={allowedSourceError ?? undefined}
          >
            <Textarea
              value={value.allowedSourceCidrs}
              disabled={disabled}
              placeholder={"203.0.113.10\n2001:db8::/48"}
              rows={4}
              onChange={({ detail }) => update("allowedSourceCidrs", detail.value)}
            />
          </FormField>
          <FormField
            label="受信拒否元IP / CIDR"
            constraintText={value.endpointMode === "web" ? "HTTP/HTTPS公開では未対応" : "許可IPより優先。1行につき1件"}
            errorText={deniedSourceError ?? undefined}
          >
            <Textarea
              value={value.deniedSourceCidrs}
              disabled={disabled}
              placeholder={"198.51.100.25\n2001:db8:ffff::/48"}
              rows={4}
              onChange={({ detail }) => update("deniedSourceCidrs", detail.value)}
            />
          </FormField>
        </ColumnLayout>
        <SpaceBetween size="m">
          <Header variant="h3">送信アクセス</Header>
          <ColumnLayout columns={2}>
            <FormField label="外部ネットワーク">
              <SegmentedControl
                selectedId={value.egressMode}
                options={[
                  { id: "disabled", text: "無効", disabled },
                  { id: "restricted", text: "許可リスト", disabled },
                  { id: "internet", text: "公開インターネット", disabled },
                ]}
                label="外部ネットワーク"
                onChange={({ detail }) => {
                  if (disabled) return;
                  const egressMode = detail.selectedId as FlashEgressMode;
                  onChange({
                    ...value,
                    egressMode,
                    allowedDestinationCidrs:
                      egressMode === "restricted" ? value.allowedDestinationCidrs : "",
                    deniedDestinationCidrs:
                      egressMode === "disabled" ? "" : value.deniedDestinationCidrs,
                  });
                }}
              />
            </FormField>
            <FormField label="同一組織のFlashサービス">
              <Toggle
                checked={value.allowSameOrganization}
                disabled={disabled}
                onChange={({ detail }) => update("allowSameOrganization", detail.checked)}
              >
                通信を許可
              </Toggle>
            </FormField>
          </ColumnLayout>
          <ColumnLayout columns={2}>
            <FormField
              label="送信許可先IP / CIDR"
              constraintText="許可リストモードで使用。内部ネットワークは指定不可"
              errorText={allowedDestinationError ?? undefined}
            >
              <Textarea
                value={value.allowedDestinationCidrs}
                disabled={disabled || value.egressMode !== "restricted"}
                placeholder={"8.8.8.8/32\n2606:4700:4700::1111/128"}
                rows={4}
                onChange={({ detail }) => update("allowedDestinationCidrs", detail.value)}
              />
            </FormField>
            <FormField
              label="送信拒否先IP / CIDR"
              constraintText="公開インターネット・許可リストから除外"
              errorText={deniedDestinationError ?? undefined}
            >
              <Textarea
                value={value.deniedDestinationCidrs}
                disabled={disabled || value.egressMode === "disabled"}
                placeholder={"203.0.113.0/24\n2001:db8::/32"}
                rows={4}
                onChange={({ detail }) => update("deniedDestinationCidrs", detail.value)}
              />
            </FormField>
          </ColumnLayout>
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
        <FormField label="起動方法">
          <SegmentedControl
            selectedId={value.processMode}
            options={[
              { id: "image", text: "イメージ既定", disabled },
              { id: "workspace", text: "Web Shell待機", disabled },
              { id: "custom", text: "カスタム", disabled },
            ]}
            label="起動方法"
            onChange={({ detail }) => {
              if (!disabled) update("processMode", detail.selectedId as FlashProcessMode);
            }}
          />
        </FormField>
        {value.processMode === "custom" ? (
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
        ) : null}
        {children}
      </SpaceBetween>
    </form>
  );
}
