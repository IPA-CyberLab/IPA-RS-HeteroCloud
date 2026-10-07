import Alert from "@cloudscape-design/components/alert";
import Box from "@cloudscape-design/components/box";
import Button from "@cloudscape-design/components/button";
import Container from "@cloudscape-design/components/container";
import FormField from "@cloudscape-design/components/form-field";
import Header from "@cloudscape-design/components/header";
import Input from "@cloudscape-design/components/input";
import Link from "@cloudscape-design/components/link";
import SpaceBetween from "@cloudscape-design/components/space-between";
import StatusIndicator from "@cloudscape-design/components/status-indicator";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { api, getApiErrorMessage } from "@/lib/api-client";
import type { FlashDomain } from "@/lib/api-types";

const labels: Record<FlashDomain["phase"], string> = {
  queued: "登録中", pending_dns: "DNS設定待ち", pending_certificate: "証明書を準備中",
  pending_gateway: "入口へ反映中", routing: "接続先へ反映中", ready: "利用可能",
  deleting: "削除中", error: "設定エラー",
};

export function FlashDomainsPanel({ organizationId, serviceId, editable = false, oidc = false }: {
  organizationId: string; serviceId: string; editable?: boolean; oidc?: boolean;
}) {
  const client = useQueryClient();
  const key = ["flash", organizationId, serviceId, "domains"];
  const query = useQuery({ queryKey: key, queryFn: ({ signal }) => api.flash.services.listDomains(organizationId, serviceId, signal), refetchInterval: 5000 });
  const [hostname, setHostname] = useState("");
  const [selected, setSelected] = useState<string | null>(null);
  const add = useMutation({ mutationFn: () => api.flash.services.addDomain(organizationId, serviceId, hostname.trim().toLowerCase().replace(/\.$/, "")),
    onSuccess: () => { setHostname(""); void client.invalidateQueries({ queryKey: key }); } });
  const remove = useMutation({ mutationFn: (id: string) => api.flash.services.deleteDomain(organizationId, serviceId, id),
    onSuccess: () => { setSelected(null); void client.invalidateQueries({ queryKey: key }); } });
  const error = add.error || remove.error || query.error;
  return <Container header={<Header variant="h2">独自ドメイン</Header>}>
    <SpaceBetween size="m">
      {editable && <>
        <Box>ドメインを登録し、表示される接続先へCNAMEを設定してください。DNSプロキシはオフにしてください。ルートドメインでは、表示されるTXTとALIAS／ANAMEなどを設定できます。</Box>
        <FormField label="独自ドメイン" description="例: app.example.com。HTTPS証明書は自動で準備・更新します。">
          <Input ariaLabel="独自ドメイン" value={hostname} onChange={({ detail }) => setHostname(detail.value)} placeholder="app.example.com" />
        </FormField>
        <Button formAction="none" loading={add.isPending} disabled={!hostname.trim() || (query.data?.items.length ?? 0) >= 8} onClick={() => add.mutate()}>ドメインを登録</Button>
      </>}
      {error && <Alert type="error">{getApiErrorMessage(error)}</Alert>}
      {query.data?.provider_unavailable && <Alert type="warning">ドメインの接続状況を取得できません。設定は保持されています。</Alert>}
      {(query.data?.items ?? []).map(domain => <Container key={domain.id} header={<Header variant="h3">{domain.hostname}</Header>}>
        <SpaceBetween size="s">
          <StatusIndicator type={domain.phase === "ready" ? "success" : domain.phase === "error" ? "error" : "pending"}>{labels[domain.phase]}</StatusIndicator>
          <Box>CNAMEの接続先: <code>{domain.cname_target ?? "確認中"}</code></Box>
          <Box>ルートドメインの確認用TXT: <code>{domain.verification.name}</code> → <code>{domain.verification.value}</code></Box>
          {oidc && <Box>認証プロバイダーに登録するコールバックURL: <code>{domain.oidc_callback_url}</code></Box>}
          {domain.phase === "ready" && <Link href={`https://${domain.hostname}`} external>https://{domain.hostname}</Link>}
          {editable && domain.phase !== "deleting" && (selected === domain.id ? <SpaceBetween direction="horizontal" size="s">
            <Button formAction="none" loading={remove.isPending} onClick={() => remove.mutate(domain.id)}>このドメインを削除</Button>
            <Button formAction="none" onClick={() => setSelected(null)}>キャンセル</Button>
          </SpaceBetween> : <Button formAction="none" onClick={() => setSelected(domain.id)}>削除</Button>)}
        </SpaceBetween>
      </Container>)}
      {!query.isPending && !(query.data?.items.length) && <Box color="text-status-inactive">独自ドメインは登録されていません。</Box>}
    </SpaceBetween>
  </Container>;
}
