import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Alert, Box, Button, FormField, Select, SpaceBetween, Toggle } from "@/components/ui";
import { api, getApiErrorMessage } from "@/lib/api-client";

export function FlashLogsPanel({ organizationId, serviceId }: { organizationId: string; serviceId: string }) {
  const [selected, setSelected] = useState("");
  const [previous, setPrevious] = useState(false);
  const base = ["organizations", organizationId, "flash", "services", serviceId, "logs"];
  const containers = useQuery({
    queryKey: [...base, "containers"],
    queryFn: ({ signal }) => api.flash.services.getLogs(organizationId, serviceId, undefined, false, signal),
    retry: false, gcTime: 0, refetchOnWindowFocus: false,
  });
  const options = (containers.data?.items ?? []).map(item => ({ label: `${item.name} (${item.phase})`, value: item.name }));
  const pod = options.some(item => item.value === selected) ? selected : options[0]?.value ?? "";
  const logs = useQuery({
    queryKey: [...base, pod, previous],
    queryFn: ({ signal }) => api.flash.services.getLogs(organizationId, serviceId, pod, previous, signal),
    enabled: Boolean(pod), retry: false, gcTime: 0, refetchOnWindowFocus: false,
  });
  return <SpaceBetween size="m">
    <Box>実行中または直前のコンテナのログを表示します。最新の500行、最大256 KiBまで取得します。</Box>
    {containers.isError && <Alert type="error">{getApiErrorMessage(containers.error)}</Alert>}
    {containers.isPending && <Box>コンテナを取得しています。</Box>}
    {containers.data && !options.length && <Box>現在表示できるコンテナはありません。</Box>}
    {Boolean(options.length) && <>
      <FormField label="コンテナ"><Select ariaLabel="ログを表示するコンテナ" options={options} selectedOption={options.find(item => item.value === pod) ?? null} onChange={({ detail }) => setSelected(detail.selectedOption.value ?? "")} /></FormField>
      <Toggle checked={previous} onChange={({ detail }) => setPrevious(detail.checked)}>前回終了したコンテナのログ</Toggle>
      <Button loading={logs.isFetching} onClick={() => { void containers.refetch(); void logs.refetch(); }}>更新</Button>
      {logs.isError && <Alert type="error">ログを取得できませんでした。前回のログがない場合もあります。{getApiErrorMessage(logs.error)}</Alert>}
      {logs.data?.truncated && <Alert type="warning">ログの一部のみを表示しています。</Alert>}
      {logs.data?.text !== null && logs.data?.text !== undefined && <pre style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere", maxHeight: "32rem", overflow: "auto" }}>{logs.data.text || "ログはありません。"}</pre>}
    </>}
  </SpaceBetween>;
}
