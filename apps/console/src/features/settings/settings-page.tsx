import Box from "@cloudscape-design/components/box";
import ColumnLayout from "@cloudscape-design/components/column-layout";
import Container from "@cloudscape-design/components/container";
import FormField from "@cloudscape-design/components/form-field";
import Header from "@cloudscape-design/components/header";
import KeyValuePairs from "@cloudscape-design/components/key-value-pairs";
import Select from "@cloudscape-design/components/select";
import SpaceBetween from "@cloudscape-design/components/space-between";
import StatusIndicator from "@cloudscape-design/components/status-indicator";
import { DataTable } from "@/components/shared/data-table";
import { PageHeader } from "@/components/shared/page-header";
import { useSession } from "@/features/auth/session";
import { useActiveOrganization } from "@/features/organizations/organization-context";
import type { Membership } from "@/lib/api-types";
import { formatDateTime } from "@/lib/utils";

export function SettingsPage() {
  const session = useSession().data!;
  const { activeOrganization, memberships, setActiveOrganizationId } =
    useActiveOrganization();

  return (
    <SpaceBetween size="l">
      <PageHeader
        title="設定"
        description="アカウント、セッション、コンソールの操作対象を確認します。"
      />
      <ColumnLayout columns={2}>
        <Container header={<Header variant="h2">アカウント</Header>}>
          <KeyValuePairs
            columns={1}
            items={[
              { label: "表示名", value: session.user.display_name },
              { label: "メールアドレス", value: session.user.email },
              { label: "ユーザーID", value: <Box variant="code">{session.user.id}</Box> },
              {
                label: "状態",
                value: (
                  <StatusIndicator type={session.user.status === "active" ? "success" : "warning"}>
                    {session.user.status === "active" ? "有効" : "停止中"}
                  </StatusIndicator>
                ),
              },
              { label: "登録日時", value: formatDateTime(session.user.created_at) },
            ]}
          />
        </Container>
        <Container header={<Header variant="h2">セッション保護</Header>}>
          <KeyValuePairs
            columns={1}
            items={[
              {
                label: "HttpOnly Cookie",
                value: (
                  <StatusIndicator type="success">
                    ブラウザスクリプトから参照できないCookieで保持
                  </StatusIndicator>
                ),
              },
              {
                label: "Origin + CSRF検証",
                value: (
                  <StatusIndicator type="success">
                    同一originとセッショントークン由来CSRFで保護
                  </StatusIndicator>
                ),
              },
            ]}
          />
        </Container>
      </ColumnLayout>
      <Container
        header={
          <Header variant="h2" description="組織スコープAPIに使用する操作対象です。">
            操作対象の組織
          </Header>
        }
      >
        <FormField label="現在の組織">
          <Select
            selectedOption={{
              value: activeOrganization.organization_id,
              label: activeOrganization.organization_name,
              description: activeOrganization.organization_slug,
            }}
            options={memberships.map((membership) => ({
              value: membership.organization_id,
              label: membership.organization_name,
              description: membership.organization_slug,
            }))}
            onChange={({ detail }) => {
              if (detail.selectedOption.value) {
                setActiveOrganizationId(detail.selectedOption.value);
              }
            }}
          />
        </FormField>
      </Container>
      <DataTable<Membership>
        columns={[
          { accessorKey: "organization_name", header: "組織" },
          {
            accessorKey: "organization_id",
            header: "組織ID",
            cell: ({ getValue }) => <Box variant="code">{getValue<string>()}</Box>,
          },
          { accessorKey: "role", header: "ロール" },
          {
            accessorKey: "principal_id",
            header: "プリンシパルID",
            cell: ({ getValue }) => <Box variant="code">{getValue<string>()}</Box>,
          },
        ]}
        data={memberships}
        getRowId={(membership) => membership.organization_id}
        searchPlaceholder="組織名、ロール、プリンシパルIDで検索"
        emptyTitle="メンバーシップがありません"
        emptyDescription="参加可能な組織がありません。"
      />
    </SpaceBetween>
  );
}
