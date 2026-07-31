import { useQuery } from "@tanstack/react-query";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useActiveOrganization } from "@/features/organizations/organization-context";
import { projectsQueryOptions } from "@/lib/queries";

interface ProjectSelectorProps {
  value?: string;
  onValueChange: (value: string) => void;
  disabled?: boolean;
}

export function ProjectSelector({
  value,
  onValueChange,
  disabled,
}: ProjectSelectorProps) {
  const { activeOrganization } = useActiveOrganization();
  const projects = useQuery(projectsQueryOptions(activeOrganization.organization_id));

  return (
    <div className="space-y-1">
      <Select
        value={value}
        onValueChange={onValueChange}
        disabled={disabled || projects.isPending || projects.isError}
      >
        <SelectTrigger aria-label="プロジェクト">
          <SelectValue
            placeholder={
              projects.isPending
                ? "プロジェクトを読み込み中"
                : projects.isError
                  ? "プロジェクトを取得できません"
                  : "プロジェクトを選択"
            }
          />
        </SelectTrigger>
        <SelectContent>
          {projects.data?.items.map((project) => (
            <SelectItem key={project.id} value={project.id}>
              {project.name}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      {projects.isError ? (
        <p className="text-xs text-red-700">
          プロジェクト一覧を取得できないため選択できません。
        </p>
      ) : null}
    </div>
  );
}
