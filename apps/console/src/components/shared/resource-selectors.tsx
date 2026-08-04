import Select from "@cloudscape-design/components/select";
import { useQuery } from "@tanstack/react-query";
import { useActiveOrganization } from "@/features/organizations/organization-context";
import { projectsQueryOptions } from "@/lib/queries";

export function ProjectSelector({
  value,
  onValueChange,
  disabled,
}: {
  value?: string;
  onValueChange: (value: string) => void;
  disabled?: boolean;
}) {
  const { activeOrganization } = useActiveOrganization();
  const projects = useQuery(projectsQueryOptions(activeOrganization.organization_id));
  const options = (projects.data?.items ?? []).map((project) => ({
    value: project.id,
    label: project.name,
    description: project.slug,
  }));
  return (
    <Select
      ariaLabel="プロジェクト"
      selectedOption={options.find((option) => option.value === value) ?? null}
      options={options}
      disabled={disabled || projects.isPending || projects.isError}
      statusType={projects.isPending ? "loading" : projects.isError ? "error" : "finished"}
      loadingText="プロジェクトを読み込んでいます"
      errorText="プロジェクトを取得できません"
      recoveryText="再試行"
      placeholder="プロジェクトを選択"
      empty="プロジェクトがありません"
      onLoadItems={() => projects.isError && void projects.refetch()}
      onChange={({ detail }) => onValueChange(detail.selectedOption.value ?? "")}
    />
  );
}
