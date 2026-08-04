import Alert from "@cloudscape-design/components/alert";
import Box from "@cloudscape-design/components/box";
import Button from "@cloudscape-design/components/button";

export function NotFoundPage() {
  return (
    <main className="auth-page">
      <div className="auth-panel">
        <Alert
          type="error"
          header="ページが見つかりません"
          action={<Button href="/overview" iconName="arrow-left">概要へ戻る</Button>}
        >
          URLを確認するか、コンソールの概要へ戻ってください。
        </Alert>
        <Box textAlign="center" color="text-body-secondary" padding={{ top: "l" }}>
          HTTP 404
        </Box>
      </div>
    </main>
  );
}
