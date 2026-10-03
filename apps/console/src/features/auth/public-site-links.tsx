import SpaceBetween from "@cloudscape-design/components/space-between";
import { PUBLIC_HOME_URL } from "@/lib/public-site";

export function PublicSiteLinks() {
  return (
    <nav aria-label="サービス案内・規約">
      <SpaceBetween direction="horizontal" size="m">
        <a href={PUBLIC_HOME_URL}>HeteroCloud について</a>
        <a href="/legal/terms/">利用規約</a>
        <a href="/legal/privacy/">プライバシー</a>
        <a href="/contact/">お問い合わせ</a>
      </SpaceBetween>
    </nav>
  );
}
