import type { ReactNode } from 'react';
import type { DashboardTopRow } from '../dashboardTypes';

export interface ShareBarProps {
  /** Rows in display order; share is 0..1 of the displayed percentage. */
  rows: DashboardTopRow[];
  renderLabel?: (row: DashboardTopRow, index: number) => ReactNode;
  emptyText?: ReactNode;
  tokenUnitLocale: string;
}

const PERCENT_FORMATTER = new Intl.NumberFormat('en-US', {
  maximumFractionDigits: 1,
});

export function ShareBar({ rows, renderLabel, emptyText, tokenUnitLocale }: ShareBarProps) {
  if (rows.length === 0) {
    return <p className="dashboard-share-empty">{emptyText ?? '—'}</p>;
  }

  const maxValue = Math.max(...rows.map((row) => row.value), 0);
  const axisTicks = buildShareAxisTicks(maxValue, tokenUnitLocale);

  return (
    <div className="dashboard-share-panel">
      <ol className="dashboard-share-list">
        {rows.map((row, index) => {
          const barPercent = maxValue > 0 ? (row.value / maxValue) * 100 : 0;
          const sharePercent = Math.max(0, Math.min(1, row.share)) * 100;
          return (
            <li key={`${row.label}-${index}`} className="dashboard-share-row">
              <span className="dashboard-share-label" title={row.label}>
                {renderLabel ? renderLabel(row, index) : row.label}
              </span>
              <span className="dashboard-share-bar" aria-hidden="true">
                <span
                  className="dashboard-share-bar-fill"
                  style={{ width: `${barPercent.toFixed(2)}%` }}
                />
              </span>
              <span className="dashboard-share-value">
                {PERCENT_FORMATTER.format(sharePercent)}%
              </span>
            </li>
          );
        })}
      </ol>
      <div
        className="dashboard-share-axis"
        style={{ gridTemplateColumns: `repeat(${axisTicks.length}, minmax(0, 1fr))` }}
      >
        {axisTicks.map((tick) => (
          <span key={`${tick.value}-${tick.percent}`}>{tick.label}</span>
        ))}
      </div>
    </div>
  );
}

function buildShareAxisTicks(maxValue: number, tokenUnitLocale: string) {
  const formatter = new Intl.NumberFormat(tokenUnitLocale, {
    notation: 'compact',
    maximumFractionDigits: 1,
  });

  return [0, maxValue / 2, maxValue].map((value, index) => ({
    value,
    percent: index * 50,
    label: formatter.format(Math.round(value)),
  }));
}
