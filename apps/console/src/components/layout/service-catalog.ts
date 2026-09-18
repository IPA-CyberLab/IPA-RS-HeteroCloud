export type ServiceGroup = "Flash" | "Flash Registry" | "プラットフォーム";

export interface ConsoleService {
  id: "flash" | "flash-registry" | "flow" | "syouyu";
  name: string;
  shortName: string;
  description: string;
  group: ServiceGroup;
  href: string;
  keywords: string[];
}

export const consoleServices: ConsoleService[] = [
  {
    id: "flash",
    name: "Flash",
    shortName: "コンテナサービス",
    description: "コンテナをVM分離で実行し、負荷に合わせてスケールします。",
    group: "Flash",
    href: "/flash/services",
    keywords: ["compute", "container", "gpu", "vm", "コンピュート"],
  },
  {
    id: "flash-registry",
    name: "Flash Registry",
    shortName: "コンテナイメージ",
    description: "Flashで使うコンテナイメージと認証情報を管理します。",
    group: "Flash Registry",
    href: "/registry",
    keywords: ["registry", "image", "artifact", "イメージ"],
  },
  {
    id: "flow",
    name: "Flow",
    shortName: "リアルタイム通信",
    description: "WebRTC、LiveKit、STUN、TURNを一つのサービスで提供します。",
    group: "プラットフォーム",
    href: "/flow/services",
    keywords: ["webrtc", "livekit", "stun", "turn", "realtime"],
  },
  {
    id: "syouyu",
    name: "Syouyu",
    shortName: "オブジェクトストレージ",
    description: "アプリケーションデータをS3互換バケットに保存します。",
    group: "プラットフォーム",
    href: "/syouyu/buckets",
    keywords: ["storage", "s3", "bucket", "ストレージ"],
  },
];

const recentServicesKey = "heterocloud.recent-services.v1";
const recentServicesChanged = "heterocloud:recent-services-changed";

export function serviceForPath(pathname: string): ConsoleService | undefined {
  return consoleServices.find(
    (service) => pathname === service.href || pathname.startsWith(`${service.href}/`),
  );
}

export function readRecentServiceIds(): ConsoleService["id"][] {
  if (typeof window === "undefined") return [];
  try {
    const value = JSON.parse(window.localStorage.getItem(recentServicesKey) ?? "[]");
    if (!Array.isArray(value)) return [];
    const knownIds = new Set(consoleServices.map((service) => service.id));
    return value.filter(
      (id): id is ConsoleService["id"] => typeof id === "string" && knownIds.has(id as ConsoleService["id"]),
    );
  } catch {
    return [];
  }
}

export function rememberServiceVisit(serviceId: ConsoleService["id"]): void {
  if (typeof window === "undefined") return;
  const ids = [serviceId, ...readRecentServiceIds().filter((id) => id !== serviceId)].slice(0, 4);
  try {
    window.localStorage.setItem(recentServicesKey, JSON.stringify(ids));
    window.dispatchEvent(new Event(recentServicesChanged));
  } catch {
    // The console remains usable when storage is unavailable or disabled.
  }
}

export function subscribeToRecentServices(listener: () => void): () => void {
  if (typeof window === "undefined") return () => undefined;
  window.addEventListener("storage", listener);
  window.addEventListener(recentServicesChanged, listener);
  return () => {
    window.removeEventListener("storage", listener);
    window.removeEventListener(recentServicesChanged, listener);
  };
}
