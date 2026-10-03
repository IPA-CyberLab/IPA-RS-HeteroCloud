import SpaceBetween from "@cloudscape-design/components/space-between";

export function PublicSiteLinks() {
  return (
    <nav aria-label="サービス案内・規約">
      <SpaceBetween direction="horizontal" size="m">
        <a href="/">HeteroCloud について</a>
        <a href="/legal/terms/">利用規約</a>
        <a href="/legal/privacy/">プライバシー</a>
        <a href="/contact/">お問い合わせ</a>
      </SpaceBetween>
    </nav>
  );
}
