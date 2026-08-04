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

export interface RealtimeServiceFormValue {
  projectId: string;
  name: string;
  region: string;
  maxParticipants: number;
  maxRooms: number;
  rateLimitRequestsPerSecond: number;
  rateLimitBurst: number;
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
  maxParticipants: 100,
  maxRooms: 100,
  rateLimitRequestsPerSecond: 20,
  rateLimitBurst: 40,
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
            max={1_000_000}
            step={1}
            value={value.maxRooms}
            disabled={disabled}
            onChange={(event) =>
              update("maxRooms", positiveInteger(event.currentTarget.valueAsNumber))
            }
          />
        </div>
      </div>

      <fieldset className="border-t border-zinc-100 pt-4" disabled={disabled}>
        <legend className="mb-3 text-sm font-medium text-zinc-800">
          IPレート制限
        </legend>
        <div className="grid gap-4 sm:grid-cols-2">
          <div className="space-y-2">
            <Label htmlFor="realtime-rate-limit-rps">RPS上限</Label>
            <Input
              id="realtime-rate-limit-rps"
              type="number"
              required
              min={1}
              max={1_000}
              step={1}
              value={value.rateLimitRequestsPerSecond}
              disabled={disabled}
              onChange={(event) =>
                update(
                  "rateLimitRequestsPerSecond",
                  positiveInteger(event.currentTarget.valueAsNumber),
                )
              }
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="realtime-rate-limit-burst">バースト上限</Label>
            <Input
              id="realtime-rate-limit-burst"
              type="number"
              required
              min={1}
              max={5_000}
              step={1}
              value={value.rateLimitBurst}
              disabled={disabled}
              onChange={(event) =>
                update(
                  "rateLimitBurst",
                  positiveInteger(event.currentTarget.valueAsNumber),
                )
              }
            />
          </div>
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
