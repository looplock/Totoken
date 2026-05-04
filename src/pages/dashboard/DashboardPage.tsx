import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Link } from 'react-router-dom';
import { BarChart3, FolderKanban, RefreshCw, Workflow } from 'lucide-react';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { AppIcon } from '../../components/app-icon/AppIcon';
import { EmptyState } from '../../components/empty-state/EmptyState';
import type { StatusTone } from '../../components/status/StatusDot';
import { InfoTooltip } from '../../components/tooltip/InfoTooltip';
import { useI18n } from '../../i18n/useI18n';
import { sourceApps, type SourceApp } from '../../lib/sourceApps';
import { isTauriRuntime } from '../../lib/tauri';
import { useTokenUnitLocale } from '../../lib/useTokenUnitLocale';
import { KpiCard } from './components/KpiCard';
import { ShareBar } from './components/ShareBar';
import { loadDashboard } from './dashboardService';
import type {
  DashboardKpiCostDelta,
  DashboardKpiDelta,
  DashboardSection,
  DashboardViewModel,
} from './dashboardTypes';
import './DashboardPage.css';

const CLOCK_INTERVAL_MS = 30_000;
const COST_FORMATTER = new Intl.NumberFormat('en-US', {
  style: 'currency',
  currency: 'USD',
  minimumFractionDigits: 2,
  maximumFractionDigits: 2,
});

type DashboardStatusSummary = {
  key: Extract<DashboardSection, 'scanner'>;
  label: string;
  cardValue: string;
  detail?: string;
  tone: StatusTone;
};

type ScanNotificationEvent = {
  status: string;
};

