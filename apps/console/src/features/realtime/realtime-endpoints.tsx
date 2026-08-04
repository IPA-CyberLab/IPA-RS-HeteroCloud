import Box from "@cloudscape-design/components/box";
import Button from "@cloudscape-design/components/button";
import Table from "@cloudscape-design/components/table";
import { useState } from "react";
import type { RealtimeServiceEndpoints } from "@/lib/api-types";
import { endpointCount, endpointGroups } from "./realtime-service-utils";

export function RealtimeEndpoints({
  endpoints,
  emptyLabel = "利用可能なエンドポイントはまだありません。",
}: {
  endpoints: RealtimeServiceEndpoints;
  emptyLabel?: string;
}) {
  const [copied, setCopied] = useState<string | null>(null);
  const items = endpointGroups.flatMap((group) =>
    endpoints[group.key].map((endpoint) => ({
      id: `${group.key}-${endpoint}`,
      kind: group.label,
      endpoint,
    })),
  );
  const copy = async (endpoint: string) => {
    await navigator.clipboard.writeText(endpoint);
    setCopied(endpoint);
    window.setTimeout(() => setCopied(null), 1_500);
  };

  return (
    <Table
      variant="embedded"
      items={items}
      trackBy="id"
      columnDefinitions={[
        { id: "kind", header: "種別", cell: (item) => item.kind, width: 140 },
        { id: "endpoint", header: "エンドポイント", cell: (item) => <Box variant="code">{item.endpoint}</Box> },
        {
          id: "copy",
          header: "",
          width: 64,
          cell: (item) => (
            <Button
              variant="inline-icon"
              iconName={copied === item.endpoint ? "check" : "copy"}
              ariaLabel={`${item.kind}エンドポイントをコピー`}
              onClick={() => void copy(item.endpoint)}
            />
          ),
        },
      ]}
      empty={<Box textAlign="center" color="text-body-secondary">{endpointCount(endpoints) === 0 ? emptyLabel : "-"}</Box>}
    />
  );
}
