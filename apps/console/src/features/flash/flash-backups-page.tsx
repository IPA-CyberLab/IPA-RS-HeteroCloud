import { PageHeader } from "@/components/shared/page-header";
import { useActiveOrganization } from "@/features/organizations/organization-context";
import SpaceBetween from "@cloudscape-design/components/space-between";
import { FlashBackupsPanel } from "./flash-backups-panel";

export function FlashBackupsPage() {
  const { activeOrganization } = useActiveOrganization();
  return <SpaceBetween size="l"><PageHeader title="Flashバックアップ" description="保存元サービスを削除した後も、バックアップから新しいサービスを復元できます。" />
    <FlashBackupsPanel key={activeOrganization.organization_id} organizationId={activeOrganization.organization_id} />
  </SpaceBetween>;
}
