import Alert from "@cloudscape-design/components/alert";
import Badge from "@cloudscape-design/components/badge";
import Box from "@cloudscape-design/components/box";
import Button from "@cloudscape-design/components/button";
import FormField from "@cloudscape-design/components/form-field";
import Input from "@cloudscape-design/components/input";
import KeyValuePairs from "@cloudscape-design/components/key-value-pairs";
import Modal from "@cloudscape-design/components/modal";
import SpaceBetween from "@cloudscape-design/components/space-between";
import StatusIndicator from "@cloudscape-design/components/status-indicator";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { ColumnDef } from "@tanstack/react-table";
import { useMemo, useState } from "react";
import { DataTable } from "@/components/shared/data-table";
import { ErrorState } from "@/components/shared/error-state";
import { FormError } from "@/components/shared/form-error";
import { PageHeader } from "@/components/shared/page-header";
import { PageLoading } from "@/components/shared/page-loading";
import { useActiveOrganization } from "@/features/organizations/organization-context";
import { api, getApiErrorMessage } from "@/lib/api-client";
import type { InvitationResponse, Principal } from "@/lib/api-types";
import { iamPrincipalsQueryOptions } from "@/lib/queries";
import { formatDateTime } from "@/lib/utils";

export function IamPrincipalsPage() {
  const { activeOrganization } = useActiveOrganization();
  const organizationId = activeOrganization.organization_id;
  const principals = useQuery(iamPrincipalsQueryOptions(organizationId));
  const queryClient = useQueryClient();
  const [serviceAccountOpen, setServiceAccountOpen] = useState(false);
  const [invitationOpen, setInvitationOpen] = useState(false);
  const [serviceAccountName, setServiceAccountName] = useState("");
  const [expiresInHours, setExpiresInHours] = useState("24");
  const createServiceAccount = useMutation({
    mutationFn: (name: string) =>
      api.iam.principals.createServiceAccount(organizationId, { name }),
    onSuccess: async () => {
      setServiceAccountOpen(false);
      setServiceAccountName("");
      await queryClient.invalidateQueries({
        queryKey: ["organizations", organizationId, "iam", "principals"],
      });
    },
  });
  const createInvitation = useMutation({
    mutationFn: () =>
      api.invitations.create(organizationId, {
        expires_in_hours: Number(expiresInHours),
      }),
  });

  const columns = useMemo<ColumnDef<Principal, unknown>[]>(
    () => [
      {
        accessorKey: "name",
        header: "プリンシパル",
        cell: ({ row }) => (
          <SpaceBetween size="xxs">
            <Box fontWeight="bold">{row.original.name}</Box>
            <Box variant="code">{row.original.id}</Box>
          </SpaceBetween>
        ),
      },
      {
        accessorKey: "kind",
        header: "種別",
        cell: ({ getValue }) => {
          const kind = getValue<Principal["kind"]>();
          return <Badge color={kind === "user" ? "blue" : "grey"}>{kind === "user" ? "ユーザー" : "サービスアカウント"}</Badge>;
        },
      },
      {
        accessorKey: "enabled",
        header: "状態",
        cell: ({ getValue }) => (
          <StatusIndicator type={getValue<boolean>() ? "success" : "stopped"}>
            {getValue<boolean>() ? "有効" : "無効"}
          </StatusIndicator>
        ),
      },
      {
        accessorKey: "user_id",
        header: "ユーザーID",
        cell: ({ getValue }) => <Box variant="code">{getValue<string | null>() ?? "-"}</Box>,
      },
      {
        accessorKey: "created_at",
        header: "作成日時",
        cell: ({ getValue }) => formatDateTime(getValue<string>()),
      },
    ],
    [],
  );

  if (principals.isPending) return <PageLoading label="プリンシパルを読み込んでいます" />;
  if (principals.isError) {
    return (
      <ErrorState
        description="IAMプリンシパル一覧を取得できませんでした。"
        onRetry={() => void principals.refetch()}
      />
    );
  }

  return (
    <SpaceBetween size="l">
      <PageHeader
        title="IAMプリンシパル"
        description={`${activeOrganization.organization_name} のユーザーおよびサービスアカウントを管理します。`}
        actions={
          <SpaceBetween direction="horizontal" size="xs">
            {activeOrganization.role === "owner" ? (
              <Button
                iconName="key"
                onClick={() => {
                  createInvitation.reset();
                  setExpiresInHours("24");
                  setInvitationOpen(true);
                }}
              >
                招待コードを発行
              </Button>
            ) : null}
            <Button
              variant="primary"
              iconName="add-plus"
              onClick={() => {
                createServiceAccount.reset();
                setServiceAccountOpen(true);
              }}
            >
              サービスアカウントを作成
            </Button>
          </SpaceBetween>
        }
      />
      <DataTable
        columns={columns}
        data={principals.data.items}
        getRowId={(principal) => principal.id}
        searchPlaceholder="名前、種別、プリンシパルIDで検索"
        emptyTitle="プリンシパルがありません"
        emptyDescription="サービスアカウントを作成するか、ユーザーを招待してください。"
      />
      <Modal
        visible={serviceAccountOpen}
        onDismiss={() => setServiceAccountOpen(false)}
        header="サービスアカウントを作成"
        footer={
          <Box float="right">
            <SpaceBetween direction="horizontal" size="xs">
              <Button onClick={() => setServiceAccountOpen(false)}>キャンセル</Button>
              <Button
                variant="primary"
                loading={createServiceAccount.isPending}
                disabled={!serviceAccountName.trim()}
                onClick={() => createServiceAccount.mutate(serviceAccountName.trim())}
              >
                作成
              </Button>
            </SpaceBetween>
          </Box>
        }
      >
        <SpaceBetween size="l">
          <Box color="text-body-secondary">
            自動化処理へIAMポリシーを割り当てるプリンシパルを作成します。
          </Box>
          <FormField label="名前">
            <Input
              value={serviceAccountName}
              placeholder="flow-deployer"
              onChange={({ detail }) => setServiceAccountName(detail.value.slice(0, 120))}
            />
          </FormField>
          <FormError
            message={createServiceAccount.isError ? getApiErrorMessage(createServiceAccount.error) : null}
          />
        </SpaceBetween>
      </Modal>
      <InvitationModal
        visible={invitationOpen}
        onDismiss={() => setInvitationOpen(false)}
        expiresInHours={expiresInHours}
        setExpiresInHours={setExpiresInHours}
        invitation={createInvitation.data}
        pending={createInvitation.isPending}
        error={createInvitation.isError ? getApiErrorMessage(createInvitation.error) : null}
        onCreate={() => createInvitation.mutate()}
      />
    </SpaceBetween>
  );
}

