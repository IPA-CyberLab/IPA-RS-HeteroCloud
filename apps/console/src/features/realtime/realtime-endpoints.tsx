import { Check, Copy, Network } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import type { RealtimeServiceEndpoints } from "@/lib/api-types";
import { endpointCount, endpointGroups } from "./realtime-service-utils";

interface RealtimeEndpointsProps {
  endpoints: RealtimeServiceEndpoints;
  emptyLabel?: string;
}

async function copyText(value: string): Promise<void> {
  await navigator.clipboard.writeText(value);
}

export function RealtimeEndpoints({
  endpoints,
  emptyLabel = "利用可能なエンドポイントはまだありません。",
}: RealtimeEndpointsProps) {
  const [copied, setCopied] = useState<string | null>(null);

  if (endpointCount(endpoints) === 0) {
    return (
      <div className="flex min-h-32 items-center justify-center px-5 text-sm text-zinc-500">
        {emptyLabel}
      </div>
    );
  }

  const handleCopy = async (endpoint: string) => {
    try {
      await copyText(endpoint);
      setCopied(endpoint);
      window.setTimeout(() => setCopied(null), 1_500);
    } catch {
      setCopied(null);
    }
  };

  return (
    <div className="divide-y divide-zinc-100">
      {endpointGroups.flatMap((group) =>
        endpoints[group.key].map((endpoint) => (
          <div
            key={`${group.key}-${endpoint}`}
            className="grid gap-2 px-4 py-3 sm:grid-cols-[8rem_minmax(0,1fr)_2.25rem] sm:items-center"
          >
            <span className="flex items-center gap-2 text-xs font-medium text-zinc-600">
              <Network className="size-3.5" />
              {group.label}
            </span>
            <code className="break-all text-xs text-zinc-800">{endpoint}</code>
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="justify-self-end"
              title={`${group.label}エンドポイントをコピー`}
              aria-label={`${group.label}エンドポイントをコピー`}
              onClick={() => void handleCopy(endpoint)}
            >
              {copied === endpoint ? <Check /> : <Copy />}
            </Button>
          </div>
        )),
      )}
    </div>
  );
}
