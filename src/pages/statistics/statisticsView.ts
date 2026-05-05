import { buildPageTokens, type PageToken } from '../../lib/pagination';
import { sourceAppLabelKey, type DisplaySourceApp, type SourceApp } from '../../lib/sourceApps';
import type {
  StatisticsDetailRow,
  StatisticsDistributionRow,
  StatisticsGranularity,
  StatisticsOverview,
  StatisticsPeriodFilter,
  StatisticsSummary,
  StatisticsTrend,
} from './statisticsTypes';

export type { PageToken };
export type TrendDirection = 'up' | 'down';
export type TrendSeriesKey = 'cache-read' | 'cache-write' | 'input' | 'output' | 'total' | 'cost';
export type DistributionMetric = 'sessions' | 'tokens' | 'cost';
export type ActivityMetric = 'sessions' | 'tokens' | 'cost';

export type MetricCard = {
  key: keyof StatisticsSummary;
  labelKey: string;
  value: string;
  delta: number;
  direction: TrendDirection;
  sparkline: number[];
};

export type DistributionItem = {
  label: string;
  value: number;
  color: string;
  shareLabel: string;
};

export type ModelUsageItem = {
  label: string;
  value: number;
  percent: number;
  sharePercent: number;
  shareLabel: string;
  sessions: number;
  inputTokens: number;
  outputTokens: number;
  avgTokensPerSession: number;
  lastActiveAt: string | null;
  trendPercent: number;
  trendDirection: TrendDirection;
  sparkline: number[];
  modelCount: number;
};

export type StatisticsNotice =
  | { kind: 'literal'; text: string }
  | { kind: 'copy'; key: 'runtimeRequired' | 'empty' }
  | null;

export const periodOptions: StatisticsPeriodFilter[] = ['1d', '7d', '30d', 'custom'];
export const granularityOptions: StatisticsGranularity[] = ['hour', 'day', 'week', 'month'];
export const palette: Record<SourceApp, string> = {
  claude_code: '#4e88ff',
  codex: '#9068f9',
  cursor: '#111827',
  opencode: '#7bcfee',
  kilocode: '#17a673',
  kiro: '#ff7f50',
};
const genericSourceAppColor = '#6f7c92';
export const emptySummary: StatisticsSummary = {
  totalTokens: { value: 0, deltaPercent: 0 },
  inputTokens: { value: 0, deltaPercent: 0 },
  outputTokens: { value: 0, deltaPercent: 0 },
  estimatedCostUsd: { value: 0, deltaPercent: 0 },
  totalSessions: { value: 0, deltaPercent: 0 },
  activeModels: { value: 0, deltaPercent: 0 },
  avgTokensPerSession: { value: 0, deltaPercent: 0 },
};
export const defaultVisibleTrendSeries: Record<TrendSeriesKey, boolean> = {
  'cache-read': true,
  'cache-write': true,
  input: true,
  output: true,
  total: true,
  cost: true,
};

export function getDefaultGranularityForPeriod(
  period: StatisticsPeriodFilter,
): StatisticsGranularity {
  return period === '1d' ? 'hour' : 'day';
}

export function buildTrendViewData(
  trend: StatisticsTrend | undefined,
  locale: string,
  granularity: StatisticsGranularity | undefined,
) {
  const bucketStarts = trend?.bucketStarts ?? [];
  const cacheReadInput = trend?.cacheReadInput ?? [];
  const cacheWriteInput = trend?.cacheWriteInput ?? [];
  const input = trend?.input ?? [];
  const output = trend?.output ?? [];
  const total = trend?.total ?? [];
  const costUsd = trend?.costUsd ?? [];
  const labels = bucketStarts.map((value) =>
    formatTrendBucketLabel(new Date(value), locale, granularity),
  );
  return {
    labels,
    cacheReadInput,
    cacheWriteInput,
    input,
    output,
    total,
    costUsd,
  };
}

export function formatActivityCellRange(dayLabel: string, hour: number) {
  const nextHour = (hour + 1) % 24;
  return `${dayLabel} · ${String(hour).padStart(2, '0')}:00-${String(nextHour).padStart(2, '0')}:00`;
}

function formatActivityCellCount(locale: string, count: number) {
  const formatted = new Intl.NumberFormat(locale === 'zh' ? 'zh-CN' : 'en-US').format(count);
  if (locale === 'zh') {
    return `${formatted} 个会话`;
  }

  return `${formatted} ${count === 1 ? 'session' : 'sessions'}`;
}
export function formatActivityMetricTooltip(
  locale: string,
  dayLabel: string,
  hour: number,
  value: number,
  metric: ActivityMetric,
) {
  return `${formatActivityCellRange(dayLabel, hour)} - ${formatActivityCellValue(locale, value, metric)}`;
}