export function DashboardPage() {
  const { t, locale } = useI18n();
  const tokenUnitLocale = useTokenUnitLocale(locale);
  const runtimeAvailable = isTauriRuntime();
  const [model, setModel] = useState<DashboardViewModel | null>(null);
  const [initialLoading, setInitialLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [loadError, setLoadError] = useState('');
  const [now, setNow] = useState(() => Date.now());
  const loadRef = useRef<() => Promise<void>>();
  const loadRequestIdRef = useRef(0);

  const load = useCallback(async () => {
    const requestId = loadRequestIdRef.current + 1;
    loadRequestIdRef.current = requestId;

    if (!runtimeAvailable) {
      setInitialLoading(false);
      setLoadError(t('dashboard.feedback.runtimeRequired'));
      return;
    }

    setRefreshing(true);
    try {
      const next = await loadDashboard();
      if (loadRequestIdRef.current !== requestId) {
        return;
      }
      setModel(next);
      setLoadError('');
    } catch (error) {
      if (loadRequestIdRef.current !== requestId) {
        return;
      }
      setLoadError(error instanceof Error ? error.message : t('dashboard.feedback.loadFailed'));
    } finally {
      if (loadRequestIdRef.current === requestId) {
        setInitialLoading(false);
        setRefreshing(false);
      }
    }
  }, [runtimeAvailable, t]);

  useEffect(() => {
    loadRef.current = load;
  }, [load]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    const timer = window.setInterval(() => {
      setNow(Date.now());
    }, CLOCK_INTERVAL_MS);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    if (!runtimeAvailable) {
      return undefined;
    }

    let unlisten: UnlistenFn | undefined;
    let cancelled = false;
    void listen<ScanNotificationEvent>('scan-notification', (event) => {
      if (event.payload.status === 'completed' || event.payload.status === 'failed') {
        setNow(Date.now());
        void loadRef.current?.();
      }
    }).then((nextUnlisten) => {
      if (cancelled) {
        nextUnlisten();
        return;
      }
      unlisten = nextUnlisten;
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [runtimeAvailable]);

  const partialFailureLabel = useMemo(
    () => (model ? formatPartialFailures(model.partialFailures, t) : ''),
    [model, t],
  );

  return (
    <div className="page dashboard-page">
      <header className="page-hero dashboard-hero">
        <div>
          <h1 className="page-title">{t('dashboard.title')}</h1>
          <p className="page-subtitle">{t('dashboard.subtitle')}</p>
        </div>
        <div className="dashboard-hero-actions">
          <StatusActionButtons t={t} />
          <Link className="dashboard-btn" to="/statistics">
            <BarChart3 size={16} />
            <span>{t('dashboard.action.openStatistics')}</span>
          </Link>
          <button
            type="button"
            className="dashboard-btn dashboard-btn-primary"
            onClick={() => void load()}
            disabled={refreshing || initialLoading}
          >
            <RefreshCw size={16} className={refreshing ? 'dashboard-spin' : undefined} />
            <span>
              {refreshing ? t('dashboard.feedback.refreshing') : t('dashboard.action.refresh')}
            </span>
          </button>
        </div>
      </header>

      {loadError ? <div className="dashboard-error-banner">{loadError}</div> : null}
      {partialFailureLabel ? (
        <div className="dashboard-warning-banner">
          {t('dashboard.feedback.partialFailure', { areas: partialFailureLabel })}
        </div>
      ) : null}

      {initialLoading && !model ? (
        <DashboardSkeleton />
      ) : model ? (
        <>
          <KpiRow model={model} t={t} locale={locale} tokenUnitLocale={tokenUnitLocale} now={now} />
          <TrendRow model={model} t={t} tokenUnitLocale={tokenUnitLocale} />
        </>
      ) : null}
    </div>
  );
}

function StatusActionButtons({ t }: { t: ReturnType<typeof useI18n>['t'] }) {
  return (
    <>
      <Link className="dashboard-btn" to="/sessions">
        <FolderKanban size={16} />
        <span>{t('dashboard.action.openSessions')}</span>
      </Link>
      <Link className="dashboard-btn" to="/management/scan-records">
        <Workflow size={16} />
        <span>{t('nav.scanRecords')}</span>
      </Link>
    </>
  );
}

function KpiRow({
  model,
  t,
  locale,
  tokenUnitLocale,
  now,
}: {
  model: DashboardViewModel;
  t: ReturnType<typeof useI18n>['t'];
  locale: 'zh' | 'en';
  tokenUnitLocale: string;
  now: number;
}) {
  const failedSet = new Set(model.partialFailures);
  const statisticsFailed = failedSet.has('statistics');

  return (
    <section className="dashboard-kpi-row">
      {buildStatusSummaries(model, t, locale, now).map((item) => (
        <article
          key={item.key}
          className={`dashboard-status-summary-card dashboard-status-summary-card-${item.tone}`}
        >
          <header className="dashboard-status-summary-head">
            <span>{item.label}</span>
          </header>
          <strong className="dashboard-status-summary-value">{item.cardValue}</strong>
          {item.detail ? (
            <span className="dashboard-status-summary-detail">{item.detail}</span>
          ) : null}
        </article>
      ))}
      <KpiCard
        label={t('dashboard.kpi.todayTokens')}
        value={statisticsFailed ? '-' : formatTokens(model.kpi.todayTokens, tokenUnitLocale)}
        secondary={statisticsFailed ? t('dashboard.feedback.sectionFailed') : undefined}
        deltaPercent={statisticsFailed ? null : model.kpi.todayTokens.deltaPercent}
        sparkline={statisticsFailed ? [] : model.kpi.todayTokens.sparkline}
        tone="default"
      />
      <KpiCard
        label={t('dashboard.kpi.todaySessions')}
        value={statisticsFailed ? '-' : formatCount(model.kpi.todaySessions.value, locale)}
        secondary={statisticsFailed ? t('dashboard.feedback.sectionFailed') : undefined}
        deltaPercent={statisticsFailed ? null : model.kpi.todaySessions.deltaPercent}
        sparkline={statisticsFailed ? [] : model.kpi.todaySessions.sparkline}
        tone="default"
      />
      <KpiCard
        label={t('dashboard.kpi.todayCost')}
        value={statisticsFailed ? '-' : formatCost(model.kpi.todayCostUsd)}
        secondary={statisticsFailed ? t('dashboard.feedback.sectionFailed') : undefined}
        deltaPercent={statisticsFailed ? null : model.kpi.todayCostUsd.deltaPercent}
        sparkline={statisticsFailed ? [] : model.kpi.todayCostUsd.sparkline}
        tone="default"
      />
    </section>
  );
}

function buildStatusSummaries(
  model: DashboardViewModel,
  t: ReturnType<typeof useI18n>['t'],
  locale: 'zh' | 'en',
  now: number,
): DashboardStatusSummary[] {
  const failedSet = new Set(model.partialFailures);

  const scanner = (() => {
    if (failedSet.has('scanner')) {
      return {
        key: 'scanner',
        label: t('dashboard.section.scanner'),
        cardValue: t('dashboard.feedback.sectionFailed'),
        tone: 'muted',
      } satisfies DashboardStatusSummary;
    }

    const s = model.scanner;
    if (!s.lastRunAt) {
      return {
        key: 'scanner',
        label: t('dashboard.section.scanner'),
        cardValue: t('dashboard.status.scannerNever'),
        tone: 'muted',
      } satisfies DashboardStatusSummary;
    }

    const tone: StatusTone =
      s.status === 'failed' || s.errorCount > 0
        ? 'danger'
        : s.status === 'running'
          ? 'warning'
          : 'success';
    const relative = formatRelative(s.lastRunAt, now, locale);
    return {
      key: 'scanner',
      label: t('dashboard.section.scanner'),
      cardValue: s.status === 'running' ? t('dashboard.status.scannerRunning') : relative,
      detail:
        s.errorCount > 0 ? t('dashboard.status.scanErrors', { count: s.errorCount }) : undefined,
      tone,
    } satisfies DashboardStatusSummary;
  })();

  return [scanner];
}

function TrendRow({
  model,
  t,
  tokenUnitLocale,
}: {
  model: DashboardViewModel;
  t: ReturnType<typeof useI18n>['t'];
  tokenUnitLocale: string;
}) {
  const failedSet = new Set(model.partialFailures);
  const statisticsFailed = failedSet.has('statistics');
  const tokenSeries = model.trendToday.map((point) => point.tokens);
  const inputSeries = model.trendToday.map((point) => point.inputTokens);
  const outputSeries = model.trendToday.map((point) => point.outputTokens);
  const costSeries = model.trendToday.map((point) => point.costUsd);

  return (
    <section className="dashboard-row dashboard-row-2">
      <article className="dashboard-card dashboard-trend-card">
        <header className="dashboard-card-header">
          <div>
            <div className="dashboard-card-title-wrap">
              <h2 className="dashboard-card-title">{t('dashboard.trend.title')}</h2>
              <InfoTooltip
                label={t('dashboard.info.show')}
                content={t('dashboard.info.trend')}
              />
            </div>
            <p className="dashboard-card-subtitle">{t('dashboard.trend.subtitle')}</p>
          </div>
          <div className="dashboard-trend-legend">
            <span>{t('dashboard.trend.legendTokens')}</span>
            <span>{t('dashboard.trend.legendInput')}</span>
            <span>{t('dashboard.trend.legendOutput')}</span>
            <span>{t('dashboard.trend.legendCost')}</span>
          </div>
        </header>
        <div className="dashboard-card-body">
          {statisticsFailed ? (
            <EmptyState compact>
              <p>{t('dashboard.feedback.sectionFailed')}</p>
            </EmptyState>
          ) : model.trendToday.length === 0 ? (
            <EmptyState compact>
              <p>{t('dashboard.trend.empty')}</p>
            </EmptyState>
          ) : (
            <DashboardTrendChart
              tokens={tokenSeries}
              inputTokens={inputSeries}
              outputTokens={outputSeries}
              costs={costSeries}
              tokenUnitLocale={tokenUnitLocale}
              tokenLabel={t('dashboard.trend.legendTokens')}
              inputLabel={t('dashboard.trend.legendInput')}
              outputLabel={t('dashboard.trend.legendOutput')}
              costLabel={t('dashboard.trend.legendCost')}
            />
          )}
        </div>
      </article>

      <div className="dashboard-distribution-stack">
        <DistributionShareCard
          title={t('dashboard.topModels.title')}
          subtitle={t('dashboard.topModels.subtitle')}
          infoLabel={t('dashboard.info.show')}
          infoContent={t('dashboard.info.topModels')}
          rows={model.topModels}
          emptyText={t('dashboard.top.empty')}
          failed={statisticsFailed}
          failedText={t('dashboard.feedback.sectionFailed')}
          tokenUnitLocale={tokenUnitLocale}
        />
        <DistributionShareCard
          title={t('dashboard.topApps.title')}
          subtitle={t('dashboard.topApps.subtitle')}
          infoLabel={t('dashboard.info.show')}
          infoContent={t('dashboard.info.topApps')}
          rows={model.topApps}
          emptyText={t('dashboard.top.empty')}
          failed={statisticsFailed}
          failedText={t('dashboard.feedback.sectionFailed')}
          tokenUnitLocale={tokenUnitLocale}
          renderLabel={(row) => (
            <span className="dashboard-share-app">
              <AppIcon app={resolveTopAppIcon(row.label)} label={row.label} />
              {row.label}
            </span>
          )}
        />
      </div>
    </section>
  );
}

function DistributionShareCard({
  title,
  subtitle,
  infoLabel,
  infoContent,
  rows,
  emptyText,
  failed = false,
  failedText,
  tokenUnitLocale,
  renderLabel,
}: {
  title: string;
  subtitle: string;
  infoLabel: string;
  infoContent: string;
  rows: DashboardViewModel['topModels'];
  emptyText: string;
  failed?: boolean;
  failedText?: string;
  tokenUnitLocale: string;
  renderLabel?: Parameters<typeof ShareBar>[0]['renderLabel'];
}) {
  return (
    <article className="dashboard-card dashboard-distribution-card">
      <header className="dashboard-card-header">
        <div>
          <div className="dashboard-card-title-wrap">
            <h2 className="dashboard-card-title">{title}</h2>
            <InfoTooltip label={infoLabel} content={infoContent} />
          </div>
          <p className="dashboard-card-subtitle">{subtitle}</p>
        </div>
      </header>
      <div className="dashboard-card-body">
        {failed ? (
          <EmptyState compact>
            <p>{failedText ?? emptyText}</p>
          </EmptyState>
        ) : (
          <ShareBar
            rows={rows}
            emptyText={emptyText}
            tokenUnitLocale={tokenUnitLocale}
            renderLabel={renderLabel}
          />
        )}
      </div>
    </article>
  );
}

function DashboardSkeleton() {
  return (
    <>
      <section className="dashboard-card dashboard-skeleton-strip" aria-hidden="true">
        <span /> <span /> <span /> <span />
      </section>
      <section className="dashboard-row dashboard-row-2 dashboard-skeleton-rows" aria-hidden="true">
        <span /> <span />
      </section>
    </>
  );
}

function DashboardTrendChart({
  tokens,
  inputTokens,
  outputTokens,
  costs,
  tokenUnitLocale,
  tokenLabel,
  inputLabel,
  outputLabel,
  costLabel,
}: {
  tokens: number[];
  inputTokens: number[];
  outputTokens: number[];
  costs: number[];
  tokenUnitLocale: string;
  tokenLabel: string;
  inputLabel: string;
  outputLabel: string;
  costLabel: string;
}) {
  return (
    <div className="dashboard-trend-chart">
      <DashboardTrendMiniChart
        label={tokenLabel}
        values={tokens}
        tone="token"
        gradientId="dashboard-token-area"
        formatTick={(value) => formatDashboardTokenTick(value, tokenUnitLocale)}
      />
      <DashboardTrendMiniChart
        label={inputLabel}
        values={inputTokens}
        tone="input"
        gradientId="dashboard-input-area"
        formatTick={(value) => formatDashboardTokenTick(value, tokenUnitLocale)}
      />
      <DashboardTrendMiniChart
        label={outputLabel}
        values={outputTokens}
        tone="output"
        gradientId="dashboard-output-area"
        formatTick={(value) => formatDashboardTokenTick(value, tokenUnitLocale)}
      />
      <DashboardTrendMiniChart
        label={costLabel}
        values={costs}
        tone="cost"
        gradientId="dashboard-cost-area"
        formatTick={formatDashboardCostTick}
      />
    </div>
  );
}

function DashboardTrendMiniChart({
  label,
  values,
  tone,
  gradientId,
  formatTick,
}: {
  label: string;
  values: number[];
  tone: 'token' | 'input' | 'output' | 'cost';
  gradientId: string;
  formatTick: (value: number) => string;
}) {
  const width = 360;
  const height = 170;
  const padding = { top: 22, right: 56, bottom: 16, left: 16 };
  const frame = {
    x: padding.left,
    y: padding.top,
    width: width - padding.left - padding.right,
    height: height - padding.top - padding.bottom,
  };
  const series = normalizeSeries(values);
  const scale = buildDashboardScale(series);
  const points = buildDashboardChartPoints(series, frame, scale);
  const path = buildSmoothPath(points);
  const areaPath = buildAreaPathFromPoints(points, frame);
  const last = points[points.length - 1];
  const classSuffix = tone === 'token' ? 'token' : tone;

  return (
    <svg
      className="dashboard-trend-mini-chart"
      viewBox={`0 0 ${width} ${height}`}
      role="img"
      aria-label={label}
    >
      <defs>
        <linearGradient id={gradientId} x1="0%" y1="0%" x2="0%" y2="100%">
          <stop offset="0%" stopColor={`var(--dashboard-trend-${tone})`} stopOpacity="0.25" />
          <stop offset="54%" stopColor={`var(--dashboard-trend-${tone})`} stopOpacity="0.11" />
          <stop offset="100%" stopColor={`var(--dashboard-trend-${tone})`} stopOpacity="0" />
        </linearGradient>
      </defs>

      <DashboardTrendGrid frame={frame} ticks={buildDashboardTickLabels(scale, formatTick)} />
      <text x={frame.x} y={frame.y - 7} className="dashboard-trend-chart-label">
        {label}
      </text>
      <path className="dashboard-trend-area" d={areaPath} fill={`url(#${gradientId})`} />
      <path className={`dashboard-trend-line dashboard-trend-line-${classSuffix}`} d={path} />
      {last ? (
        <circle
          className={`dashboard-trend-dot dashboard-trend-dot-${classSuffix}`}
          cx={last.x}
          cy={last.y}
          r="3"
        />
      ) : null}
    </svg>
  );
}

function DashboardTrendGrid({
  frame,
  ticks,
}: {
  frame: { x: number; y: number; width: number; height: number };
  ticks: Array<{ ratio: number; label: string }>;
}) {
  return (
    <g className="dashboard-trend-grid-lines">
      {ticks.map((tick) => (
        <g key={tick.ratio}>
          <line
            x1={frame.x}
            x2={frame.x + frame.width}
            y1={frame.y + frame.height * tick.ratio}
            y2={frame.y + frame.height * tick.ratio}
          />
          <text
            x={frame.x + frame.width + 8}
            y={frame.y + frame.height * tick.ratio}
            dy="0.32em"
            className="dashboard-trend-axis-label"
          >
            {tick.label}
          </text>
        </g>
      ))}
    </g>
  );
}

function normalizeSeries(values: number[]) {
  const cleaned = values.filter((value) => Number.isFinite(value));
  if (cleaned.length >= 2) {
    return cleaned;
  }

  if (cleaned.length === 1) {
    return [cleaned[0], cleaned[0]];
  }

  return [0, 0];
}

function buildDashboardChartPoints(
  values: number[],
  frame: { x: number; y: number; width: number; height: number },
  scale: { min: number; max: number },
) {
  const { min, max } = scale;
  const range = max - min || Math.max(max, 1);
  const stepX = frame.width / Math.max(1, values.length - 1);

  return values.map((value, index) => {
    const normalized = max === min ? 0 : (value - min) / range;
    return {
      x: frame.x + stepX * index,
      y: frame.y + frame.height - normalized * (frame.height * 0.86) - frame.height * 0.07,
    };
  });
}

function buildDashboardScale(values: number[]) {
  const finiteValues = values.filter((value) => Number.isFinite(value));
  if (finiteValues.length === 0) {
    return { min: 0, max: 1 };
  }

  return {
    min: Math.min(...finiteValues),
    max: Math.max(...finiteValues),
  };
}

function buildDashboardTickLabels(
  scale: { min: number; max: number },
  formatter: (value: number) => string,
) {
  const middle = scale.min + (scale.max - scale.min) / 2;
  return [
    { ratio: 0, label: formatter(scale.max) },
    { ratio: 0.5, label: formatter(middle) },
    { ratio: 1, label: formatter(scale.min) },
  ];
}

function buildSmoothPath(points: Array<{ x: number; y: number }>) {
  if (points.length === 0) {
    return '';
  }

  if (points.length === 1) {
    return `M${points[0].x.toFixed(2)},${points[0].y.toFixed(2)}`;
  }

  return points.reduce((path, point, index) => {
    if (index === 0) {
      return `M${point.x.toFixed(2)},${point.y.toFixed(2)}`;
    }

    const previous = points[index - 1];
    const controlX = previous.x + (point.x - previous.x) / 2;
    return `${path} C${controlX.toFixed(2)},${previous.y.toFixed(2)} ${controlX.toFixed(2)},${point.y.toFixed(2)} ${point.x.toFixed(2)},${point.y.toFixed(2)}`;
  }, '');
}

function buildAreaPathFromPoints(
  points: Array<{ x: number; y: number }>,
  frame: { x: number; y: number; width: number; height: number },
) {
  if (points.length === 0) {
    return '';
  }

  const linePath = buildSmoothPath(points);
  const baseline = frame.y + frame.height;
  const last = points[points.length - 1];
  const first = points[0];
  return `${linePath} L${last.x.toFixed(2)},${baseline.toFixed(2)} L${first.x.toFixed(2)},${baseline.toFixed(2)} Z`;
}

function formatTokens(d: DashboardKpiDelta, tokenUnitLocale: string): string {
  return new Intl.NumberFormat(tokenUnitLocale, {
    notation: 'compact',
    maximumFractionDigits: 1,
  }).format(d.value);
}

function formatCost(d: DashboardKpiCostDelta): string {
  return COST_FORMATTER.format(d.valueUsd);
}

function formatCount(value: number, locale: 'zh' | 'en'): string {
  return new Intl.NumberFormat(locale === 'zh' ? 'zh-CN' : 'en-US', {
    maximumFractionDigits: 0,
  }).format(value);
}

function formatDashboardTokenTick(value: number, tokenUnitLocale: string) {
  return new Intl.NumberFormat(tokenUnitLocale, {
    notation: 'compact',
    maximumFractionDigits: 1,
  }).format(value);
}

function formatDashboardCostTick(value: number) {
  if (value === 0) {
    return '$0';
  }

  if (Math.abs(value) < 0.01) {
    return `$${value.toFixed(4)}`;
  }

  return new Intl.NumberFormat('en-US', {
    style: 'currency',
    currency: 'USD',
    notation: 'compact',
    maximumFractionDigits: 2,
  }).format(value);
}

function formatPartialFailures(
  sections: DashboardSection[],
  t: ReturnType<typeof useI18n>['t'],
): string {
  if (sections.length === 0) return '';
  return sections.map((key) => t(`dashboard.section.${key}`)).join(', ');
}

function resolveTopAppIcon(label: string): SourceApp | 'generic' {
  return sourceApps.includes(label as SourceApp) ? (label as SourceApp) : 'generic';
}

function formatRelative(value: string | null, now: number, locale: 'zh' | 'en'): string {
  if (!value) return '';
  const date = new Date(value);
  const ms = now - date.getTime();
  if (!Number.isFinite(ms) || ms < 0) return '';
  const minutes = Math.round(ms / 60_000);
  if (minutes < 1) return locale === 'zh' ? '刚刚' : 'just now';
  if (minutes < 60) return locale === 'zh' ? `${minutes} 分钟前` : `${minutes}m ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return locale === 'zh' ? `${hours} 小时前` : `${hours}h ago`;
  const days = Math.round(hours / 24);
  if (days < 30) return locale === 'zh' ? `${days} 天前` : `${days}d ago`;
  return date.toLocaleDateString();
}
