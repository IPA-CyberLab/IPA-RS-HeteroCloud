import Alert from "@cloudscape-design/components/alert";
import Box from "@cloudscape-design/components/box";
import Button from "@cloudscape-design/components/button";
import Container from "@cloudscape-design/components/container";
import FormField from "@cloudscape-design/components/form-field";
import Header from "@cloudscape-design/components/header";
import Input from "@cloudscape-design/components/input";
import SpaceBetween from "@cloudscape-design/components/space-between";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { api, getApiErrorMessage } from "@/lib/api-client";
import type { FlashService } from "@/lib/api-types";

export function FlashSecretsPanel({
  organizationId,
  service,
  secretFiles,
  onSecretFilesChange,
  onBusyChange,
  disabled,
}: {
  organizationId: string;
  service: FlashService;
  secretFiles: Record<string, string>;
  onSecretFilesChange: (files: Record<string, string>) => void;
  onBusyChange: (busy: boolean) => void;
  disabled: boolean;
}) {
  const [name, setName] = useState("");
  const [value, setValue] = useState("");
  const secrets = useQuery({
    queryKey: ["organizations", organizationId, "flash", "services", service.id, "secrets"],
    queryFn: ({ signal }) => api.flash.services.listSecrets(organizationId, service.id, signal),
  });
  const save = useMutation({
    mutationFn: () => api.flash.services.putSecret(organizationId, service.id, name, value),
    onSuccess: async () => {
      setName("");
      setValue("");
      await secrets.refetch();
    },
  });
  const remove = useMutation({
    mutationFn: (secretName: string) => api.flash.services.deleteSecret(organizationId, service.id, secretName),
    onSuccess: async () => { await secrets.refetch(); },
  });
  const busy = save.isPending || remove.isPending;
  useEffect(() => {
    onBusyChange(busy);
    return () => onBusyChange(false);
  }, [busy, onBusyChange]);
  const validName = /^[a-z][a-z0-9-]{0,61}[a-z0-9]$/.test(name) || /^[a-z]$/.test(name);
  const error = save.error ?? remove.error;

  return (
    <Container header={<Header variant="h2">コンテナのシークレット</Header>}>
      <SpaceBetween size="m">
        <Box>登録・更新した値はOpenBaoに保存します。接続・解除は「変更を保存」で反映されます。接続したシークレットはコンテナ内の <code>/vault/secrets/名前</code> に読み取り専用ファイルとして配置されます。</Box>
        {secrets.isError ? <Alert type="error">シークレット一覧を取得できません。Secret Managerの接続を確認してください。</Alert> : null}
        {error ? <Alert type="error">{getApiErrorMessage(error)}</Alert> : null}
        {secrets.data?.items.length ? secrets.data.items.map((secretName) => {
          const attachedFile = Object.entries(secretFiles).find(([, selected]) => selected === secretName)?.[0];
          const persistedFile = Object.entries(service.spec.secret_files ?? {}).find(([, selected]) => selected === secretName)?.[0];
          return (
            <SpaceBetween key={secretName} direction="horizontal" size="s" alignItems="center">
              <Box variant="code">{secretName}</Box>
              <Box>{attachedFile ? `/vault/secrets/${attachedFile}` : "未接続"}</Box>
              {attachedFile ? (
                <Button disabled={disabled || busy} onClick={() => {
                  const next = { ...secretFiles };
                  delete next[attachedFile];
                  onSecretFilesChange(next);
                }}>接続解除</Button>
              ) : (
                <Button disabled={disabled || busy || Object.keys(secretFiles).length >= 32} onClick={() =>
                  onSecretFilesChange({ ...secretFiles, [secretName]: secretName })
                }>コンテナへ接続</Button>
              )}
              <Button disabled={disabled || Boolean(attachedFile || persistedFile) || busy} onClick={() => remove.mutate(secretName)}>削除</Button>
            </SpaceBetween>
          );
        }) : (!secrets.isPending && !secrets.isError ? <Box color="text-body-secondary">登録済みのシークレットはありません。</Box> : null)}
        <SpaceBetween direction="horizontal" size="s" alignItems="end">
          <FormField label="名前" errorText={name && !validName ? "英小文字で始まる英小文字・数字・ハイフン、63文字以内" : undefined}>
            <Input value={name} disabled={disabled} onChange={({ detail }) => setName(detail.value)} placeholder="database-url" />
          </FormField>
          <FormField label="値" description="保存後は表示されません">
            <Input type="password" value={value} disabled={disabled} onChange={({ detail }) => setValue(detail.value)} />
          </FormField>
          <Button variant="primary" loading={save.isPending} disabled={disabled || !validName || !value || value.length > 16_384 || secrets.isError} onClick={() => save.mutate()}>登録・更新</Button>
        </SpaceBetween>
      </SpaceBetween>
    </Container>
  );
}