export function formatActivityCellValue(locale: string, value: number, metric: ActivityMetric) {
  if (metric === 'cost') {
    return formatActivityCostValue(value);
  }

  const roundedValue = Math.round(value);
  const formatted = new Intl.NumberFormat(locale === 'zh' ? 'zh-CN' : 'en-US').format(roundedValue);

  if (metric === 'tokens') {
    return locale === 'zh' ? `${formatted} Tokens` : `${formatted} tokens`;
  }

  return formatActivityCellCount(locale, roundedValue);
}

function formatActivityCostValue(value: number) {
  if (Math.abs(value) < 1) {
    return formatUsdValue(value, 4, 4);
  }

  return formatUsdAmount(value);
}

export function buildDistribution(
  rows: StatisticsDistributionRow[],
  metric: DistributionMetric,
  totalValue: number,
  t: (key: string) => string,
  enabledApps: SourceApp[],
) {
  const extraApps = rows
    .map((row) => row.app)
    .filter(
      (app, index, apps) => !enabledApps.includes(app as SourceApp) && apps.indexOf(app) === index,
    );
  const appOrder: DisplaySourceApp[] = [...enabledApps, ...extraApps];

  return appOrder
    .map((app) => {
      const row = rows.find((item) => item.app === app);
      const value =
        metric === 'sessions'
          ? (row?.sessions ?? 0)
          : metric === 'cost'
            ? (row?.estimatedCostUsd ?? 0)
            : (row?.totalTokens ?? 0);
      return {
        label: t(sourceAppLabelKey(app)),
        value,
        color: getSourceAppColor(app),
        shareLabel: `${totalValue > 0 ? ((value / totalValue) * 100).toFixed(1) : '0.0'}%`,
      };
    })
    .filter((item) => item.value > 0);
}

export function getSourceAppColor(app: DisplaySourceApp): string {
  return app === 'generic' ? genericSourceAppColor : palette[app];
}
export function buildModelUsage(
  rows: StatisticsDetailRow[],
  totalValue: number,
  metric: DistributionMetric,
) {
  const grouped = new Map<
    string,
    Omit<ModelUsageItem, 'percent' | 'sharePercent' | 'shareLabel'>
  >();
  for (const row of rows) {
    const value = getModelUsageValue(row, metric);
    const current = grouped.get(row.model) ?? {
      label: row.model,
      value: 0,
      sessions: 0,
      inputTokens: 0,
      outputTokens: 0,
      avgTokensPerSession: 0,
      lastActiveAt: null,
      trendPercent: 0,
      trendDirection: 'up' as TrendDirection,
      sparkline: [],
      modelCount: 1,
    };
    current.value += value;
    current.sessions += row.sessions;
    current.inputTokens += row.inputTokens;
    current.outputTokens += row.outputTokens;
    current.lastActiveAt =
      current.lastActiveAt === null
        ? row.lastActiveAt
        : row.lastActiveAt === null
          ? current.lastActiveAt
          : current.lastActiveAt > row.lastActiveAt
            ? current.lastActiveAt
            : row.lastActiveAt;
    current.sparkline = mergeSparklineSeries(current.sparkline, row.sparkline);
    grouped.set(row.model, current);
  }

  const items = Array.from(grouped.entries())
    .map(([, item]) => {
      const avgTokensPerSession = item.sessions > 0 ? Math.round(item.value / item.sessions) : 0;
      const trendPercent = computeSparklineDeltaPercent(item.sparkline);
      const sharePercent = totalValue > 0 ? (item.value / totalValue) * 100 : 0;
      return {
        ...item,
        avgTokensPerSession,
        trendPercent,
        trendDirection: getTrendDirection(trendPercent),
        sharePercent,
        shareLabel: formatPercentLabel(sharePercent),
        percent: 0,
      };
    })
    .sort((left, right) => right.value - left.value);

  const maxValue = items[0]?.value ?? 1;
  return items.map((item) => ({
    ...item,
    percent: (item.value / maxValue) * 100,
  }));
}

function getModelUsageValue(row: StatisticsDetailRow, metric: DistributionMetric) {
  if (metric === 'sessions') {
    return row.sessions;
  }

  if (metric === 'cost') {
    return row.estimatedCostUsd;
  }

  return row.inputTokens + row.outputTokens;
}

