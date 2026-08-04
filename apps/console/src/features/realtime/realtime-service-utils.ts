import type {
  RealtimeService,
  RealtimeServiceEndpoints,
  RealtimeServiceMetricSample,
  RealtimeServiceMetrics,
} from "@/lib/api-types";

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

function counterIncrease(current: number, previous: number): number {
  if (!Number.isFinite(current) || current < 0) return 0;
  if (!Number.isFinite(previous) || previous < 0 || current < previous) {
    return current;
  }
  return current - previous;
}

export function transferRateSamplesPerHour(
  samples: RealtimeServiceMetricSample[],
): RealtimeServiceMetricSample[] {
  const ordered = [...samples].sort(
    (left, right) => Date.parse(left.sampled_at) - Date.parse(right.sampled_at),
  );

  return ordered.map((sample, index) => {
    const previous = ordered[index - 1];
    if (!previous) {
      return {
        ...sample,
        ingress_bytes: 0,
        egress_bytes: 0,
        transferred_bytes: 0,
      };
    }

    const elapsedMilliseconds =
      Date.parse(sample.sampled_at) - Date.parse(previous.sampled_at);
    if (!Number.isFinite(elapsedMilliseconds) || elapsedMilliseconds <= 0) {
      return {
        ...sample,
        ingress_bytes: 0,
        egress_bytes: 0,
        transferred_bytes: 0,
      };
    }

    const ingressBytes =
      (counterIncrease(sample.ingress_bytes, previous.ingress_bytes) * 3_600_000) /
      elapsedMilliseconds;
    const egressBytes =
      (counterIncrease(sample.egress_bytes, previous.egress_bytes) * 3_600_000) /
      elapsedMilliseconds;
    return {
      ...sample,
      ingress_bytes: ingressBytes,
      egress_bytes: egressBytes,
      transferred_bytes: ingressBytes + egressBytes,
    };
  });
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
