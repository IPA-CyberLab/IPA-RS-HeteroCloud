import { Box } from "@/components/ui";
import { SpaceBetween } from "@/components/ui";
import { Spinner } from "@/components/ui";

interface PageLoadingProps {
  label?: string;
  fullScreen?: boolean;
}

export function PageLoading({
  label = "読み込んでいます",
  fullScreen = false,
}: PageLoadingProps) {
  return (
    <div className={fullScreen ? "page-loading page-loading--full" : "page-loading"}>
      <Box padding="xxl" textAlign="center" color="text-body-secondary">
        <SpaceBetween direction="horizontal" size="xs" alignItems="center">
          <Spinner />
          <span role="status">{label}</span>
        </SpaceBetween>
      </Box>
    </div>
  );
}