export function buildLinePath(values: number[], width: number, height: number, inset: number) {
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

export function buildChartPoints(
  values: number[],
  width: number,
  height: number,
  padding: { top: number; right: number; bottom: number; left: number },
  maxValue: number,
) {
  return values.map((value, index) => ({
    x: getChartX(index, values.length, width, padding),
    y: getChartY(value, height, padding, maxValue),
  }));
}

export function buildSmoothChartPath(points: Array<{ x: number; y: number }>) {
  if (points.length === 0) {
    return '';
  }

  if (points.length === 1) {
    return `M${points[0].x.toFixed(2)} ${points[0].y.toFixed(2)}`;
  }

  let path = `M${points[0].x.toFixed(2)} ${points[0].y.toFixed(2)}`;

  for (let index = 1; index < points.length; index += 1) {
    const previous = points[index - 1];
    const current = points[index];
    const controlX = ((previous.x + current.x) / 2).toFixed(2);
    path += ` C${controlX} ${previous.y.toFixed(2)}, ${controlX} ${current.y.toFixed(2)}, ${current.x.toFixed(2)} ${current.y.toFixed(2)}`;
  }

  return path;
}

export function buildSmoothAreaPath(
  points: Array<{ x: number; y: number }>,
  height: number,
  padding: { top: number; right: number; bottom: number; left: number },
) {
  if (points.length === 0) {
    return '';
  }

  const linePath = buildSmoothChartPath(points);
  const baselineY = (height - padding.bottom).toFixed(2);
  const startX = points[0].x.toFixed(2);
  const endX = points[points.length - 1].x.toFixed(2);
  return `${linePath} L${endX} ${baselineY} L${startX} ${baselineY} Z`;
}

export function averagePointY(points: Array<{ x: number; y: number }>) {
  if (points.length === 0) {
    return 0;
  }

  return points.reduce((sum, point) => sum + point.y, 0) / points.length;
}

export function getChartX(
  index: number,
  length: number,
  width: number,
  padding: { top: number; right: number; bottom: number; left: number },
) {
  if (length <= 1) {
    return padding.left;
  }

  return padding.left + (index / (length - 1)) * (width - padding.left - padding.right);
}

export function getChartY(
  value: number,
  height: number,
  padding: { top: number; right: number; bottom: number; left: number },
  maxValue: number,
) {
  const range = maxValue || 1;
  return height - padding.bottom - (value / range) * (height - padding.top - padding.bottom);
}

export function buildCompactModelUsageItems(items: ModelUsageItem[], locale: string) {
  const compactRowCount = 8;
  if (items.length <= compactRowCount) {
    return items.slice(0, compactRowCount);
  }

  const head = items.slice(0, compactRowCount - 1);
  const tail = items.slice(compactRowCount - 1);
  const others = tail.reduce<ModelUsageItem>(
    (aggregate, item) => ({
      ...aggregate,
      value: aggregate.value + item.value,
      sharePercent: aggregate.sharePercent + item.sharePercent,
      sessions: aggregate.sessions + item.sessions,
      inputTokens: aggregate.inputTokens + item.inputTokens,
      outputTokens: aggregate.outputTokens + item.outputTokens,
      avgTokensPerSession: 0,
      lastActiveAt:
        aggregate.lastActiveAt === null
          ? item.lastActiveAt
          : item.lastActiveAt === null
            ? aggregate.lastActiveAt
            : aggregate.lastActiveAt > item.lastActiveAt
              ? aggregate.lastActiveAt
              : item.lastActiveAt,
      sparkline: mergeSparklineSeries(aggregate.sparkline, item.sparkline),
      modelCount: aggregate.modelCount + item.modelCount,
    }),
    {
      label: locale === 'zh' ? `其他 ${tail.length} 个模型` : `Others (${tail.length})`,
      value: 0,
      percent: 0,
      sharePercent: 0,
      shareLabel: '0.0%',
      sessions: 0,
      inputTokens: 0,
      outputTokens: 0,
      avgTokensPerSession: 0,
      lastActiveAt: null,
      trendPercent: 0,
      trendDirection: 'up',
      sparkline: [],
      modelCount: 0,
    },
  );

  others.avgTokensPerSession = others.sessions > 0 ? Math.round(others.value / others.sessions) : 0;
  others.trendPercent = computeSparklineDeltaPercent(others.sparkline);
  others.trendDirection = getTrendDirection(others.trendPercent);
  others.shareLabel = formatPercentLabel(others.sharePercent);

  return [...head, others];
}

export function buildAxisTicks(
  maxValue: number,
  formatter: Pick<Intl.NumberFormat, 'format'>,
  count: number,
) {
  if (count <= 1) {
    return [{ value: maxValue, percent: 100, label: formatter.format(maxValue) }];
  }

  if (!maxValue) {
    return Array.from({ length: count }, (_, index) => ({
      value: 0,
      percent: (index / Math.max(1, count - 1)) * 100,
      label: '0',
    }));
  }

  return Array.from({ length: count }, (_, index) => {
    const value = Math.round((maxValue / Math.max(1, count - 1)) * index);
    return {
      value,
      percent: (index / Math.max(1, count - 1)) * 100,
      label: formatter.format(value),
    };
  });
}

export function createCompactCurrencyFormatter(): Pick<Intl.NumberFormat, 'format'> {
  return {
    format(value: number) {
      return new Intl.NumberFormat('en-US', {
        style: 'currency',
        currency: 'USD',
        notation: Math.abs(value) >= 1000 ? 'compact' : 'standard',
        minimumFractionDigits: Math.abs(value) >= 1000 ? 0 : 2,
        maximumFractionDigits: Math.abs(value) >= 1000 ? 1 : 2,
      }).format(value);
    },
  };
}

function mergeSparklineSeries(current: number[], next: number[]) {
  const maxLength = Math.max(current.length, next.length);
  return Array.from(
    { length: maxLength },
    (_, index) => (current[index] ?? 0) + (next[index] ?? 0),
  );
}

function computeSparklineDeltaPercent(values: number[]) {
  if (values.length === 0) {
    return 0;
  }

  const midpoint = Math.max(1, Math.floor(values.length / 2));
  const previous = values.slice(0, midpoint).reduce((sum, value) => sum + value, 0);
  const current = values.slice(midpoint).reduce((sum, value) => sum + value, 0);

  if (previous <= 0) {
    return current > 0 ? 100 : 0;
  }

  return ((current - previous) / previous) * 100;
}

function formatPercentLabel(value: number) {
  return `${value.toFixed(1)}%`;
}

export function buildTrendAxisScale(values: number[], tickCount: number) {
  const max = Math.max(0, ...values);
  if (max <= 0) {
    return {
      maxValue: 1,
      tickValues: Array.from(
        { length: tickCount },
        (_, index) => (tickCount - index - 1) / Math.max(1, tickCount - 1),
      ),
    };
  }

  return {
    maxValue: max,
    tickValues: Array.from(
      { length: tickCount },
      (_, index) => (max * (tickCount - index - 1)) / Math.max(1, tickCount - 1),
    ),
  };
}

export { buildPageTokens };

export function formatRelativeTime(value: string | null, locale: string) {
  if (!value) {
    return locale === 'zh' ? '暂无' : 'N/A';
  }

  const diffMs = Date.now() - new Date(value).getTime();
  const diffMinutes = Math.max(0, Math.floor(diffMs / 60_000));
  if (diffMinutes < 60) {
    return locale === 'zh' ? `${diffMinutes} 分钟前` : `${diffMinutes} min ago`;
  }

  const diffHours = Math.floor(diffMinutes / 60);
  if (diffHours < 48) {
    return locale === 'zh' ? `${diffHours} 小时前` : `${diffHours} hr ago`;
  }

  const diffDays = Math.floor(diffHours / 24);
  return locale === 'zh' ? `${diffDays} 天前` : `${diffDays} days ago`;
}

export function formatAxisNumber(value: number, tokenUnitLocale: string) {
  return new Intl.NumberFormat(tokenUnitLocale, {
    notation: 'compact',
    maximumFractionDigits: 1,
  }).format(value);
}

export function formatCurrencyAxisNumber(value: number, locale: string) {
  const prefix = '$';
  if (value >= 1000) {
    return `${prefix}${(value / 1000).toFixed(1)}K`;
  }

  if (value >= 100) {
    return `${prefix}${Math.round(value)}`;
  }

  if (value >= 10) {
    return `${prefix}${value.toFixed(1).replace(/\.0$/, '')}`;
  }

  if (value >= 1) {
    return `${prefix}${value.toFixed(2).replace(/0$/, '').replace(/\.$/, '')}`;
  }

  return `${prefix}${new Intl.NumberFormat(locale === 'zh' ? 'zh-CN' : 'en-US', {
    minimumFractionDigits: 0,
    maximumFractionDigits: 3,
  }).format(value)}`;
}

export function formatChartTooltipTokenValue(value: number, tokenUnitLocale: string) {
  return new Intl.NumberFormat(tokenUnitLocale, {
    notation: 'compact',
    maximumFractionDigits: 2,
  }).format(value);
}

export function formatChartTooltipCurrencyValue(value: number) {
  return formatUsdAmount(value);
}

export function formatUsdAmount(value: number) {
  return formatUsdValue(value, 2, 2);
}

function formatUsdValue(
  value: number,
  minimumFractionDigits: number,
  maximumFractionDigits: number,
) {
  const sign = value < 0 ? '-' : '';
  const formatted = new Intl.NumberFormat('en-US', {
    minimumFractionDigits,
    maximumFractionDigits,
  }).format(Math.abs(value));
  return `${sign}$${formatted}`;
}

export function formatCompactCurrencyValue(value: number) {
  if (Math.abs(value) >= 1000) {
    const sign = value < 0 ? '-' : '';
    return `${sign}$${(Math.abs(value) / 1000).toFixed(1).replace(/\.0$/, '')}K`;
  }

  if (Math.abs(value) >= 1) {
    return formatUsdAmount(value);
  }

  return formatUsdValue(value, 4, 4);
}

function formatShortDate(date: Date, locale: string) {
  if (locale === 'zh') {
    return `${date.getMonth() + 1}月${date.getDate()}日`;
  }

  return `${date.getMonth() + 1}/${date.getDate()}`;
}

function formatTrendBucketLabel(
  date: Date,
  locale: string,
  granularity: StatisticsGranularity | undefined,
) {
  if (granularity === 'hour') {
    return `${String(date.getHours()).padStart(2, '0')}:00`;
  }

  return formatShortDate(date, locale);
}

export function getSummaryComparisonLabelKey(period: StatisticsPeriodFilter) {
  if (period === '1d') {
    return 'statistics.metric.vsPreviousDay';
  }

  if (period === '7d') {
    return 'statistics.metric.vsPrevious7Days';
  }

  if (period === '30d') {
    return 'statistics.metric.vsPrevious30Days';
  }

  return 'statistics.metric.vsPreviousPeriod';
}

export function formatRangeLabel(range: StatisticsOverview['range'] | undefined, locale: string) {
  if (!range) {
    return locale === 'zh' ? '当前周期' : 'Current range';
  }

  const start = formatShortDate(new Date(range.startAt), locale);
  const end = formatShortDate(new Date(new Date(range.endAt).getTime() - 1), locale);
  return `${start} - ${end}`;
}

export function getDefaultCustomRange() {
  const end = new Date();
  const start = new Date(end);
  start.setDate(end.getDate() - 13);
  return {
    startDate: formatDateInputValue(start),
    endDate: formatDateInputValue(end),
  };
}

function formatDateInputValue(date: Date) {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, '0');
  const day = String(date.getDate()).padStart(2, '0');
  return `${year}-${month}-${day}`;
}

