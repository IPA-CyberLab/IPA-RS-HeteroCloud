import { Alert, Box, Button, Container, FormField, Header, Input, Select, SpaceBetween, Textarea } from "@/components/ui";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { api, getApiErrorMessage } from "@/lib/api-client";
import type { FlashSshKeyPreview } from "@/lib/api-types";
import { useSession } from "@/features/auth/session";

export function FlashSshPanel({organizationId, serviceId, sshEnabled, editable = false}: {organizationId: string; serviceId: string; sshEnabled: boolean; editable?: boolean}) {
  const client = useQueryClient();
  const userId = useSession().data?.user.id;
  const keysKey = ["flash", organizationId, "ssh-keys"];
  const bindingsKey = ["flash", organizationId, serviceId, "ssh-keys"];
  const keys = useQuery({queryKey: keysKey, queryFn: ({signal}) => api.flash.sshKeys.list(organizationId, signal)});
  const info = useQuery({queryKey: ["flash", organizationId, "ssh-info"], queryFn: ({signal}) => api.flash.sshKeys.info(organizationId, signal)});
  const bindings = useQuery({queryKey: bindingsKey, queryFn: ({signal}) => api.flash.sshKeys.forService(organizationId, serviceId, signal)});
  const [selected, setSelected] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [publicKey, setPublicKey] = useState("");
  const [github, setGithub] = useState("");
  const [githubResults, setGithubResults] = useState<{username: string; items: FlashSshKeyPreview[]} | null>(null);
  const register = useMutation({
    mutationFn: (input: {name: string; public_key: string; github_username?: string}) => api.flash.sshKeys.create(organizationId, input),
    onSuccess: (item) => { setName(""); setPublicKey(""); setSelected(item.id); void client.invalidateQueries({queryKey: keysKey}); },
  });
  const preview = useMutation({
    mutationFn: (username: string) => api.flash.sshKeys.previewGithub(organizationId, username),
    onSuccess: setGithubResults,
  });
  const assign = useMutation({
    mutationFn: ({id, enabled}: {id: string; enabled: boolean}) => api.flash.sshKeys.setForService(organizationId, serviceId, id, enabled),
    onSuccess: (result) => { client.setQueryData(bindingsKey, result); setSelected(null); },
  });
  const removeKey = useMutation({
    mutationFn: (id: string) => api.flash.sshKeys.delete(organizationId, id),
    onSuccess: () => { void client.invalidateQueries({queryKey: keysKey}); void client.invalidateQueries({queryKey: bindingsKey}); },
  });
  const assigned = bindings.data?.key_ids ?? [];
  const available = (keys.data?.items ?? []).filter((key) => !assigned.includes(key.id));
  const selectedKey = available.find((key) => key.id === selected);
  const error = keys.error || bindings.error || register.error || preview.error || assign.error || removeKey.error;
  return <Container header={<Header variant="h2">SSH 接続</Header>}>
    <SpaceBetween size="m">
      <Box>SSH: {sshEnabled ? "有効" : "無効"}</Box>
      {editable && <Box>公開鍵を登録し、このサービスへの接続を許可する鍵を選択してください。鍵の所有者は、登録したユーザーの権限でこのサービスに接続します。</Box>}
      {sshEnabled && info.data?.enabled && <>
        <Box>接続: <code>{`heterocloud --endpoint ${window.location.origin} --organization-id ${organizationId} flash ssh ${serviceId}`}</code></Box>
        {info.data.tcp && <Box>直接接続: <code>{`ssh -p ${info.data.tcp.port} -l ${organizationId}+${serviceId} ${info.data.tcp.host}`}</code></Box>}
        <Box>初回接続時のホスト鍵: <code>{info.data.host_key_fingerprint}</code></Box>
      </>}
      {sshEnabled && info.data && !info.data.enabled && <Alert type="warning">SSH 接続の準備ができていません。</Alert>}
      {error && <Alert type="error">{getApiErrorMessage(error)}</Alert>}
      {editable && <><FormField label="登録済みの公開鍵">
        <Select ariaLabel="接続を許可する公開鍵" selectedOption={selectedKey ? {value: selectedKey.id, label: selectedKey.name} : null}
          options={available.map((key) => ({value: key.id, label: `${key.name} · ${key.fingerprint}`}))}
          placeholder="公開鍵を選択" onChange={({detail}) => setSelected(detail.selectedOption.value ?? null)} />
      </FormField>
      <Button formAction="none" disabled={!selected || bindings.isPending || assign.isPending} loading={assign.isPending} onClick={() => selected && assign.mutate({id: selected, enabled: true})}>この鍵で接続を許可</Button></>}
      {assigned.length === 0 && !bindings.isPending && <Box>このサービスに許可された鍵はありません。</Box>}
      {assigned.map((id) => {
        const key = keys.data?.items.find((item) => item.id === id);
        return <SpaceBetween key={id} direction="horizontal" size="s">
          <Box>{key ? `${key.name} · ${key.fingerprint}` : id}</Box>
          {editable && <Button formAction="none" disabled={assign.isPending} onClick={() => assign.mutate({id, enabled: false})}>接続許可を解除</Button>}
        </SpaceBetween>;
      })}
      {editable && <><Header variant="h3">公開鍵を貼り付けて登録</Header>
      <FormField label="鍵の名前"><Input ariaLabel="鍵の名前" value={name} onChange={({detail}) => setName(detail.value)} placeholder="自分の PC" /></FormField>
      <FormField label="公開鍵" description=".pub ファイルの内容を貼り付けてください。秘密鍵は入力しないでください。"><Textarea ariaLabel="公開鍵" value={publicKey} onChange={({detail}) => setPublicKey(detail.value)} placeholder="ssh-ed25519 ..." /></FormField>
      <Button formAction="none" disabled={!name.trim() || !publicKey.trim()} loading={register.isPending} onClick={() => register.mutate({name: name.trim(), public_key: publicKey.trim()})}>公開鍵を登録</Button>
      <Header variant="h3">GitHub から取り込む</Header>
      <FormField label="GitHub ユーザー名"><Input ariaLabel="GitHub ユーザー名" value={github} onChange={({detail}) => {setGithub(detail.value); setGithubResults(null);}} placeholder="username" /></FormField>
      <Button formAction="none" disabled={!github.trim()} loading={preview.isPending} onClick={() => preview.mutate(github.trim())}>公開鍵を検索</Button>
      {githubResults && githubResults.items.length === 0 && <Box>対応する公開鍵が見つかりませんでした。</Box>}
      {githubResults?.items.map((key) => <SpaceBetween key={key.fingerprint} direction="horizontal" size="s">
        <Box>{key.fingerprint}</Box>
        <Button formAction="none" loading={register.isPending} onClick={() => register.mutate({name: `${githubResults.username} · ${key.fingerprint.slice(-8)}`, public_key: key.public_key, github_username: githubResults.username})}>この鍵を登録</Button>
      </SpaceBetween>)}
      {(keys.data?.items ?? []).map((key) => <SpaceBetween key={key.id} direction="horizontal" size="s">
        <Box>{key.name} · {key.fingerprint}</Box>
        {key.user_id === userId && <Button formAction="none" loading={removeKey.isPending} onClick={() => removeKey.mutate(key.id)}>登録を削除</Button>}
      </SpaceBetween>)}</>}
    </SpaceBetween>
  </Container>;
}
