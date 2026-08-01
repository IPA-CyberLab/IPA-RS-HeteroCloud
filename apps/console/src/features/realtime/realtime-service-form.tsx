import { RadioTower } from "lucide-react";
import type { FormEvent } from "react";
import { ProjectSelector } from "@/components/shared/resource-selectors";
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
import type { TrafficMode } from "@/lib/api-types";

export interface RealtimeServiceFormValue {
  projectId: string;
  name: string;
  region: string;
  trafficMode: TrafficMode;
  maxParticipants: number;
  maxRooms: number;
  turnEnabled: boolean;
}

interface RealtimeServiceFormProps {
  value: RealtimeServiceFormValue;
  onChange: (value: RealtimeServiceFormValue) => void;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
  disabled?: boolean;
  projectLocked?: boolean;
  children: React.ReactNode;
}

function positiveInteger(value: number): number {
  return Number.isFinite(value) ? Math.max(1, Math.trunc(value)) : 1;
}

export const defaultRealtimeServiceFormValue: RealtimeServiceFormValue = {
  projectId: "",
  name: "",
  region: "heteronet-global",
  trafficMode: "forwarded",
  maxParticipants: 100,
  maxRooms: 100,
  turnEnabled: true,
};

export function RealtimeServiceForm({
  value,
  onChange,
  onSubmit,
  disabled,
  projectLocked,
  children,
}: RealtimeServiceFormProps) {
  const update = <Key extends keyof RealtimeServiceFormValue>(
    key: Key,
    nextValue: RealtimeServiceFormValue[Key],
  ) => onChange({ ...value, [key]: nextValue });

  return (
    <form onSubmit={onSubmit} className="space-y-5">
      <div className="space-y-2">
        <Label>プロジェクト</Label>
        <ProjectSelector
          value={value.projectId}
          onValueChange={(projectId) => update("projectId", projectId)}
          disabled={disabled || projectLocked}
        />
      </div>

      <div className="space-y-2">
        <Label htmlFor="realtime-service-name">サービス名</Label>
        <Input
          id="realtime-service-name"
          required
          maxLength={120}
          value={value.name}
          disabled={disabled}
          onChange={(event) => update("name", event.target.value)}
          placeholder="realtime-production"
        />
      </div>

      <div className="grid gap-4 sm:grid-cols-3">
        <div className="space-y-2">
          <Label>リージョン</Label>
          <Select
            value={value.region}
            onValueChange={(region) => update("region", region)}
            disabled={disabled}
          >
            <SelectTrigger aria-label="リージョン">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="heteronet-global">HeteroNet Global</SelectItem>
              <SelectItem value="heteronet-jp">HeteroNet Japan</SelectItem>
            </SelectContent>
          </Select>
        </div>
        <div className="space-y-2">
          <Label htmlFor="realtime-max-participants">同時参加者上限</Label>
          <Input
            id="realtime-max-participants"
            type="number"
            required
            min={1}
            max={100_000}
            step={1}
            value={value.maxParticipants}
            disabled={disabled}
            onChange={(event) =>
              update("maxParticipants", event.currentTarget.valueAsNumber || 1)
            }
          />
        </div>
        <div className="space-y-2">
          <Label htmlFor="realtime-max-rooms">ルーム上限</Label>
          <Input
            id="realtime-max-rooms"
            type="number"
            required
            min={1}
            step={1}
            value={value.maxRooms}
            disabled={disabled}
            onChange={(event) =>
              update("maxRooms", positiveInteger(event.currentTarget.valueAsNumber))
            }
          />
        </div>
      </div>

      <fieldset className="space-y-2" disabled={disabled}>
        <legend className="text-sm font-medium text-zinc-800">通信モード</legend>
        <div className="grid gap-3 sm:grid-cols-2">
          {(
            [
              {
                value: "direct",
                label: "ダイレクト",
                description: "公開IP所有ノードへ配置し、転送を行いません。",
              },
              {
                value: "forwarded",
                label: "転送",
                description: "公開ノードを入口として内部Podへ転送します。",
              },
            ] as const
          ).map((mode) => (
            <label
              key={mode.value}
              className={`cursor-pointer border p-3 ${
                value.trafficMode === mode.value
                  ? "border-emerald-600 bg-emerald-50"
                  : "border-zinc-200 hover:bg-zinc-50"
              }`}
            >
              <span className="flex items-center gap-2">
                <input
                  type="radio"
                  name="traffic-mode"
                  value={mode.value}
                  checked={value.trafficMode === mode.value}
                  onChange={() => update("trafficMode", mode.value)}
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
          <Label htmlFor="realtime-turn-enabled">TURN</Label>
          <p className="mt-1 text-xs text-zinc-500">接続不能時のリレー経路</p>
        </div>
        <Switch
          id="realtime-turn-enabled"
          checked={value.turnEnabled}
          onCheckedChange={(turnEnabled) => update("turnEnabled", turnEnabled)}
          disabled={disabled}
        />
      </div>

      {children}
    </form>
  );
}
