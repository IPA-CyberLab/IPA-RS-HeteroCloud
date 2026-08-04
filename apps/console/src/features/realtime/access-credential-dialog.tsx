import Alert from "@cloudscape-design/components/alert";
import Box from "@cloudscape-design/components/box";
import Button from "@cloudscape-design/components/button";
import FormField from "@cloudscape-design/components/form-field";
import Header from "@cloudscape-design/components/header";
import KeyValuePairs from "@cloudscape-design/components/key-value-pairs";
import Modal from "@cloudscape-design/components/modal";
import Multiselect from "@cloudscape-design/components/multiselect";
import Select from "@cloudscape-design/components/select";
import SpaceBetween from "@cloudscape-design/components/space-between";
import Table from "@cloudscape-design/components/table";
import { useMutation } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { FormError } from "@/components/shared/form-error";
import { api, getApiErrorMessage } from "@/lib/api-client";
import type { RealtimeAccessCredential } from "@/lib/api-types";
import { RealtimeEndpoints } from "./realtime-endpoints";
import { formatCredentialDate, normalizeEndpoints, realtimePermissions } from "./realtime-service-utils";

const expiryOptions = [
  { value: "60", label: "1分" },
  { value: "180", label: "3分" },
  { value: "300", label: "5分" },
];

export function AccessCredentialDialog({
  organizationId,
  serviceId,
  serviceName,
  disabled,
}: {
  organizationId: string;
  serviceId: string;
  serviceName: string;
  disabled?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const [expiresInSeconds, setExpiresInSeconds] = useState(300);
  const [permissions, setPermissions] = useState<string[]>(
    realtimePermissions.map((permission) => permission.value),
  );
  const [credential, setCredential] = useState<RealtimeAccessCredential | null>(null);
  const [copied, setCopied] = useState<string | null>(null);
  const issue = useMutation({
    mutationFn: () =>
      api.realtime.services.issueAccessCredential(organizationId, serviceId, {
        expires_in_seconds: expiresInSeconds,
        permissions,
      }),
    onSuccess: setCredential,
  });
  const headerEntries = useMemo(() => Object.entries(credential?.headers ?? {}), [credential]);
  const close = () => {
    setOpen(false);
    setCredential(null);
    setCopied(null);
    issue.reset();
  };
  const copy = async (key: string, value: string) => {
    await navigator.clipboard.writeText(value);
    setCopied(key);
    window.setTimeout(() => setCopied(null), 1_500);
  };

  return (
    <>
      <Button iconName="key" disabled={disabled} onClick={() => setOpen(true)}>
        テスト用短期アクセス
      </Button>
      <Modal
        visible={open}
        onDismiss={close}
        size="large"
        header="短期アクセスを手動発行"
        footer={
          <Box float="right">
            {credential ? (
              <Button variant="primary" onClick={close}>閉じる</Button>
            ) : (
              <SpaceBetween direction="horizontal" size="xs">
                <Button onClick={close}>キャンセル</Button>
                <Button
                  variant="primary"
                  iconName="key"
                  loading={issue.isPending}
                  disabled={permissions.length === 0}
                  onClick={() => issue.mutate()}
                >
                  発行
                </Button>
              </SpaceBetween>
            )}
          </Box>
        }
      >
        {credential ? (
          <SpaceBetween size="l">
            <Alert type="warning">
              この秘密値は今回だけ表示されます。閉じると再表示できません。
            </Alert>
            <KeyValuePairs
              columns={3}
              items={[
                { label: "発行日時", value: formatCredentialDate(credential.issued_at) },
                { label: "有効期限", value: formatCredentialDate(credential.expires_at) },
                {
                  label: "IPレート制限",
                  value: `${credential.rate_limit.requests_per_second} RPS / burst ${credential.rate_limit.burst}`,
                },
              ]}
            />
            <Table
              variant="embedded"
              header={
                <Header
                  variant="h3"
                  actions={
                    <Button
                      iconName={copied === "all" ? "check" : "copy"}
                      onClick={() =>
                        void copy(
                          "all",
                          headerEntries.map(([key, value]) => `${key}: ${value}`).join("\n"),
                        )
                      }
                    >
                      すべてコピー
                    </Button>
                  }
                >
                  リクエストヘッダー
                </Header>
              }
              items={headerEntries.map(([key, value]) => ({ key, value }))}
              trackBy="key"
              columnDefinitions={[
                { id: "key", header: "ヘッダー", cell: (item) => <Box variant="code">{item.key}</Box> },
                { id: "value", header: "値", cell: (item) => <Box variant="code">{item.value}</Box> },
                {
                  id: "copy",
                  header: "",
                  width: 64,
                  cell: (item) => (
                    <Button
                      variant="inline-icon"
                      iconName={copied === item.key ? "check" : "copy"}
                      ariaLabel={`${item.key}をコピー`}
                      onClick={() => void copy(item.key, item.value)}
                    />
                  ),
                },
              ]}
            />
            <div>
              <Header variant="h3">接続先</Header>
              <RealtimeEndpoints endpoints={normalizeEndpoints(credential.endpoints)} />
            </div>
          </SpaceBetween>
        ) : (
          <SpaceBetween size="l">
            <Box color="text-body-secondary">
              {serviceName} の動作確認用です。開発者連携には開発者認証情報を使用します。
            </Box>
            <FormField label="有効期間">
              <Select
                ariaLabel="有効期間"
                selectedOption={expiryOptions.find((option) => option.value === String(expiresInSeconds)) ?? expiryOptions[2]}
                options={expiryOptions}
                disabled={issue.isPending}
                onChange={({ detail }) => setExpiresInSeconds(Number(detail.selectedOption.value))}
              />
            </FormField>
            <FormField label="権限">
              <Multiselect
                selectedOptions={realtimePermissions.filter((option) => permissions.includes(option.value))}
                options={realtimePermissions}
                disabled={issue.isPending}
                placeholder="権限を選択"
                tokenLimit={realtimePermissions.length}
                onChange={({ detail }) =>
                  setPermissions(detail.selectedOptions.flatMap((option) => option.value ? [option.value] : []))
                }
              />
            </FormField>
            <FormError message={issue.isError ? getApiErrorMessage(issue.error) : null} />
          </SpaceBetween>
        )}
      </Modal>
    </>
  );
}
