import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { ColumnDef } from "@tanstack/react-table";
import { Boxes, LoaderCircle, Plus } from "lucide-react";
import { type FormEvent, useMemo, useState } from "react";
import { DataTable } from "@/components/shared/data-table";
import { ErrorState } from "@/components/shared/error-state";
import { FormError } from "@/components/shared/form-error";
import { PageHeader } from "@/components/shared/page-header";
import { PageLoading } from "@/components/shared/page-loading";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useActiveOrganization } from "@/features/organizations/organization-context";
import { api, getApiErrorMessage } from "@/lib/api-client";
import type { Project } from "@/lib/api-types";
import { projectsQueryOptions } from "@/lib/queries";
import { formatDateTime } from "@/lib/utils";

export function ProjectsPage() {
  const { activeOrganization } = useActiveOrganization();
  const organizationId = activeOrganization.organization_id;
  const projects = useQuery(projectsQueryOptions(organizationId));
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [slug, setSlug] = useState("");

  const createProject = useMutation({
    mutationFn: (input: { name: string; slug: string }) =>
      api.projects.create(organizationId, input),
    onSuccess: async () => {
      setOpen(false);
      setName("");
      setSlug("");
      await queryClient.invalidateQueries({
        queryKey: ["organizations", organizationId, "projects"],
      });
    },
  });

  const columns = useMemo<ColumnDef<Project, unknown>[]>(
    () => [
      {
        accessorKey: "name",
        header: "プロジェクト",
        cell: ({ row }) => (
          <div className="flex items-center gap-3">
            <span className="flex size-8 items-center justify-center rounded-[5px] bg-zinc-100 text-zinc-600">
              <Boxes className="size-4" />
            </span>
            <div>
              <div className="font-medium text-zinc-900">{row.original.name}</div>
              <div className="text-xs text-zinc-500">{row.original.slug}</div>
            </div>
          </div>
        ),
      },
      {
        accessorKey: "id",
        header: "プロジェクトID",
        cell: ({ getValue }) => (
          <span className="font-mono text-xs">{getValue<string>()}</span>
        ),
      },
      {
        accessorKey: "created_at",
        header: "作成日時",
        cell: ({ getValue }) => formatDateTime(getValue<string>()),
      },
    ],
    [],
  );

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    createProject.mutate({ name: name.trim(), slug: slug.trim() });
  };

  if (projects.isPending) {
    return <PageLoading label="プロジェクトを読み込んでいます" />;
  }

  if (projects.isError) {
    return (
      <ErrorState
        description="選択中の組織からプロジェクト一覧を取得できませんでした。"
        onRetry={() => void projects.refetch()}
      />
    );
  }

  return (
    <div className="space-y-6">
      <PageHeader
        title="プロジェクト"
        description={`${activeOrganization.organization_name} のサービスリソース境界を管理します。`}
        actions={
          <Dialog
            open={open}
            onOpenChange={(nextOpen) => {
              setOpen(nextOpen);
              if (nextOpen) createProject.reset();
            }}
          >
            <DialogTrigger asChild>
              <Button>
                <Plus />
                プロジェクトを作成
              </Button>
            </DialogTrigger>
            <DialogContent>
              <DialogHeader>
                <DialogTitle>プロジェクトを作成</DialogTitle>
                <DialogDescription>
                  {activeOrganization.organization_name} に新しいプロジェクトを作成します。
                </DialogDescription>
              </DialogHeader>
              <form onSubmit={submit} className="space-y-5">
                <div className="space-y-2">
                  <Label htmlFor="project-name">プロジェクト名</Label>
                  <Input
                    id="project-name"
                    required
                    maxLength={120}
                    value={name}
                    onChange={(event) => setName(event.target.value)}
                    placeholder="Realtime Production"
                  />
                </div>
                <div className="space-y-2">
                  <Label htmlFor="project-slug">プロジェクトslug</Label>
                  <Input
                    id="project-slug"
                    required
                    pattern="[a-z][a-z0-9-]{1,61}[a-z0-9]"
                    maxLength={63}
                    value={slug}
                    onChange={(event) =>
                      setSlug(event.target.value.toLowerCase().replace(/\s+/g, "-"))
                    }
                    placeholder="realtime-prod"
                  />
                  <p className="text-xs text-zinc-500">
                    3〜63文字の英小文字、数字、ハイフンを使用します。
                  </p>
                </div>
                <FormError
                  message={
                    createProject.isError
                      ? getApiErrorMessage(createProject.error)
                      : null
                  }
                />
                <DialogFooter>
                  <DialogClose asChild>
                    <Button type="button" variant="secondary">
                      キャンセル
                    </Button>
                  </DialogClose>
                  <Button type="submit" disabled={createProject.isPending}>
                    {createProject.isPending ? (
                      <>
                        <LoaderCircle className="animate-spin" />
                        作成中
                      </>
                    ) : (
                      "作成"
                    )}
                  </Button>
                </DialogFooter>
              </form>
            </DialogContent>
          </Dialog>
        }
      />

      <DataTable
        columns={columns}
        data={projects.data.items}
        getRowId={(project) => project.id}
        searchPlaceholder="プロジェクト名、slug、IDで検索"
        emptyTitle="プロジェクトがありません"
        emptyDescription="最初のプロジェクトを作成してください。"
      />
    </div>
  );
}