function InvitationModal({
  visible,
  onDismiss,
  expiresInHours,
  setExpiresInHours,
  invitation,
  pending,
  error,
  onCreate,
}: {
  visible: boolean;
  onDismiss: () => void;
  expiresInHours: string;
  setExpiresInHours: (value: string) => void;
  invitation?: InvitationResponse;
  pending: boolean;
  error: string | null;
  onCreate: () => void;
}) {
  const [copied, setCopied] = useState<"code" | "url" | null>(null);
  const registrationUrl = invitation
    ? `${window.location.origin}/register#invitation_code=${encodeURIComponent(invitation.code)}`
    : "";
  const copy = async (value: string, kind: "code" | "url") => {
    await navigator.clipboard.writeText(value);
    setCopied(kind);
  };
  const hours = Number(expiresInHours);

  return (
    <Modal
      visible={visible}
      onDismiss={() => {
        setCopied(null);
        onDismiss();
      }}
      header="組織への招待"
      footer={
        <Box float="right">
          {invitation ? (
            <Button variant="primary" onClick={onDismiss}>完了</Button>
          ) : (
            <SpaceBetween direction="horizontal" size="xs">
              <Button onClick={onDismiss}>キャンセル</Button>
              <Button
                variant="primary"
                loading={pending}
                disabled={!Number.isInteger(hours) || hours < 1 || hours > 168}
                onClick={onCreate}
              >
                発行
              </Button>
            </SpaceBetween>
          )}
        </Box>
      }
    >
      {invitation ? (
        <SpaceBetween size="l">
          <Alert type="warning">このコードは閉じると再表示できません。</Alert>
          <FormField label="招待コード">
            <SpaceBetween direction="horizontal" size="xs">
              <Input readOnly value={invitation.code} />
              <Button
                iconName={copied === "code" ? "check" : "copy"}
                ariaLabel="招待コードをコピー"
                onClick={() => void copy(invitation.code, "code")}
              />
            </SpaceBetween>
          </FormField>
          <FormField label="登録URL">
            <SpaceBetween direction="horizontal" size="xs">
              <Input readOnly value={registrationUrl} />
              <Button
                iconName={copied === "url" ? "check" : "copy"}
                ariaLabel="登録URLをコピー"
                onClick={() => void copy(registrationUrl, "url")}
              />
            </SpaceBetween>
          </FormField>
          <KeyValuePairs
            columns={2}
            items={[
              { label: "最大利用回数", value: invitation.max_uses },
              { label: "有効期限", value: formatDateTime(invitation.expires_at) },
            ]}
          />
        </SpaceBetween>
      ) : (
        <SpaceBetween size="l">
          <Box color="text-body-secondary">
            1回だけ利用できる有効期限付き招待コードを発行します。
          </Box>
          <FormField label="有効時間" description="1〜168時間。登録完了後は直ちに無効になります。">
            <Input
              type="number"
              value={expiresInHours}
              onChange={({ detail }) => setExpiresInHours(detail.value)}
            />
          </FormField>
          <FormError message={error} />
        </SpaceBetween>
      )}
    </Modal>
  );
}
