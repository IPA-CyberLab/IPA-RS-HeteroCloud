import ColumnLayout from "@cloudscape-design/components/column-layout";
import FormField from "@cloudscape-design/components/form-field";
import Input from "@cloudscape-design/components/input";
import Select from "@cloudscape-design/components/select";
import SpaceBetween from "@cloudscape-design/components/space-between";
import Toggle from "@cloudscape-design/components/toggle";
import type { FormEvent, ReactNode } from "react";
import { ProjectSelector } from "@/components/shared/resource-selectors";

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

const regions = [
  { value: "heteronet-global", label: "HeteroNet Global" },
  { value: "heteronet-jp", label: "HeteroNet Japan" },
];

function integer(value: string, fallback = 1) {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? Math.max(1, Math.trunc(parsed)) : fallback;
}

export function RealtimeServiceForm({
  value,
  onChange,
  onSubmit,
  disabled,
  projectLocked,
  children,
}: {
  value: RealtimeServiceFormValue;
  onChange: (value: RealtimeServiceFormValue) => void;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
  disabled?: boolean;
  projectLocked?: boolean;
  children: ReactNode;
}) {
  const update = <Key extends keyof RealtimeServiceFormValue>(
    key: Key,
    nextValue: RealtimeServiceFormValue[Key],
  ) => onChange({ ...value, [key]: nextValue });

  return (
    <form onSubmit={onSubmit}>
      <SpaceBetween size="l">
        <FormField label="プロジェクト">
          <ProjectSelector
            value={value.projectId}
            onValueChange={(projectId) => update("projectId", projectId)}
            disabled={disabled || projectLocked}
          />
        </FormField>
        <FormField label="サービス名">
          <Input
            value={value.name}
            disabled={disabled}
            placeholder="realtime-production"
            onChange={({ detail }) => update("name", detail.value.slice(0, 120))}
          />
        </FormField>
        <ColumnLayout columns={3}>
          <FormField label="リージョン">
            <Select
              ariaLabel="リージョン"
              selectedOption={regions.find((region) => region.value === value.region) ?? regions[0]}
              options={regions}
              disabled={disabled}
              onChange={({ detail }) => update("region", detail.selectedOption.value ?? regions[0].value)}
            />
          </FormField>
          <FormField label="同時参加者上限" constraintText="1〜100,000">
            <Input
              type="number"
              inputMode="numeric"
              step={1}
              nativeInputAttributes={{ min: 1, max: 100_000 }}
              value={String(value.maxParticipants)}
              disabled={disabled}
              onChange={({ detail }) => update("maxParticipants", integer(detail.value))}
            />
          </FormField>
          <FormField label="ルーム上限" constraintText="1〜1,000,000">
            <Input
              type="number"
              inputMode="numeric"
              step={1}
              nativeInputAttributes={{ min: 1, max: 1_000_000 }}
              value={String(value.maxRooms)}
              disabled={disabled}
              onChange={({ detail }) => update("maxRooms", integer(detail.value))}
            />
          </FormField>
        </ColumnLayout>
        <ColumnLayout columns={2}>
          <FormField
            label="RPS上限"
            description="同一送信元IPに許可する1秒あたりの要求数"
          >
            <Input
              type="number"
              inputMode="numeric"
              step={1}
              nativeInputAttributes={{ min: 1, max: 1_000 }}
              value={String(value.rateLimitRequestsPerSecond)}
              disabled={disabled}
              onChange={({ detail }) => update("rateLimitRequestsPerSecond", integer(detail.value))}
            />
          </FormField>
          <FormField
            label="バースト上限"
            description="短時間に許可する要求数の上限"
          >
            <Input
              type="number"
              inputMode="numeric"
              step={1}
              nativeInputAttributes={{ min: 1, max: 5_000 }}
              value={String(value.rateLimitBurst)}
              disabled={disabled}
              onChange={({ detail }) => update("rateLimitBurst", integer(detail.value))}
            />
          </FormField>
        </ColumnLayout>
        <Toggle
          checked={value.turnEnabled}
          disabled={disabled}
          onChange={({ detail }) => update("turnEnabled", detail.checked)}
        >
          TURNリレーを有効化
        </Toggle>
        {children}
      </SpaceBetween>
    </form>
  );
}
