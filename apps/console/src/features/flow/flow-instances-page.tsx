import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { ColumnDef } from "@tanstack/react-table";
import { LoaderCircle, Plus, RadioTower, Workflow } from "lucide-react";
import { type FormEvent, useMemo, useState } from "react";
import { DataTable } from "@/components/shared/data-table";
import { ErrorState } from "@/components/shared/error-state";
import { FormError } from "@/components/shared/form-error";
import { PageHeader } from "@/components/shared/page-header";
import { PageLoading } from "@/components/shared/page-loading";
import { ProjectSelector } from "@/components/shared/resource-selectors";
import { StatusBadge } from "@/components/shared/status-badge";
import { Badge } from "@/components/ui/badge";
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { useActiveOrganization } from "@/features/organizations/organization-context";
import { api, getApiErrorMessage } from "@/lib/api-client";
import type { FlowInstance, TrafficMode } from "@/lib/api-types";
import {
  flowInstancesQueryOptions,
  projectsQueryOptions,
} from "@/lib/queries";
import { formatDateTime, formatNumber } from "@/lib/utils";

const trafficModeLabels: Record<TrafficMode, string> = {
  direct: "ダイレクト",
  forwarded: "転送",
};

export function FlowInstancesPage() {
  const { activeOrganization } = useActiveOrganization();
  const organizationId = activeOrganization.organization_id;
  const instances = useQuery(flowInstancesQueryOptions(organizationId));
  const projects = useQuery(projectsQueryOptions(organizationId));
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);
  const [projectId, setProjectId] = useState("");
  const [name, setName] = useState("");
  const [region, setRegion] = useState("heteronet-global");
  const [trafficMode, setTrafficMode] = useState<TrafficMode>("forwarded");
  const [maxParticipants, setMaxParticipants] = useState(100);
  const [turnEnabled, setTurnEnabled] = useState(true);

  const createInstance = useMutation({
    mutationFn: api.flow.instances.create.bind(api.flow.instances, organizationId),
    onSuccess: async () => {
      setOpen(false);
      setProjectId("");
      setName("");
      setRegion("heteronet-global");
      setTrafficMode("forwarded");
      setMaxParticipants(100);
      setTurnEnabled(true);
      await queryClient.invalidateQueries({
        queryKey: ["organizations", organizationId, "flow", "instances"],
      });
    },
  });

  const projectNames = useMemo(
    () =>
      new Map(
        (projects.data?.items ?? []).map((project) => [project.id, project.name]),
      ),
    [projects.data],
  );

  const columns = useMemo<ColumnDef<FlowInstance, unknown>[]>(
    () => [
      {
        accessorKey: "name",
        header: "インスタンス",
        cell: ({ row }) => (
          <div className="flex items-center gap-3">
            <span className="flex size-8 items-center justify-center rounded-[5px] bg-emerald-50 text-emerald-700">
              <Workflow className="size-4" />
            </span>
            <div>
              <div className="font-medium text-zinc-900">{row.original.name}</div>
              <div className="font-mono text-xs text-zinc-500">
                {row.original.id}
              </div>
            </div>
          </div>
        ),
      },
      {
        id: "project",
        accessorFn: (instance) =>
          projectNames.get(instance.project_id) ?? instance.project_id,
        header: "プロジェクト",
        cell: ({ row }) =>
          projectNames.get(row.original.project_id) ?? (
            <span className="font-mono text-xs">{row.original.project_id}</span>
          ),
      },
      {
        accessorKey: "state",
        header: "状態",
        cell: ({ getValue }) => (
          <StatusBadge status={getValue<FlowInstance["state"]>()} />
        ),
      },
      {
        id: "trafficMode",
        accessorFn: (instance) => instance.spec.traffic_mode,
        header: "通信モード",
        cell: ({ row }) => {
          const mode = row.original.spec.traffic_mode;
          return (
            <Badge variant={mode === "direct" ? "warning" : "info"}>
              {trafficModeLabels[mode]}
            </Badge>
          );
        },
      },
      {
        id: "region",
        accessorFn: (instance) => instance.spec.region,
        header: "リージョン",
      },
      {
        id: "maxParticipants",
        accessorFn: (instance) => instance.spec.max_participants,
        header: "参加者上限",
        cell: ({ row }) => formatNumber(row.original.spec.max_participants),
      },
      {
        id: "turn",
        accessorFn: (instance) => String(instance.spec.turn_enabled),
        header: "TURN",
        cell: ({ row }) => (
          <Badge variant={row.original.spec.turn_enabled ? "success" : "neutral"}>
            {row.original.spec.turn_enabled ? "有効" : "無効"}
          </Badge>
        ),
      },
      {
        accessorKey: "updated_at",
        header: "更新日時",
        cell: ({ getValue }) => formatDateTime(getValue<string>()),
      },
    ],
    [projectNames],
  );

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    createInstance.mutate({
      project_id: projectId,
      name: name.trim(),
      spec: {
        region,
        traffic_mode: trafficMode,
        max_participants: maxParticipants,
        turn_enabled: turnEnabled,
        metadata: {},
      },
    });
  };

  if (instances.isPending || projects.isPending) {
    return <PageLoading label="Flowインスタンスを読み込んでいます" />;
  }

  if (instances.isError || projects.isError) {
    return (
      <ErrorState
        description="Flowインスタンスまたはプロジェクト一覧を取得できませんでした。"
        onRetry={() => {
          void instances.refetch();
          void projects.refetch();
        }}
      />
    );
  }

  return (
    <div className="space-y-6">
      <PageHeader
        title="Flowインスタンス"
        description={`${activeOrganization.organization_name} のLiveKit、WebRTC、STUN、TURN、マッチング基盤を管理します。`}
        actions={
          <Dialog
            open={open}
            onOpenChange={(nextOpen) => {
              setOpen(nextOpen);
              if (nextOpen) createInstance.reset();
            }}
          >
            <DialogTrigger asChild>
              <Button>
                <Plus />
                インスタンスを作成
              </Button>
            </DialogTrigger>
            <DialogContent className="max-w-xl">
              <DialogHeader>
                <DialogTitle>Flowインスタンスを作成</DialogTitle>
                <DialogDescription>
                  配置先プロジェクトとHeteroNetworkの通信モードを指定します。
                </DialogDescription>
              </DialogHeader>
              <form onSubmit={submit} className="space-y-5">
                <div className="space-y-2">
                  <Label>プロジェクト</Label>
                  <ProjectSelector
                    value={projectId}
                    onValueChange={setProjectId}
                    disabled={createInstance.isPending}
                  />
                </div>
                <div className="space-y-2">
                  <Label htmlFor="flow-name">インスタンス名</Label>
                  <Input
                    id="flow-name"
                    required
                    maxLength={120}
                    value={name}
                    onChange={(event) => setName(event.target.value)}
                    placeholder="realtime-tokyo"
                  />
                </div>
                <div className="grid gap-4 sm:grid-cols-2">
                  <div className="space-y-2">
                    <Label>リージョン</Label>
                    <Select value={region} onValueChange={setRegion}>
                      <SelectTrigger>
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="heteronet-global">
                          HeteroNet Global
                        </SelectItem>
                        <SelectItem value="heteronet-jp">HeteroNet Japan</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                  <div className="space-y-2">
                    <Label htmlFor="max-participants">参加者上限</Label>
                    <Input
                      id="max-participants"
                      type="number"
                      required
                      min={1}
                      max={100000}
                      value={maxParticipants}
                      onChange={(event) =>
                        setMaxParticipants(event.currentTarget.valueAsNumber || 1)
                      }
                    />
                  </div>
                </div>
                <fieldset className="space-y-2">
                  <legend className="text-sm font-medium text-zinc-800">
                    通信モード
                  </legend>
                  <div className="grid gap-3 sm:grid-cols-2">
                    {(
                      [
                        {
                          value: "direct",
                          label: "ダイレクト",
                          description:
                            "公開IP所有ノードへ配置し、転送を避けます。",
                        },
                        {
                          value: "forwarded",
                          label: "転送",
                          description:
                            "公開ノードから内部Podへ転送し、配置先を広げます。",
                        },
                      ] as const
                    ).map((mode) => (
                      <label
                        key={mode.value}
                        className={`cursor-pointer border p-3 ${
                          trafficMode === mode.value
                            ? "border-emerald-600 bg-emerald-50"
                            : "border-zinc-200 hover:bg-zinc-50"
                        }`}
                      >
                        <span className="flex items-center gap-2">
                          <input
                            type="radio"
                            name="traffic-mode"
                            value={mode.value}
                            checked={trafficMode === mode.value}
                            onChange={() => setTrafficMode(mode.value)}
                            className="size-4 accent-emerald-700"
                          />
                          <RadioTower className="size-4 text-zinc-600" />
                          <span className="text-sm font-medium">{mode.label}</span>
                        </span>
                        <span className="mt-2 block text-xs leading-5 text-zinc-600">
                          {mode.description}
                        </span>
                      </label>
                    ))}
                  </div>
                </fieldset>
                <div className="flex items-center justify-between gap-6 border-t border-zinc-100 pt-4">
                  <div>
                    <Label htmlFor="turn-enabled">TURNを有効化</Label>
                    <p className="mt-1 text-xs text-zinc-500">
                      直接接続できないクライアントを中継します。
                    </p>
                  </div>
                  <Switch
                    id="turn-enabled"
                    checked={turnEnabled}
                    onCheckedChange={setTurnEnabled}
                  />
                </div>
                <FormError
                  message={
                    createInstance.isError
                      ? getApiErrorMessage(createInstance.error)
                      : null
                  }
                />
                <DialogFooter>
                  <DialogClose asChild>
                    <Button type="button" variant="secondary">
                      キャンセル
                    </Button>
                  </DialogClose>
                  <Button
                    type="submit"
                    disabled={createInstance.isPending || !projectId}
                  >
                    {createInstance.isPending ? (
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
        data={instances.data.items}
        getRowId={(instance) => instance.id}
        searchPlaceholder="名前、プロジェクト、リージョン、状態で検索"
        emptyTitle="Flowインスタンスがありません"
        emptyDescription="プロジェクトを選択して最初のFlowインスタンスを作成してください。"
      />
    </div>
  );
}