export function resolveEffectiveSourceApp(
  selectedApp: 'all' | SourceApp,
  sourceFilter: 'all' | SourceApp,
): { kind: 'all' } | { kind: 'single'; sourceApp: SourceApp } | { kind: 'conflict' } {
  if (selectedApp === 'all' && sourceFilter === 'all') {
    return { kind: 'all' };
  }

  if (selectedApp === 'all') {
    return sourceFilter === 'all' ? { kind: 'all' } : { kind: 'single', sourceApp: sourceFilter };
  }

  if (sourceFilter === 'all') {
    return { kind: 'single', sourceApp: selectedApp };
  }

  if (selectedApp === sourceFilter) {
    return { kind: 'single', sourceApp: selectedApp };
  }

  return { kind: 'conflict' };
}

export function createEmptyActivityMatrix() {
  return Array.from({ length: 7 }, () => Array.from({ length: 24 }, () => 0));
}

export function getActivityMetricView(
  activity: StatisticsOverview['activity'] | undefined,
  metric: ActivityMetric,
) {
  if (!activity) {
    return {
      matrix: createEmptyActivityMatrix(),
      maxValue: 0,
    };
  }

  if (metric === 'tokens') {
    return activity.tokens;
  }

  if (metric === 'cost') {
    return activity.cost;
  }

  return activity.sessions;
}

export function shouldRenderTrendLabel(
  index: number,
  length: number,
  granularity: StatisticsGranularity,
) {
  if (granularity === 'hour') {
    if (length === 24) {
      return index === 0 || index === length - 1 || index % 3 === 0;
    }

    if (length <= 8) {
      return true;
    }

    return index === 0 || index === length - 1 || index % 4 === 0;
  }

  if (length <= 8) {
    return true;
  }

  const step = Math.ceil(length / 8);
  return index === 0 || index === length - 1 || index % step === 0;
}

export function getTrendDirection(delta: number): TrendDirection {
  return delta < 0 ? 'down' : 'up';
}

export function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}

export function scaleSeries(values: number[], multiplier: number) {
  return values.map((value) => Math.max(4, Math.round(value * multiplier)));
}
