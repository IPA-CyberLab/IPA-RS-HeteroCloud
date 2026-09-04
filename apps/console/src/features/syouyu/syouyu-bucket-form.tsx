import ColumnLayout from "@cloudscape-design/components/column-layout";
import FormField from "@cloudscape-design/components/form-field";
import Input from "@cloudscape-design/components/input";
import Select from "@cloudscape-design/components/select";
import SpaceBetween from "@cloudscape-design/components/space-between";
import type { FormEvent, ReactNode } from "react";
import { ProjectSelector } from "@/components/shared/resource-selectors";
import type { SyouyuQuotaLimits } from "@/lib/api-types";
import {
  bucketFormError,
  bucketNameError,
  formatBytes,
  GIBIBYTE,
  type SyouyuBucketFormValue,
} from "./syouyu-utils";

const regions = [
  { value: "heteronet-global", label: "HeteroNet Global" },
  { value: "heteronet-jp", label: "HeteroNet Japan" },
];

function integer(value: string): number {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? Math.max(1, Math.trunc(parsed)) : 1;
}

export function SyouyuBucketForm({
  value,
  quota,
  onChange,
  onSubmit,
  disabled,
  children,
}: {
  value: SyouyuBucketFormValue;
  quota: SyouyuQuotaLimits;
  onChange: (value: SyouyuBucketFormValue) => void;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
  disabled?: boolean;
  children?: ReactNode;
}) {
  const update = <Key extends keyof SyouyuBucketFormValue>(
    key: Key,
    next: SyouyuBucketFormValue[Key],
  ) => onChange({ ...value, [key]: next });
  const validationError = bucketFormError(value, quota);
  const nameError = value.bucketName ? bucketNameError(value.bucketName.trim()) : null;

  return (
    <form onSubmit={onSubmit}>
      <SpaceBetween size="l">
        <FormField label="プロジェクト">
          <ProjectSelector
            value={value.projectId}
            onValueChange={(projectId) => update("projectId", projectId)}
            disabled={disabled}
          />
        </FormField>
        <FormField
          label="バケット名"
          description="S3 APIで使用するグローバルに一意な名前。作成後は変更できません。"
          errorText={nameError ?? undefined}
        >
          <Input
            value={value.bucketName}
            placeholder="production-assets"
            disabled={disabled}
            autoComplete="off"
            onChange={({ detail }) =>
              update("bucketName", detail.value.toLowerCase().slice(0, 63))
            }
          />
        </FormField>
        <ColumnLayout columns={3}>
          <FormField label="リージョン">
            <Select
              ariaLabel="リージョン"
              selectedOption={
                regions.find((region) => region.value === value.region) ?? regions[0]
              }
              options={regions}
              disabled={disabled}
              onChange={({ detail }) =>
                update("region", detail.selectedOption.value ?? regions[0].value)
              }
            />
          </FormField>
          <FormField
            label="容量上限 (GiB)"
            constraintText={`最大 ${formatBytes(quota.max_bytes_per_bucket)}`}
          >
            <Input
              type="number"
              inputMode="numeric"
              step={1}
              nativeInputAttributes={{
                min: 1,
                max: Math.max(1, Math.floor(quota.max_bytes_per_bucket / GIBIBYTE)),
              }}
              value={String(value.quotaGib)}
              disabled={disabled}
              onChange={({ detail }) => update("quotaGib", integer(detail.value))}
            />
          </FormField>
          <FormField
            label="オブジェクト数上限"
            constraintText={`最大 ${quota.max_objects_per_bucket.toLocaleString("ja-JP")}`}
          >
            <Input
              type="number"
              inputMode="numeric"
              step={1}
              nativeInputAttributes={{ min: 1, max: quota.max_objects_per_bucket }}
              value={String(value.quotaObjects)}
              disabled={disabled}
              onChange={({ detail }) => update("quotaObjects", integer(detail.value))}
            />
          </FormField>
        </ColumnLayout>
        {children}
        <button type="submit" hidden disabled={Boolean(validationError)} />
      </SpaceBetween>
    </form>
  );
}
