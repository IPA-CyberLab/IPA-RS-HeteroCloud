import { Box, ColumnLayout, Container, Header, SpaceBetween } from "@/components/ui";
import type { ResourceQuotaLimits } from "@/lib/api-types";
import { formatNumber } from "@/lib/utils";

const descriptions: Record<keyof ResourceQuotaLimits, Record<string, string>> = {
  flow: {
    max_services: "サービス数", max_rooms_per_service: "ルーム / サービス",
    max_total_rooms: "合計ルーム", max_participants_per_service: "同時参加者 / サービス",
    max_rate_limit_requests_per_second: "IPあたり RPS", max_rate_limit_burst: "IPあたりバースト",
    max_developer_credentials_per_service: "開発者認証情報 / サービス",
  },
  flash: {
    max_services: "サービス数", max_replicas_per_service: "レプリカ / サービス",
    max_total_replicas: "合計レプリカ", max_cpu_millis_per_vm: "CPU / VM (millicores)",
    max_total_cpu_millis: "合計 CPU (millicores)", max_memory_mib_per_vm: "メモリ / VM (MiB)",
    max_total_memory_mib: "合計メモリ (MiB)", max_disk_gib_per_vm: "ディスク / VM (GiB)",
    max_total_disk_gib: "合計ディスク (GiB)",
    max_weekly_cpu_millicore_seconds: "週次 CPU (millicore-seconds)",
    max_weekly_memory_mib_seconds: "週次メモリ (MiB-seconds)",
    max_weekly_gpu_seconds: "週次 GPU (秒)",
  },
  registry: { storage_gib: "保存容量 (GiB)", max_credentials: "認証情報数" },
  syouyu: {
    max_buckets: "バケット数", max_bytes_per_bucket: "容量 / バケット (bytes)",
    max_objects_per_bucket: "オブジェクト / バケット", max_total_bytes: "合計容量 (bytes)",
    max_credentials_per_bucket: "認証情報 / バケット", max_total_credentials: "合計認証情報",
  },
};

export function QuotaSummary({ limits, member = false }: { limits: ResourceQuotaLimits; member?: boolean }) {
  const groups: [keyof ResourceQuotaLimits, string][] = [
    ["flash", "Flash"], ["registry", "Flash Registry"], ["flow", "Flow"], ["syouyu", "Syouyu"],
  ];
  return <SpaceBetween size="m">
    {groups.map(([key, name]) => <Container key={key} header={<Header variant="h3">{name}</Header>}>
      <ColumnLayout columns={3} variant="text-grid">
        {Object.entries(limits[key]).map(([field, value]) => {
          const shared = member && (key === "registry" || (key === "syouyu" && ["max_credentials_per_bucket", "max_total_credentials"].includes(field)));
          return <div key={field}>
            <Box variant="awsui-key-label">{descriptions[key][field] ?? field}{shared ? "（組織共通）" : ""}</Box>
            <Box>{formatNumber(value)}</Box>
          </div>;
        })}
      </ColumnLayout>
    </Container>)}
  </SpaceBetween>;
}
