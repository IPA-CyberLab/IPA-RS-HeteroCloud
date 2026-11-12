import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Alert, Box, Button, FormField, Input, Modal, SpaceBetween, Toggle } from "@/components/ui";
import { api, getApiErrorMessage } from "@/lib/api-client";
import type { FlashBackupDestination, FlashBackupDestinationInput } from "@/lib/api-types";

export const backupDestinationsKey = (organizationId: string) => ["organizations", organizationId, "flash", "backup-destinations"];
export function useBackupDestinations(organizationId: string) {
  return useQuery({ queryKey: backupDestinationsKey(organizationId), queryFn: ({signal}) => api.flash.backupDestinations.list(organizationId, signal), refetchInterval: 15000, retry: false });
}

const empty = (): FlashBackupDestinationInput => ({ name: "", request_id: crypto.randomUUID(), endpoint: "", bucket: "", region: "", prefix: "", virtual_hosted: false, access_key: "", secret_key: "" });

export function FlashBackupDestinations({organizationId, items, editable = true}: {organizationId: string; items: FlashBackupDestination[]; editable?: boolean}) {
  const client = useQueryClient();
  const [open, setOpen] = useState(false);
  const [rotation, setRotation] = useState<FlashBackupDestination | null>(null);
  const [input, setInput] = useState(empty);
  const invalidate = () => client.invalidateQueries({queryKey: backupDestinationsKey(organizationId)});
  const save = useMutation({ mutationFn: () => rotation
    ? api.flash.backupDestinations.rotate(organizationId, rotation.id, {access_key: input.access_key, secret_key: input.secret_key})
    : api.flash.backupDestinations.create(organizationId, input),
    onSuccess: () => { setOpen(false); setRotation(null); setInput(empty()); void invalidate(); },
  });
  const dismiss = () => { if (!save.isPending) { setOpen(false); setRotation(null); setInput(empty()); save.reset(); } };
  const field = (key: keyof FlashBackupDestinationInput, value: string | boolean) => setInput(current => ({ ...current, [key]: value, ...(!["access_key", "secret_key"].includes(key) ? {request_id: crypto.randomUUID()} : {}) }));
  return <SpaceBetween size="s">
    <Box>保存先: Syouyu（既定）</Box>
    {items.map(item => <Box key={item.id}>{item.name} — {item.bucket} / {item.state === "ready" ? "前回接続可能" : "接続確認中"} {editable && <Button onClick={() => { setRotation(item); setInput(empty()); setOpen(true); }}>認証情報を更新</Button>}</Box>)}
    {editable && <Button onClick={() => { setRotation(null); setInput(empty()); setOpen(true); }}>S3の保存先を追加</Button>}
    <Modal visible={open} onDismiss={dismiss} header={rotation ? "S3の認証情報を更新" : "S3の保存先を追加"} footer={<SpaceBetween direction="horizontal" size="s"><Button disabled={save.isPending} onClick={dismiss}>キャンセル</Button><Button variant="primary" loading={save.isPending} disabled={!input.access_key || !input.secret_key || (!rotation && (!input.name || !input.endpoint || !input.bucket || !input.region))} onClick={() => save.mutate()}>保存</Button></SpaceBetween>}>
      <SpaceBetween size="m">
        <Box>認証情報はシークレットとして保存し、再表示しません。S3には指定プレフィックス以下の組織専用領域を使用します。</Box>
        {!rotation && <>
          <FormField label="表示名"><Input ariaLabel="保存先の表示名" value={input.name} onChange={({detail}) => field("name", detail.value)}/></FormField>
          <FormField label="S3 エンドポイント" description="公開 HTTPS のオリジンを指定します。"><Input ariaLabel="S3 エンドポイント" placeholder="https://s3.example.com" value={input.endpoint} onChange={({detail}) => field("endpoint", detail.value)}/></FormField>
          <FormField label="バケット"><Input ariaLabel="S3 バケット" value={input.bucket} onChange={({detail}) => field("bucket", detail.value)}/></FormField>
          <FormField label="リージョン"><Input ariaLabel="S3 リージョン" placeholder="ap-northeast-1" value={input.region} onChange={({detail}) => field("region", detail.value)}/></FormField>
          <FormField label="プレフィックス（任意）"><Input ariaLabel="S3 プレフィックス" value={input.prefix} onChange={({detail}) => field("prefix", detail.value)}/></FormField>
          <Toggle checked={input.virtual_hosted} onChange={({detail}) => field("virtual_hosted", detail.checked)}>バケットをホスト名で指定する</Toggle>
        </>}
        <FormField label="アクセスキー"><Input ariaLabel="S3 アクセスキー" type="password" autoComplete="off" value={input.access_key} onChange={({detail}) => field("access_key", detail.value)}/></FormField>
        <FormField label="シークレットキー"><Input ariaLabel="S3 シークレットキー" type="password" autoComplete="new-password" value={input.secret_key} onChange={({detail}) => field("secret_key", detail.value)}/></FormField>
        {save.isError && <Alert type="error">{getApiErrorMessage(save.error)}</Alert>}
      </SpaceBetween>
    </Modal>
  </SpaceBetween>;
}
