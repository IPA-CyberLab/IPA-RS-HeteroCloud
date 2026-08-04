import Box from "@cloudscape-design/components/box";
import SpaceBetween from "@cloudscape-design/components/space-between";
import Spinner from "@cloudscape-design/components/spinner";

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
