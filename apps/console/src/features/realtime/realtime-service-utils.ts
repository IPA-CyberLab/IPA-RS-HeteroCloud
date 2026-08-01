import type {
  RealtimeService,
  RealtimeServiceEndpoints,
  RealtimeServiceMetrics,
  TrafficMode,
} from "@/lib/api-types";

export const trafficModeLabels: Record<TrafficMode, string> = {
  direct: "ダイレクト",
  forwarded: "転送",
};

export const endpointGroups = [
  { key: "api", label: "API" },
  { key: "signaling", label: "シグナリング" },
  { key: "livekit", label: "LiveKit" },
  { key: "stun", label: "STUN" },
  { key: "turn", label: "TURN" },
] as const satisfies ReadonlyArray<{
  key: keyof RealtimeServiceEndpoints;
  label: string;
}>;

export const realtimePermissions = [
  { value: "flow.room.create", label: "ルーム作成" },
  { value: "flow.room.read", label: "ルーム参照" },
  { value: "flow.room.join", label: "ルーム参加" },
  { value: "flow.signal.connect", label: "シグナリング接続" },
  { value: "flow.turn.issue", label: "TURN認証情報発行" },
  { value: "flow.metrics.read", label: "メトリクス参照" },
] as const;

export function emptyEndpoints(): RealtimeServiceEndpoints {
  return {
    api: [],
    signaling: [],
    livekit: [],
    stun: [],
    turn: [],
  };
}

function stringArray(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.filter((item): item is string => typeof item === "string");
}

export function normalizeEndpoints(value: unknown): RealtimeServiceEndpoints {
  if (Array.isArray(value)) {
    return { ...emptyEndpoints(), api: stringArray(value) };
  }
  if (!value || typeof value !== "object") return emptyEndpoints();

  const source = value as Record<string, unknown>;
  return {
    api: stringArray(source.api ?? source.api_urls ?? source.endpoints),
    signaling: stringArray(
      source.signaling ?? source.signaling_urls ?? source.websocket,
    ),
    livekit: stringArray(source.livekit ?? source.livekit_urls ?? source.sfu),
    stun: stringArray(source.stun ?? source.stun_urls),
    turn: stringArray(source.turn ?? source.turn_urls),
  };
}

export function serviceEndpoints(
  service: Pick<RealtimeService, "status">,
): RealtimeServiceEndpoints {
  return normalizeEndpoints(service.status.endpoints ?? service.status);
}

export function endpointCount(endpoints: RealtimeServiceEndpoints): number {
  return endpointGroups.reduce(
    (count, group) => count + endpoints[group.key].length,
    0,
  );
}

export function formatBytes(value: number): string {
  if (!Number.isFinite(value) || value < 0) return "—";
  if (value < 1_000) return `${Math.round(value)} B`;

  const units = ["KB", "MB", "GB", "TB", "PB"];
  let amount = value / 1_000;
  let unitIndex = 0;
  while (amount >= 1_000 && unitIndex < units.length - 1) {
    amount /= 1_000;
    unitIndex += 1;
  }
  return `${new Intl.NumberFormat("ja-JP", {
    maximumFractionDigits: amount >= 100 ? 0 : amount >= 10 ? 1 : 2,
  }).format(amount)} ${units[unitIndex]}`;
}

export function transferredBytes(
  metrics: Pick<
    RealtimeServiceMetrics,
    "transferred_bytes" | "ingress_bytes" | "egress_bytes"
  >,
): number {
  return Number.isFinite(metrics.transferred_bytes)
    ? metrics.transferred_bytes
    : metrics.ingress_bytes + metrics.egress_bytes;
}

export function formatCredentialDate(value: string | number): string {
  const normalized =
    typeof value === "number" ? new Date(value * 1_000).toISOString() : value;
  const date = new Date(normalized);
  if (Number.isNaN(date.getTime())) return "—";
  return new Intl.DateTimeFormat("ja-JP", {
    dateStyle: "medium",
    timeStyle: "medium",
  }).format(date);
}
