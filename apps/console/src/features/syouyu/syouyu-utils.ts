import type {
  SyouyuBucket,
  SyouyuBucketSpec,
  SyouyuQuotaLimits,
} from "@/lib/api-types";

export const GIBIBYTE = 1024 ** 3;

export function formatBytes(value: number): string {
  if (!Number.isFinite(value) || value < 0) return "-";
  if (value < 1024) return `${Math.round(value)} B`;

  const units = ["KiB", "MiB", "GiB", "TiB", "PiB"];
  let amount = value / 1024;
  let unitIndex = 0;
  while (amount >= 1024 && unitIndex < units.length - 1) {
    amount /= 1024;
    unitIndex += 1;
  }
  return `${amount.toLocaleString("ja-JP", {
    maximumFractionDigits: amount >= 100 ? 0 : amount >= 10 ? 1 : 2,
  })} ${units[unitIndex]}`;
}

export function quotaGib(bytes: number): number {
  return Math.max(1, Math.ceil(bytes / GIBIBYTE));
}

export interface SyouyuBucketFormValue {
  projectId: string;
  bucketName: string;
  region: string;
  quotaGib: number;
  quotaObjects: number;
}

export const defaultSyouyuBucketFormValue: SyouyuBucketFormValue = {
  projectId: "",
  bucketName: "",
  region: "heteronet-global",
  quotaGib: 10,
  quotaObjects: 1_000_000,
};

function looksLikeIpv4(value: string): boolean {
  const parts = value.split(".");
  return (
    parts.length === 4 &&
    parts.every((part) => /^\d{1,3}$/.test(part) && Number(part) <= 255)
  );
}

export function bucketNameError(value: string): string | null {
  if (value.length < 3 || value.length > 63) {
    return "バケット名は3〜63文字で入力してください。";
  }
  if (!/^[a-z0-9][a-z0-9.-]*[a-z0-9]$/.test(value)) {
    return "小文字の英数字で開始・終了し、小文字、数字、ハイフン、ピリオドだけを使用してください。";
  }
  if (value.includes("..")) {
    return "ピリオドを連続して使用できません。";
  }
  if (looksLikeIpv4(value)) {
    return "IPアドレス形式のバケット名は使用できません。";
  }
  return null;
}

export function bucketFormError(
  value: SyouyuBucketFormValue,
  quota: SyouyuQuotaLimits,
): string | null {
  if (!value.projectId) return "プロジェクトを選択してください。";
  const nameError = bucketNameError(value.bucketName.trim());
  if (nameError) return nameError;
  if (!/^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/.test(value.region)) {
    return "リージョンが不正です。";
  }
  if (!Number.isInteger(value.quotaGib) || value.quotaGib < 1) {
    return "容量上限は1 GiB以上の整数で入力してください。";
  }
  if (value.quotaGib * GIBIBYTE > quota.max_bytes_per_bucket) {
    return `容量上限は${formatBytes(quota.max_bytes_per_bucket)}以下にしてください。`;
  }
  if (!Number.isInteger(value.quotaObjects) || value.quotaObjects < 1) {
    return "オブジェクト数上限は1以上の整数で入力してください。";
  }
  if (value.quotaObjects > quota.max_objects_per_bucket) {
    return `オブジェクト数上限は${quota.max_objects_per_bucket.toLocaleString("ja-JP")}以下にしてください。`;
  }
  return null;
}

export function bucketSpecFromForm(
  value: SyouyuBucketFormValue,
  metadata: Record<string, unknown> = {},
): SyouyuBucketSpec {
  return {
    region: value.region,
    bucket_name: value.bucketName.trim(),
    quota_bytes: value.quotaGib * GIBIBYTE,
    quota_objects: value.quotaObjects,
    metadata,
  };
}

export function bucketFormFromBucket(
  bucket: SyouyuBucket,
): SyouyuBucketFormValue {
  return {
    projectId: bucket.project_id,
    bucketName: bucket.spec.bucket_name,
    region: bucket.spec.region,
    quotaGib: quotaGib(bucket.spec.quota_bytes),
    quotaObjects: bucket.spec.quota_objects,
  };
}

export function defaultBucketForm(
  quota: SyouyuQuotaLimits,
  availableBytes = quota.max_total_bytes,
): SyouyuBucketFormValue {
  return {
    ...defaultSyouyuBucketFormValue,
    quotaGib: Math.max(
      1,
      Math.min(
        10,
        Math.floor(quota.max_bytes_per_bucket / GIBIBYTE),
        Math.floor(availableBytes / GIBIBYTE),
      ),
    ),
    quotaObjects: Math.max(
      1,
      Math.min(1_000_000, quota.max_objects_per_bucket),
    ),
  };
}
