import Box from "@cloudscape-design/components/box";
import SpaceBetween from "@cloudscape-design/components/space-between";
import type { ReactNode } from "react";

interface EmptyStateProps {
  title: string;
  description: string;
  action?: ReactNode;
}

export function EmptyState({ title, description, action }: EmptyStateProps) {
  return (
    <Box margin={{ vertical: "xxl" }} textAlign="center" color="inherit">
      <SpaceBetween size="m">
        <div>
          <Box variant="strong">{title}</Box>
          <Box variant="p" color="text-body-secondary">
            {description}
          </Box>
        </div>
        {action}
      </SpaceBetween>
    </Box>
  );
}
