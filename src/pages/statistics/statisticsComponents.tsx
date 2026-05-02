import { ArrowDownRight, ArrowUpRight, ChevronDown } from 'lucide-react';
import { AppIcon } from '../../components/app-icon/AppIcon';
import { sourceAppLabelKey } from '../../lib/sourceApps';
import type { StatisticsDetailRow } from './statisticsTypes';
import { buildLinePath, formatRelativeTime, formatUsdAmount } from './statisticsView';

export function SelectField({
  value,
  onChange,
  options,
  compact = false,
}: {
  value: string;
  onChange: (value: string) => void;
  options: Array<{ value: string; label: string }>;
  compact?: boolean;
}) {
  return (
    <label
      className={
        compact ? 'statistics-select-wrap statistics-select-wrap-compact' : 'statistics-select-wrap'
      }
    >
      <select value={value} onChange={(event) => onChange(event.target.value)}>
        {options.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
      <ChevronDown size={16} />
    </label>
  );
}

export function StatisticsRow({
  locale,
  row,
  numberFormatter,
  t,
}: {
  locale: string;
  row: StatisticsDetailRow;
  numberFormatter: Intl.NumberFormat;
  t: (key: string) => string;
}) {
  const total = row.inputTokens + row.outputTokens;
  const appLabel = t(sourceAppLabelKey(row.app));

  return (
    <tr>
      <td>
        <span className="statistics-app-cell">
          <AppIcon app={row.app} label={appLabel} />
          <span>{appLabel}</span>
        </span>
      </td>
      <td className="statistics-table-muted">{row.model}</td>
      <td>{numberFormatter.format(row.sessions)}</td>
      <td>{numberFormatter.format(row.inputTokens)}</td>
      <td>{numberFormatter.format(row.outputTokens)}</td>
      <td>{numberFormatter.format(total)}</td>
      <td>{formatUsdAmount(row.estimatedCostUsd)}</td>
      <td>{numberFormatter.format(row.avgTokensPerSession)}</td>
      <td className="statistics-table-muted">{formatRelativeTime(row.lastActiveAt, locale)}</td>
      <td>
        <div className="statistics-trend-cell">
          <MiniSparkline values={row.sparkline} />
          <span
            className={
              row.trendDirection === 'down'
                ? 'statistics-trend-badge statistics-trend-down'
                : 'statistics-trend-badge statistics-trend-up'
            }
          >
            {row.trendDirection === 'down' ? (
              <ArrowDownRight size={13} />
            ) : (
              <ArrowUpRight size={13} />
            )}
            {Math.abs(row.trendPercent).toFixed(1)}%
          </span>
        </div>
      </td>
    </tr>
  );
}

export function MiniSparkline({
  values,
  color = 'var(--color-accent)',
}: {
  values: number[];
  color?: string;
}) {
  const width = 74;
  const height = 26;
  const path = buildLinePath(values.length > 0 ? values : [0], width, height, 2);

  return (
    <svg
      className="statistics-mini-sparkline"
      viewBox={`0 0 ${width} ${height}`}
      aria-hidden="true"
    >
      <path d={path} fill="none" stroke={color} strokeWidth="2" strokeLinecap="round" />
    </svg>
  );
}

export { TrendChart } from './statisticsTrendChart';
export { DistributionChart, ModelUsagePanel } from './statisticsDistributionCharts';
export { HeatmapMatrix } from './statisticsHeatmap';
