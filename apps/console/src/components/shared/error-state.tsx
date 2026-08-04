import Alert from "@cloudscape-design/components/alert";
import Button from "@cloudscape-design/components/button";

interface ErrorStateProps {
  title?: string;
  description: string;
  onRetry?: () => void;
}

export function ErrorState({
  title = "データを取得できませんでした",
  description,
  onRetry,
}: ErrorStateProps) {
  return (
    <Alert
      type="error"
      header={title}
      action={
        onRetry ? (
          <Button iconName="refresh" onClick={onRetry}>
            再試行
          </Button>
        ) : undefined
      }
    >
      {description}
    </Alert>
  );
}
