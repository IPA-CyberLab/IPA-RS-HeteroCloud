import Box from "@cloudscape-design/components/box";
import Container from "@cloudscape-design/components/container";
import Header from "@cloudscape-design/components/header";
import LineChart from "@cloudscape-design/components/line-chart";
import type { RealtimeServiceMetricSample } from "@/lib/api-types";

export function RealtimeMetricChart({
  label,
  samples,
  value,
  formatValue,
  loading = false,
}: {
  label: string;
  samples: RealtimeServiceMetricSample[];
  value: (sample: RealtimeServiceMetricSample) => number;
  formatValue: (value: number) => string;
  color: string;
  loading?: boolean;
}) {
  const data = [...samples]
    .sort((left, right) => Date.parse(left.sampled_at) - Date.parse(right.sampled_at))
    .map((sample) => ({
      x: new Date(sample.sampled_at),
      y: Math.max(0, Number.isFinite(value(sample)) ? value(sample) : 0),
    }));
  const latest = data.at(-1)?.y ?? 0;

  return (
    <Container
      header={
        <Header
          variant="h2"
          actions={<Box variant="awsui-value-large">{loading && !data.length ? "-" : formatValue(latest)}</Box>}
        >
          {label}
        </Header>
      }
    >
      <LineChart
        ariaLabel={`${label}の推移`}
        height={220}
        statusType={loading ? "loading" : "finished"}
        loadingText="履歴を読み込んでいます"
        xScaleType="time"
        yScaleType="linear"
        series={[{ title: label, type: "line", data }]}
        xTitle="測定日時"
        yTitle={label}
        hideFilter
        hideLegend
        detailPopoverSize="medium"
        empty={<Box textAlign="center" color="text-body-secondary">履歴データがありません</Box>}
        noMatch={<Box textAlign="center">表示できるデータがありません</Box>}
        i18nStrings={{
          filterLabel: "表示する系列",
          filterPlaceholder: "系列を選択",
          filterSelectedAriaLabel: "選択済み",
          legendAriaLabel: "凡例",
          chartAriaRoleDescription: "時系列グラフ",
          xTickFormatter: (date) =>
            new Intl.DateTimeFormat("ja-JP", {
              month: "numeric",
              day: "numeric",
              hour: "2-digit",
              minute: "2-digit",
            }).format(date),
          yTickFormatter: formatValue,
        }}
      />
    </Container>
  );
}
