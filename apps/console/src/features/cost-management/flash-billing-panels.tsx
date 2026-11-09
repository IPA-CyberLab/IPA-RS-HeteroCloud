import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Alert, Box, Button, ColumnLayout, Container, FormField, Header, Input, SpaceBetween } from "@/components/ui";
import { FormError } from "@/components/shared/form-error";
import { api, getApiErrorMessage } from "@/lib/api-client";
import type { FlashEstimate } from "@/lib/api-types";

const SAFE_INTEGER = 9_000_000_000_000_000n;

// Decimal strings avoid floating-point errors when storing prices and budgets.
export function toMicrounits(value: string, maximum = SAFE_INTEGER): number | null {
  const match = /^(0|[1-9]\d*)(?:\.(\d{1,6}))?$/.exec(value.trim());
  if (!match) return null;
  const result = BigInt(match[1]) * 1_000_000n + BigInt((match[2] ?? "").padEnd(6, "0"));
  return result <= maximum ? Number(result) : null;
}

export function formatMicrounits(currency: string, amount: number) {
  const value = BigInt(amount);
  const whole = value / 1_000_000n;
  const fraction = (value % 1_000_000n).toString().padStart(6, "0").replace(/0+$/, "");
  return `${currency} ${whole.toLocaleString("ja-JP")}${fraction ? `.${fraction}` : ""}`;
}

export function decimalMicrounits(amount: number) {
  const value = BigInt(amount);
  const whole = value / 1_000_000n;
  const fraction = (value % 1_000_000n).toString().padStart(6, "0").replace(/0+$/, "");
  return `${whole}${fraction ? `.${fraction}` : ""}`;
}

export function FlashBudgetPanel({ organizationId, estimate, canEdit }: {
  organizationId: string; estimate?: FlashEstimate; canEdit: boolean;
}) {
  const queryClient = useQueryClient();
  const [currency, setCurrency] = useState(estimate?.budget?.currency ?? estimate?.rate_card?.currency ?? "USD");
  const [amount, setAmount] = useState(estimate?.budget ? decimalMicrounits(estimate.budget.amount_microunits) : "");
  const [warning, setWarning] = useState(String(estimate?.budget?.warning_percentage ?? 80));
  const amountMicros = toMicrounits(amount);
  const warningNumber = Number(warning);
  const valid = /^[A-Z]{3}$/.test(currency) && amountMicros !== null && amountMicros > 0 &&
    Number.isInteger(warningNumber) && warningNumber >= 1 && warningNumber <= 100;
  const refresh = () => queryClient.invalidateQueries({ queryKey: ["organizations", organizationId, "flash", "usage"] });
  const save = useMutation({
    mutationFn: () => api.flash.weeklyBudget.set(organizationId, {
      currency, amount_microunits: amountMicros!, warning_percentage: warningNumber,
    }),
    onSuccess: refresh,
  });
  const remove = useMutation({ mutationFn: () => api.flash.weeklyBudget.remove(organizationId), onSuccess: refresh });
  const price = estimate?.rate_card;
  const budget = estimate?.budget;
  return <Container header={<Header variant="h2" description="Flash の週間割当時間から算出する概算です。請求額や全サービスの費用ではありません。">週間費用と予算</Header>}>
    <SpaceBetween size="m">
      <Box>今週の概算: {price && estimate?.amount_microunits != null
        ? formatMicrounits(price.currency, estimate.amount_microunits)
        : "取得できません（今週の料金が未設定、または計算範囲外）"}</Box>
      <Box>週間予算: {budget ? formatMicrounits(budget.currency, budget.amount_microunits) : "未設定"}</Box>
      {budget && price && budget.currency !== price.currency && <Alert type="warning">予算と料金の通貨が異なるため、予算との比較はできません。</Alert>}
      {estimate?.warning_reached && <Alert type="warning">週間予算の警告率に達しました。これはサービスの停止や請求ではありません。</Alert>}
      {canEdit && <>
        <ColumnLayout columns={3}>
          <FormField label="通貨コード"><Input value={currency} onChange={({detail}) => setCurrency(detail.value.toUpperCase())} /></FormField>
          <FormField label="週間予算"><Input value={amount} inputMode="decimal" onChange={({detail}) => setAmount(detail.value)} /></FormField>
          <FormField label="警告率（%）"><Input value={warning} inputMode="numeric" onChange={({detail}) => setWarning(detail.value)} /></FormField>
        </ColumnLayout>
        {save.isError && <FormError message={getApiErrorMessage(save.error)} />}
        {remove.isError && <FormError message={getApiErrorMessage(remove.error)} />}
        <SpaceBetween direction="horizontal" size="xs">
          <Button variant="primary" disabled={!valid || save.isPending} loading={save.isPending} onClick={() => save.mutate()}>予算を保存</Button>
          {budget && <Button disabled={remove.isPending} onClick={() => remove.mutate()}>予算を解除</Button>}
        </SpaceBetween>
      </>}
    </SpaceBetween>
  </Container>;
}

export function FlashRateCardPanel() {
  const client = useQueryClient();
  const rates = useQuery({ queryKey: ["owner", "flash", "rate-cards"], queryFn: ({signal}) => api.owner.flashRateCards.list(signal) });
  const [currency, setCurrency] = useState("USD");
  const [cpu, setCpu] = useState("");
  const [memory, setMemory] = useState("");
  const [gpu, setGpu] = useState("");
  const values = [cpu, memory, gpu].map(value => toMicrounits(value, 1_000_000_000_000n));
  const valid = /^[A-Z]{3}$/.test(currency) && values.every(value => value !== null);
  const schedule = useMutation({
    mutationFn: () => api.owner.flashRateCards.schedule({ currency,
      cpu_microunits_per_vcpu_hour: values[0]!, memory_microunits_per_gib_hour: values[1]!,
      gpu_microunits_per_gpu_hour: values[2]! }),
    onSuccess: () => client.invalidateQueries({queryKey: ["owner", "flash", "rate-cards"]}),
  });
  return <Container header={<Header variant="h2" description="新しい単価は翌週の月曜 00:00 UTC から適用されます。既に始まった週の単価は変わりません。">Flash 料金の設定</Header>}>
    <SpaceBetween size="m">
      {rates.isError && <FormError message={getApiErrorMessage(rates.error)} />}
      {rates.data?.items.length ? <Box>最新の設定: {rates.data.items[0].currency} · 適用開始 {new Date(rates.data.items[0].effective_week_start * 1000).toLocaleString("ja-JP")}</Box> : <Box>単価は設定されていません。</Box>}
      <ColumnLayout columns={4}>
        <FormField label="通貨コード"><Input value={currency} onChange={({detail}) => setCurrency(detail.value.toUpperCase())} /></FormField>
        <FormField label="vCPU 時間あたり"><Input value={cpu} inputMode="decimal" onChange={({detail}) => setCpu(detail.value)} /></FormField>
        <FormField label="メモリ GiB 時間あたり"><Input value={memory} inputMode="decimal" onChange={({detail}) => setMemory(detail.value)} /></FormField>
        <FormField label="GPU 時間あたり"><Input value={gpu} inputMode="decimal" onChange={({detail}) => setGpu(detail.value)} /></FormField>
      </ColumnLayout>
      {schedule.isError && <FormError message={getApiErrorMessage(schedule.error)} />}
      {schedule.isSuccess && <Alert type="success">翌週からの単価を保存しました。</Alert>}
      <Button variant="primary" disabled={!valid || schedule.isPending} loading={schedule.isPending} onClick={() => schedule.mutate()}>翌週の単価を保存</Button>
    </SpaceBetween>
  </Container>;
}
