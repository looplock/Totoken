import type { ReactNode } from 'react';
import { ArrowDownRight, ArrowUpRight } from 'lucide-react';

export type KpiCardTone = 'default' | 'success' | 'warning' | 'danger';
export type KpiDeltaDirection = 'up' | 'down' | 'flat';

export interface KpiCardProps {
  /** Headline of the card, e.g. "Today tokens". */
  label: string;
  /** Pre-formatted main value. We don't format inside the component because
   *  call-sites already vary (Intl.NumberFormat, currency, percent, etc.). */
  value: string;
  /** Optional smaller line below the value (e.g. "12 / 480 failures"). */
  secondary?: string;
  /** Percent (-1..+∞ in absolute units, e.g. 12.4 for +12.4%). When `null`
   *  we render an em-dash and skip the up/down arrow entirely. */
  deltaPercent?: number | null;
  sparkline?: number[];
  /** Tone affects the card's accent rail; the delta colour is independent. */
  tone?: KpiCardTone;
  /** Slot for a tiny inline graphic — Sparkline goes here in P3. */
  trailing?: ReactNode;
}

const PERCENT_FORMATTER = new Intl.NumberFormat('en-US', {
  minimumFractionDigits: 1,
  maximumFractionDigits: 1,
});

/**
 * Single KPI cell. Pure presentation — caller decides what the value/secondary
 * strings look like. We compute the up/down arrow + colour from `deltaPercent`
 * here so every card behaves consistently:
 *  - delta > +0.05  → up arrow, success colour
 *  - delta < -0.05  → down arrow, danger colour
 *  - |delta| ≤ 0.05 → flat dash, muted colour ("noise floor")
 *  - delta == null  → no arrow, dash placeholder
 */
export function KpiCard({
  label,
  value,
  secondary,
  deltaPercent,
  sparkline,
  tone = 'default',
  trailing,
}: KpiCardProps) {
  const deltaInfo = describeDelta(deltaPercent);
  return (
    <article className={`dashboard-kpi-card dashboard-kpi-card-${tone}`}>
      <header className="dashboard-kpi-head">
        <span className="dashboard-kpi-label">{label}</span>
        {trailing ? <span className="dashboard-kpi-trailing">{trailing}</span> : null}
      </header>
      <div className="dashboard-kpi-value">{value}</div>
      <footer className="dashboard-kpi-foot">
        {secondary ? <span className="dashboard-kpi-secondary">{secondary}</span> : <span />}
        <span className="dashboard-kpi-trend">
          <span
            className={`dashboard-kpi-delta dashboard-kpi-delta-${deltaInfo.direction}`}
            aria-label={deltaInfo.ariaLabel}
          >
            {deltaInfo.direction === 'down' ? <ArrowDownRight size={14} /> : null}
            {deltaInfo.direction === 'up' || deltaInfo.direction === 'flat' ? (
              <ArrowUpRight size={14} />
            ) : null}
            {deltaInfo.label}
          </span>
          {sparkline && sparkline.length > 0 ? <DashboardKpiSparkline values={sparkline} /> : null}
        </span>
      </footer>
    </article>
  );
}

function DashboardKpiSparkline({ values }: { values: number[] }) {
  const width = 74;
  const height = 26;
  const path = buildLinePath(values.length > 0 ? values : [0], width, height, 2);

  return (
    <svg className="dashboard-kpi-sparkline" viewBox={`0 0 ${width} ${height}`} aria-hidden="true">
      <path d={path} fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
    </svg>
  );
}

function buildLinePath(values: number[], width: number, height: number, inset: number) {
  if (values.length === 0) {
    return '';
  }

  const max = Math.max(...values);
  const min = Math.min(...values);
  const range = max - min || 1;

  return values
    .map((value, index) => {
      const x = inset + (index / Math.max(1, values.length - 1)) * (width - inset * 2);
      const y = height - inset - ((value - min) / range) * (height - inset * 2);
      return `${index === 0 ? 'M' : 'L'}${x.toFixed(2)} ${y.toFixed(2)}`;
    })
    .join(' ');
}

function describeDelta(deltaPercent: number | null | undefined): {
  direction: KpiDeltaDirection | 'none';
  label: string;
  ariaLabel: string;
} {
  if (deltaPercent === null || deltaPercent === undefined || !Number.isFinite(deltaPercent)) {
    return { direction: 'none', label: '—', ariaLabel: 'no comparison' };
  }
  if (Math.abs(deltaPercent) <= 0.05) {
    return {
      direction: 'flat',
      label: ' 0%',
      ariaLabel: 'no change',
    };
  }
  if (deltaPercent > 0) {
    return {
      direction: 'up',
      label: ` ${PERCENT_FORMATTER.format(deltaPercent)}%`,
      ariaLabel: `up ${PERCENT_FORMATTER.format(deltaPercent)} percent`,
    };
  }
  return {
    direction: 'down',
    label: ` ${PERCENT_FORMATTER.format(Math.abs(deltaPercent))}%`,
    ariaLabel: `down ${PERCENT_FORMATTER.format(Math.abs(deltaPercent))} percent`,
  };
}
