import type {
  FlashPortProtocol,
  FlashService,
  FlashServiceEndpoint,
  FlashServiceStatus,
} from "@/lib/api-types";

export interface DisplayFlashEndpoint {
  key: string;
  name: string;
  protocol: string;
  address: string;
}

export function flashProviderStatus(
  status: FlashServiceStatus,
): FlashServiceStatus {
  return status.status && typeof status.status === "object"
    ? status.status
    : status;
}

function endpointAddress(endpoint: FlashServiceEndpoint): string | null {
  const transport = ["tcp", "udp"].includes(endpoint.protocol?.toLowerCase() ?? "");
  if (endpoint.url && !transport) return endpoint.url;
  let host = endpoint.host ?? endpoint.address;
  if (!host && endpoint.url && transport) {
    try {
      const url = new URL(endpoint.url);
      host = url.hostname;
      if (!endpoint.port && url.port) return `${host}:${url.port}`;
    } catch {
      return null;
    }
  }
  if (!host) return null;
  if (host.includes(":") && !host.startsWith("[")) host = `[${host}]`;
  return endpoint.port ? `${host}:${endpoint.port}` : host;
}

function endpointProtocol(value: unknown): string {
  if (typeof value !== "string") return "-";
  return value.toUpperCase();
}

function endpointFromObject(
  endpoint: FlashServiceEndpoint,
  index: number,
  fallbackName?: string,
): DisplayFlashEndpoint | null {
  const address = endpointAddress(endpoint);
  if (!address) return null;
  const name = endpoint.name ?? fallbackName ?? `endpoint-${index + 1}`;
  return {
    key: `${name}-${endpoint.protocol ?? "endpoint"}-${address}-${index}`,
    name,
    protocol: endpointProtocol(endpoint.protocol),
    address,
  };
}

export function flashServiceEndpoints(
  status: FlashServiceStatus,
): DisplayFlashEndpoint[] {
  const source = flashProviderStatus(status).endpoints;
  if (Array.isArray(source)) {
    return source.flatMap((endpoint, index) => {
      if (typeof endpoint === "string") {
        return [{
          key: `endpoint-${endpoint}-${index}`,
          name: `endpoint-${index + 1}`,
          protocol: "-",
          address: endpoint,
        }];
      }
      if (!endpoint || typeof endpoint !== "object") return [];
      const normalized = endpointFromObject(
        endpoint as FlashServiceEndpoint,
        index,
      );
      return normalized ? [normalized] : [];
    });
  }

  if (!source || typeof source !== "object") return [];
  return Object.entries(source).flatMap(([name, value], index) => {
    if (typeof value === "string") {
      return [{
        key: `${name}-${value}-${index}`,
        name,
        protocol: "-",
        address: value,
      }];
    }
    if (Array.isArray(value)) {
      return value.flatMap((address, addressIndex) =>
        typeof address === "string"
          ? [{
              key: `${name}-${address}-${addressIndex}`,
              name,
              protocol: "-",
              address,
            }]
          : [],
      );
    }
    if (!value || typeof value !== "object") return [];
    const normalized = endpointFromObject(
      value as FlashServiceEndpoint,
      index,
      name,
    );
    return normalized ? [normalized] : [];
  });
}

export function readyReplicas(service: Pick<FlashService, "status">): number {
  const status = flashProviderStatus(service.status);
  const value =
    status.ready_replicas ?? status.available_replicas ?? 0;
  return typeof value === "number" && Number.isFinite(value) ? value : 0;
}

export function requestedReplicas(service: Pick<FlashService, "status" | "spec">): number | null {
  const status = flashProviderStatus(service.status);
  const value = status.requested_replicas ?? status.replicas;
  if (typeof value === "number" && Number.isInteger(value) && value >= 0) return value;
  return service.spec.autoscaling ? null : service.spec.replicas;
}

export function flashScaleLabel(spec: FlashService["spec"]): string {
  const scale = spec.autoscaling;
  if (!scale) return `固定・${spec.replicas}`;
  return `自動・${scale.min_replicas}〜${scale.max_replicas}`;
}

export function flashProtocolLabel(protocol: FlashPortProtocol): string {
  return protocol.toUpperCase();
}

export function flashExposureLabel(
  exposure: Pick<FlashService["spec"]["exposure"], "type" | "traffic_mode" | "endpoint_mode">,
): string {
  if (exposure.type === "internal") return "内部";
  if (exposure.endpoint_mode === "load_balancer") return "公開・ドメイン (LB)";
  return exposure.traffic_mode === "direct" ? "公開・ダイレクト" : "公開・転送";
}
