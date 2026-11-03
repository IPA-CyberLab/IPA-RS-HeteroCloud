import { Box } from "@/components/ui";
import { Container } from "@/components/ui";
import { FormField } from "@/components/ui";
import { Header } from "@/components/ui";
import { KeyValuePairs } from "@/components/ui";
import { Select } from "@/components/ui";
import { SpaceBetween } from "@/components/ui";
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
        description="アカウント情報と操作対象の組織を確認します。"
      />
      <Container header={<Header variant="h2">アカウント</Header>}>
        <KeyValuePairs
          columns={1}
          items={[
            { label: "表示名", value: session.user.display_name },
            { label: "メールアドレス", value: session.user.email },
            { label: "ユーザーID", value: <Box variant="code">{session.user.id}</Box> },
            { label: "登録日時", value: formatDateTime(session.user.created_at) },
          ]}
        />
      </Container>
      <Container
        header={
          <Header variant="h2" description="操作する組織を選択します。">
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
