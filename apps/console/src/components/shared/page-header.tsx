import Header from "@cloudscape-design/components/header";
import type { ReactNode } from "react";

interface PageHeaderProps {
  title: string;
  description: string;
  actions?: ReactNode;
}

export function PageHeader({ title, description, actions }: PageHeaderProps) {
  return (
    <Header variant="h1" description={description} actions={actions}>
      {title}
    </Header>
  );
}
