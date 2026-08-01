import { useId } from "react";
import type { RealtimeServiceMetricSample } from "@/lib/api-types";
import { formatDateTime } from "@/lib/utils";

const chartWidth = 640;
const chartHeight = 176;
const plotTop = 12;
const plotBottom = 164;

interface RealtimeMetricChartProps {
  label: string;
  samples: RealtimeServiceMetricSample[];
  value: (sample: RealtimeServiceMetricSample) => number;
  formatValue: (value: number) => string;
  color: string;
  loading?: boolean;
}

function formatAxisTime(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "—";

  return new Intl.DateTimeFormat("ja-JP", {
    month: "numeric",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(date);
}

export function RealtimeMetricChart({
  label,
  samples,
  value,
  formatValue,
  color,
  loading = false,
}: RealtimeMetricChartProps) {
  const id = useId();
  const orderedSamples = [...samples].sort(
    (left, right) => Date.parse(left.sampled_at) - Date.parse(right.sampled_at),
  );
  const values = orderedSamples.map((sample) => {
    const sampleValue = value(sample);
    return Number.isFinite(sampleValue) ? Math.max(0, sampleValue) : 0;
  });
  const peak = Math.max(0, ...values);
  const scaleMaximum = Math.max(1, peak);
  const latest = values.at(-1) ?? 0;
  const points = values
    .map((sampleValue, index) => {
      const x =
        values.length === 1
          ? chartWidth / 2
          : (index / (values.length - 1)) * chartWidth;
      const y =
        plotTop + (1 - sampleValue / scaleMaximum) * (plotBottom - plotTop);
      return `${x},${y}`;
    })
    .join(" ");
  const firstSample = orderedSamples.at(0);
  const lastSample = orderedSamples.at(-1);

  return (
    <figure
      className="min-w-0 border border-zinc-200 bg-white p-4"
      aria-labelledby={`${id}-caption`}
    >
      <figcaption id={`${id}-caption`} className="flex items-baseline justify-between gap-4">
        <span className="text-sm font-semibold text-zinc-900">{label}</span>
        <span className="text-lg font-semibold text-zinc-950">
          {loading && samples.length === 0 ? "—" : formatValue(latest)}
        </span>
      </figcaption>

      <div className="mt-3 h-40 w-full border-y border-zinc-100 bg-zinc-50/60">
        {orderedSamples.length === 0 ? (
          <div
            className="flex h-full items-center justify-center text-sm text-zinc-500"
            role="status"
          >
            {loading ? "履歴を読み込んでいます" : "履歴データがありません"}
          </div>
        ) : (
          <svg
            className="h-full w-full"
            viewBox={`0 0 ${chartWidth} ${chartHeight}`}
            preserveAspectRatio="none"
            role="img"
            aria-labelledby={`${id}-title ${id}-description`}
          >
            <title id={`${id}-title`}>{label}の推移</title>
            <desc id={`${id}-description`}>
              {orderedSamples.length}件の測定値。最新値は{formatValue(latest)}、最大値は
              {formatValue(peak)}です。
            </desc>
            {[0.25, 0.5, 0.75].map((position) => (
              <line
                key={position}
                x1="0"
                x2={chartWidth}
                y1={plotTop + position * (plotBottom - plotTop)}
                y2={plotTop + position * (plotBottom - plotTop)}
                stroke="#d4d4d8"
                strokeWidth="1"
                vectorEffect="non-scaling-stroke"
              />
            ))}
            <polyline
              points={points}
              fill="none"
              stroke={color}
              strokeWidth="2.5"
              strokeLinejoin="round"
              strokeLinecap="round"
              vectorEffect="non-scaling-stroke"
            />
            {values.map((sampleValue, index) => {
              const x =
                values.length === 1
                  ? chartWidth / 2
                  : (index / (values.length - 1)) * chartWidth;
              const y =
                plotTop +
                (1 - sampleValue / scaleMaximum) * (plotBottom - plotTop);
              const sample = orderedSamples[index];
              return (
                <circle
                  key={`${sample.sampled_at}-${index}`}
                  cx={x}
                  cy={y}
                  r={index === values.length - 1 ? 3.5 : 2}
                  fill={color}
                  vectorEffect="non-scaling-stroke"
                >
                  <title>
                    {formatDateTime(sample.sampled_at)}: {formatValue(sampleValue)}
                  </title>
                </circle>
              );
            })}
          </svg>
        )}
      </div>

      <div className="mt-2 flex justify-between gap-3 text-xs text-zinc-500">
        <span>{firstSample ? formatAxisTime(firstSample.sampled_at) : "—"}</span>
        <span>{lastSample ? formatAxisTime(lastSample.sampled_at) : "—"}</span>
      </div>

      <table className="sr-only">
        <caption>{label}の履歴データ</caption>
        <thead>
          <tr>
            <th scope="col">測定日時</th>
            <th scope="col">値</th>
          </tr>
        </thead>
        <tbody>
          {orderedSamples.map((sample, index) => (
            <tr key={`${sample.sampled_at}-${index}`}>
              <td>{formatDateTime(sample.sampled_at)}</td>
              <td>{formatValue(Math.max(0, value(sample)))}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </figure>
  );
}
