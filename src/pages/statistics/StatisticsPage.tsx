import { useDeferredValue, useEffect, useMemo, useState } from 'react';
import {
  ArrowDownRight,
  ArrowDownToLine,
  ArrowUpRight,
  ArrowUpToLine,
  Bot,
  Coins,
  Gauge,
  MessageSquare,
} from 'lucide-react';
import { InfoTooltip } from '../../components/tooltip/InfoTooltip';
import { EmptyState } from '../../components/empty-state/EmptyState';
import { useI18n } from '../../i18n/useI18n';
import { isTauriRuntime } from '../../lib/tauri';
import type { SourceApp } from '../../lib/sourceApps';
import { useEnabledSourceApps } from '../../lib/useEnabledSourceApps';
import { useTokenUnitLocale } from '../../lib/useTokenUnitLocale';
import { fetchStatisticsOverview } from './statisticsService';
import type {
  StatisticsGranularity,
  StatisticsOverview,
  StatisticsPeriodFilter,
  StatisticsSummary,
} from './statisticsTypes';
import {
  DistributionChart,
  HeatmapMatrix,
  MiniSparkline,
  ModelUsagePanel,
  TrendChart,
} from './statisticsComponents';
import { StatisticsDetailTable } from './statisticsDetailTable';
import { StatisticsFilters } from './statisticsFilters';
import {
  buildDistribution,
  buildModelUsage,
  buildPageTokens,
  buildTrendViewData,
  defaultVisibleTrendSeries,
  emptySummary,
  getActivityMetricView,
  getDefaultCustomRange,
  getDefaultGranularityForPeriod,
  getSummaryComparisonLabelKey,
  getTrendDirection,
  resolveEffectiveSourceApp,
  scaleSeries,
} from './statisticsView';
import type {
  ActivityMetric,
  DistributionMetric,
  MetricCard,
  StatisticsNotice,
  TrendSeriesKey,
} from './statisticsView';
import './StatisticsPage.css';

type StatisticsCopy = {
  loading: string;
  runtimeRequired: string;
  empty: string;
  customStart: string;
  customEnd: string;
};

function renderMetricIcon(metricKey: keyof StatisticsSummary) {
  switch (metricKey) {
    case 'totalTokens':
      return <Coins size={14} aria-hidden="true" />;
    case 'inputTokens':
      return <ArrowDownToLine size={14} aria-hidden="true" />;
    case 'outputTokens':
      return <ArrowUpToLine size={14} aria-hidden="true" />;
    case 'totalSessions':
      return <MessageSquare size={14} aria-hidden="true" />;
    case 'activeModels':
      return <Bot size={14} aria-hidden="true" />;
    case 'avgTokensPerSession':
      return <Gauge size={14} aria-hidden="true" />;
    default:
      return null;
  }
}

function resolveStatisticsNotice(notice: StatisticsNotice, copy: StatisticsCopy) {
  if (!notice) {
    return null;
  }

  if (notice.kind === 'literal') {
    return notice.text;
  }

  return copy[notice.key];
}

export function StatisticsPage() {
  const { locale, t } = useI18n();
  const enabledSourceApps = useEnabledSourceApps();
  const tokenUnitLocale = useTokenUnitLocale(locale);
  const isZh = locale === 'zh';
  const copy = useMemo<StatisticsCopy>(
    () => ({
      loading: isZh ? '正在加载统计数据...' : 'Loading statistics...',
      runtimeRequired: isZh
        ? '统计页面需要在 Tauri 桌面环境中运行。'
        : 'The statistics page requires the Tauri desktop runtime.',
      customStart: isZh ? '开始日期' : 'Start date',
      customEnd: isZh ? '结束日期' : 'End date',
      empty: t('statistics.empty'),
    }),
    [isZh, t],
  );
  const defaultCustomRange = useMemo(() => getDefaultCustomRange(), []);
  const [search, setSearch] = useState('');
  const [selectedApp, setSelectedApp] = useState<'all' | SourceApp>('all');
  const [period, setPeriod] = useState<StatisticsPeriodFilter>('1d');
  const [granularity, setGranularity] = useState<StatisticsGranularity>(
    getDefaultGranularityForPeriod('1d'),
  );
  const [modelFilter, setModelFilter] = useState('all');
  const [sourceFilter, setSourceFilter] = useState<'all' | SourceApp>('all');
  const [page, setPage] = useState(1);
  const [rowsPerPage, setRowsPerPage] = useState(25);
  const [customStartDate, setCustomStartDate] = useState(defaultCustomRange.startDate);
  const [customEndDate, setCustomEndDate] = useState(defaultCustomRange.endDate);
  const [overview, setOverview] = useState<StatisticsOverview | null>(null);
  const [loading, setLoading] = useState(true);
  const [errorNotice, setErrorNotice] = useState<StatisticsNotice>(null);
  const [hoveredDistributionIndex, setHoveredDistributionIndex] = useState<number | null>(null);
  const [distributionMetric, setDistributionMetric] = useState<DistributionMetric>('tokens');
  const [modelUsageMetric, setModelUsageMetric] = useState<DistributionMetric>('tokens');
  const [activityMetric, setActivityMetric] = useState<ActivityMetric>('sessions');
  const [visibleTrendSeries, setVisibleTrendSeries] =
    useState<Record<TrendSeriesKey, boolean>>(defaultVisibleTrendSeries);
  const deferredSearch = useDeferredValue(search);
  const appTabs = useMemo<Array<'all' | SourceApp>>(
    () => ['all', ...enabledSourceApps],
    [enabledSourceApps],
  );

  const effectiveSource = useMemo(
    () => resolveEffectiveSourceApp(selectedApp, sourceFilter),
    [selectedApp, sourceFilter],
  );
  const effectiveSourceApp =
    effectiveSource.kind === 'single' ? effectiveSource.sourceApp : undefined;
  const numberFormatter = useMemo(() => new Intl.NumberFormat(isZh ? 'zh-CN' : 'en-US'), [isZh]);
  const compactFormatter = useMemo(
    () =>
      new Intl.NumberFormat(tokenUnitLocale, {
        notation: 'compact',
        maximumFractionDigits: 1,
      }),
    [tokenUnitLocale],
  );

  useEffect(() => {
    if (period !== 'custom') {
      return;
    }

    if (customEndDate < customStartDate) {
      setCustomEndDate(customStartDate);
    }
  }, [customEndDate, customStartDate, period]);

  useEffect(() => {
    if (!isTauriRuntime()) {
      setOverview(null);
      setErrorNotice({ kind: 'copy', key: 'runtimeRequired' });
      setLoading(false);
      return;
    }

    if (effectiveSource.kind === 'conflict') {
      setOverview(null);
      setErrorNotice(null);
      setLoading(false);
      return;
    }

    let cancelled = false;

    async function load() {
      try {
        setLoading(true);
        setErrorNotice(null);
        const result = await fetchStatisticsOverview({
          q: deferredSearch.trim() || undefined,
          sourceApp: effectiveSourceApp,
          model: modelFilter !== 'all' ? modelFilter : undefined,
          period,
          granularity,
          startDate: period === 'custom' ? customStartDate : undefined,
          endDate: customEndDate,
        });

        if (cancelled) {
          return;
        }

        setOverview(result);
      } catch (error) {
        if (cancelled) {
          return;
        }

        setOverview(null);
        setErrorNotice(
          error instanceof Error
            ? { kind: 'literal', text: error.message }
            : { kind: 'copy', key: 'empty' },
        );
      } finally {
        if (!cancelled) {
          setLoading(false);
        }
      }
    }

    void load();

    return () => {
      cancelled = true;
    };
  }, [
    customEndDate,
    customStartDate,
    deferredSearch,
    effectiveSource,
    effectiveSourceApp,
    granularity,
    modelFilter,
    period,
  ]);

  useEffect(() => {
    if (modelFilter !== 'all' && !overview?.availableModels.includes(modelFilter)) {
      setModelFilter('all');
    }
  }, [modelFilter, overview?.availableModels]);

  useEffect(() => {
    if (selectedApp !== 'all' && !enabledSourceApps.includes(selectedApp)) {
      setSelectedApp('all');
    }
    if (sourceFilter !== 'all' && !enabledSourceApps.includes(sourceFilter)) {
      setSourceFilter('all');
    }
  }, [enabledSourceApps, selectedApp, sourceFilter]);

  const detailRows = overview?.detailRows ?? [];
  const errorMessage = resolveStatisticsNotice(errorNotice, copy);
  const totalItems = detailRows.length;
  const totalPages = Math.max(1, Math.ceil(totalItems / rowsPerPage));
  const currentPage = Math.min(page, totalPages);
  const trendView = useMemo(
    () => buildTrendViewData(overview?.trend, locale, overview?.range.granularity),
    [locale, overview?.range.granularity, overview?.trend],
  );
  const summary = overview?.summary ?? emptySummary;
  const totalSessions = summary.totalSessions.value;
  const totalTokens = summary.totalTokens.value;
  const totalEstimatedCost = detailRows.reduce((sum, row) => sum + row.estimatedCostUsd, 0);
  const activityView = getActivityMetricView(overview?.activity, activityMetric);
  const distributionItems = buildDistribution(
    overview?.distribution ?? [],
    distributionMetric,
    distributionMetric === 'sessions'
      ? totalSessions
      : distributionMetric === 'cost'
        ? totalEstimatedCost
        : totalTokens,
    t,
    enabledSourceApps,
  );
  const modelUsageTotal =
    modelUsageMetric === 'sessions'
      ? totalSessions
      : modelUsageMetric === 'cost'
        ? totalEstimatedCost
        : totalTokens;
  const modelUsageItems = buildModelUsage(detailRows, modelUsageTotal, modelUsageMetric);
  const pageTokens = buildPageTokens(currentPage, totalPages);
  const availableModels = overview?.availableModels ?? [];
  const pagedRows = detailRows.slice((currentPage - 1) * rowsPerPage, currentPage * rowsPerPage);
  const summaryComparisonLabel = t(getSummaryComparisonLabelKey(period));
  const sectionInfoLabel = isZh ? '查看说明' : 'Show section info';
  const trendLegendItems: Array<{ key: TrendSeriesKey; label: string; className: string }> = [
    {
      key: 'cost',
      label: t('statistics.chart.cost'),
      className: 'statistics-legend-cost',
    },
    {
      key: 'cache-read',
      label: t('messagePage.chip.cacheRead'),
      className: 'statistics-legend-cache-read',
    },
    {
      key: 'cache-write',
      label: t('messagePage.chip.cacheWrite'),
      className: 'statistics-legend-cache-write',
    },
    {
      key: 'input',
      label: t('statistics.chart.input'),
      className: 'statistics-legend-input',
    },
    {
      key: 'output',
      label: t('statistics.chart.output'),
      className: 'statistics-legend-output',
    },
    {
      key: 'total',
      label: t('statistics.chart.total'),
      className: 'statistics-legend-total',
    },
  ];
  const metricCards: MetricCard[] = [
    {
      key: 'totalTokens',
      labelKey: 'statistics.metric.totalTokens',
      value: compactFormatter.format(summary.totalTokens.value),
      delta: summary.totalTokens.deltaPercent,
      direction: getTrendDirection(summary.totalTokens.deltaPercent),
      sparkline: trendView.total.slice(-8),
    },
    {
      key: 'inputTokens',
      labelKey: 'statistics.metric.inputTokens',
      value: compactFormatter.format(summary.inputTokens.value),
      delta: summary.inputTokens.deltaPercent,
      direction: getTrendDirection(summary.inputTokens.deltaPercent),
      sparkline: trendView.input.slice(-8),
    },
    {
      key: 'outputTokens',
      labelKey: 'statistics.metric.outputTokens',
      value: compactFormatter.format(summary.outputTokens.value),
      delta: summary.outputTokens.deltaPercent,
      direction: getTrendDirection(summary.outputTokens.deltaPercent),
      sparkline: trendView.output.slice(-8),
    },
    {
      key: 'totalSessions',
      labelKey: 'statistics.metric.totalSessions',
      value: numberFormatter.format(summary.totalSessions.value),
      delta: summary.totalSessions.deltaPercent,
      direction: getTrendDirection(summary.totalSessions.deltaPercent),
      sparkline: scaleSeries(trendView.total.slice(-8), 0.00016),
    },
    {
      key: 'activeModels',
      labelKey: 'statistics.metric.activeModels',
      value: numberFormatter.format(summary.activeModels.value),
      delta: summary.activeModels.deltaPercent,
      direction: getTrendDirection(summary.activeModels.deltaPercent),
      sparkline: scaleSeries(trendView.output.slice(-8), 0.00003),
    },
    {
      key: 'avgTokensPerSession',
      labelKey: 'statistics.metric.avgTokensPerSession',
      value: numberFormatter.format(summary.avgTokensPerSession.value),
      delta: summary.avgTokensPerSession.deltaPercent,
      direction: getTrendDirection(summary.avgTokensPerSession.deltaPercent),
      sparkline: scaleSeries(trendView.total.slice(-8), 0.015),
    },
  ];

  useEffect(() => {
    if (page > totalPages) {
      setPage(totalPages);
    }
  }, [page, totalPages]);

  return (
    <div className="statistics-page">
      <section className="statistics-hero">
        <div>
          <h1 className="page-title">{t('statistics.title')}</h1>
          <p className="page-subtitle">{t('statistics.subtitle')}</p>
        </div>
      </section>

      <StatisticsFilters
        locale={locale}
        t={t}
        copy={copy}
        search={search}
        selectedApp={selectedApp}
        period={period}
        granularity={granularity}
        modelFilter={modelFilter}
        sourceFilter={sourceFilter}
        customStartDate={customStartDate}
        customEndDate={customEndDate}
        appTabs={appTabs}
        enabledSourceApps={enabledSourceApps}
        availableModels={availableModels}
        range={overview?.range}
        onSearchChange={setSearch}
        onSelectedAppChange={setSelectedApp}
        onPeriodChange={setPeriod}
        onGranularityChange={setGranularity}
        onModelFilterChange={setModelFilter}
        onSourceFilterChange={setSourceFilter}
        onCustomStartDateChange={setCustomStartDate}
        onCustomEndDateChange={setCustomEndDate}
        onResetPage={() => setPage(1)}
      />

      {errorMessage ? <section className="statistics-notice">{errorMessage}</section> : null}

      {loading && !overview ? (
        <EmptyState>{copy.loading}</EmptyState>
      ) : (
        <>
          <section className="statistics-summary-grid">
            {metricCards.map((card) => (
              <article key={card.key} className="statistics-metric-card">
                <div className="statistics-metric-top">
                  <span className="statistics-metric-icon">{renderMetricIcon(card.key)}</span>
                  <span className="statistics-metric-label">{t(card.labelKey)}</span>
                </div>
                <div className="statistics-metric-value">{card.value}</div>
                <div className="statistics-metric-footer">
                  <span
                    className={
                      card.direction === 'down'
                        ? 'statistics-metric-delta statistics-metric-delta-down'
                        : 'statistics-metric-delta'
                    }
                  >
                    {card.direction === 'down' ? (
                      <ArrowDownRight size={14} />
                    ) : (
                      <ArrowUpRight size={14} />
                    )}
                    {Math.abs(card.delta).toFixed(1)}%
                  </span>
                  <span>{summaryComparisonLabel}</span>
                  <MiniSparkline values={card.sparkline} />
                </div>
              </article>
            ))}
          </section>

          <section className="statistics-trend-row">
            <article className="statistics-card statistics-card-trend">
              <header className="statistics-card-header">
                <div className="statistics-card-title">
                  <h2>{t('statistics.section.trend')}</h2>
                  <InfoTooltip label={sectionInfoLabel} content={t('statistics.info.trend')} />
                </div>
                <div className="statistics-chart-legend">
                  {trendLegendItems.map((item) => (
                    <button
                      key={item.key}
                      type="button"
                      aria-pressed={visibleTrendSeries[item.key]}
                      className={
                        visibleTrendSeries[item.key]
                          ? `statistics-legend-item ${item.className}`
                          : `statistics-legend-item ${item.className} statistics-legend-item-hidden`
                      }
                      onClick={() =>
                        setVisibleTrendSeries((current) => ({
                          ...current,
                          [item.key]: !current[item.key],
                        }))
                      }
                    >
                      {item.label}
                    </button>
                  ))}
                </div>
              </header>
              <TrendChart
                locale={locale}
                granularity={overview?.range.granularity ?? granularity}
                labels={trendView.labels}
                cacheRead={trendView.cacheReadInput}
                cacheWrite={trendView.cacheWriteInput}
                input={trendView.input}
                output={trendView.output}
                total={trendView.total}
                cost={trendView.costUsd}
                visibleSeries={visibleTrendSeries}
                tokenUnitLocale={tokenUnitLocale}
                t={t}
              />
            </article>
          </section>

          <section className="statistics-secondary-grid">
            <article className="statistics-card statistics-activity-card">
              <header className="statistics-card-header">
                <div className="statistics-card-title">
                  <h2>{t('statistics.section.activity')}</h2>
                  <InfoTooltip label={sectionInfoLabel} content={t('statistics.info.activity')} />
                </div>
                <div className="statistics-card-header-tools">
                  <div
                    className="statistics-segmented statistics-segmented-compact"
                    role="tablist"
                    aria-label={t('statistics.section.activity')}
                  >
                    <button
                      type="button"
                      className={
                        activityMetric === 'sessions' ? 'statistics-segmented-active' : undefined
                      }
                      onClick={() => setActivityMetric('sessions')}
                    >
                      {t('statistics.activity.sessions')}
                    </button>
                    <button
                      type="button"
                      className={
                        activityMetric === 'tokens' ? 'statistics-segmented-active' : undefined
                      }
                      onClick={() => setActivityMetric('tokens')}
                    >
                      {t('statistics.activity.tokens')}
                    </button>
                    <button
                      type="button"
                      className={
                        activityMetric === 'cost' ? 'statistics-segmented-active' : undefined
                      }
                      onClick={() => setActivityMetric('cost')}
                    >
                      {t('statistics.activity.cost')}
                    </button>
                  </div>
                </div>
              </header>
              <HeatmapMatrix
                locale={locale}
                metric={activityMetric}
                matrix={activityView.matrix}
                maxValue={activityView.maxValue}
              />
            </article>

            <article className="statistics-card statistics-secondary-card statistics-secondary-card-distribution">
              <header className="statistics-card-header">
                <div className="statistics-card-title">
                  <h2>{t('statistics.section.distribution')}</h2>
                  <InfoTooltip
                    label={sectionInfoLabel}
                    content={t('statistics.info.distribution')}
                  />
                </div>
                <div className="statistics-card-header-tools">
                  <div
                    className="statistics-segmented statistics-segmented-compact"
                    role="tablist"
                    aria-label={t('statistics.section.distribution')}
                  >
                    <button
                      type="button"
                      className={
                        distributionMetric === 'sessions'
                          ? 'statistics-segmented-active'
                          : undefined
                      }
                      onClick={() => setDistributionMetric('sessions')}
                    >
                      {t('statistics.distribution.sessions')}
                    </button>
                    <button
                      type="button"
                      className={
                        distributionMetric === 'tokens' ? 'statistics-segmented-active' : undefined
                      }
                      onClick={() => setDistributionMetric('tokens')}
                    >
                      {t('statistics.distribution.tokens')}
                    </button>
                    <button
                      type="button"
                      className={
                        distributionMetric === 'cost' ? 'statistics-segmented-active' : undefined
                      }
                      onClick={() => setDistributionMetric('cost')}
                    >
                      {t('statistics.distribution.cost')}
                    </button>
                  </div>
                </div>
              </header>
              <DistributionChart
                totalSessions={totalSessions}
                totalTokens={totalTokens}
                totalEstimatedCost={totalEstimatedCost}
                locale={locale}
                tokenUnitLocale={tokenUnitLocale}
                metric={distributionMetric}
                label={
                  distributionMetric === 'sessions'
                    ? t('statistics.metric.totalSessions')
                    : distributionMetric === 'cost'
                      ? t('statistics.chart.cost')
                      : t('statistics.metric.totalTokens')
                }
                items={distributionItems}
                hoveredDistributionIndex={hoveredDistributionIndex}
                setHoveredDistributionIndex={setHoveredDistributionIndex}
              />
            </article>

            <article className="statistics-card statistics-secondary-card">
              <header className="statistics-card-header statistics-card-header-inline">
                <div className="statistics-card-title">
                  <h2>{t('statistics.section.modelUsage')}</h2>
                  <InfoTooltip label={sectionInfoLabel} content={t('statistics.info.modelUsage')} />
                </div>
                <div className="statistics-card-header-tools">
                  <div
                    className="statistics-segmented statistics-segmented-compact"
                    role="tablist"
                    aria-label={t('statistics.section.modelUsage')}
                  >
                    <button
                      type="button"
                      className={
                        modelUsageMetric === 'sessions' ? 'statistics-segmented-active' : undefined
                      }
                      onClick={() => setModelUsageMetric('sessions')}
                    >
                      {t('statistics.distribution.sessions')}
                    </button>
                    <button
                      type="button"
                      className={
                        modelUsageMetric === 'tokens' ? 'statistics-segmented-active' : undefined
                      }
                      onClick={() => setModelUsageMetric('tokens')}
                    >
                      {t('statistics.distribution.tokens')}
                    </button>
                    <button
                      type="button"
                      className={
                        modelUsageMetric === 'cost' ? 'statistics-segmented-active' : undefined
                      }
                      onClick={() => setModelUsageMetric('cost')}
                    >
                      {t('statistics.distribution.cost')}
                    </button>
                  </div>
                </div>
              </header>
              <ModelUsagePanel
                items={modelUsageItems}
                locale={locale}
                metric={modelUsageMetric}
                compactFormatter={compactFormatter}
                numberFormatter={numberFormatter}
              />
            </article>
          </section>

          <StatisticsDetailTable
            loading={loading}
            loadingLabel={copy.loading}
            sectionInfoLabel={sectionInfoLabel}
            rows={pagedRows}
            rowsPerPage={rowsPerPage}
            currentPage={currentPage}
            totalPages={totalPages}
            pageTokens={pageTokens}
            locale={locale}
            numberFormatter={numberFormatter}
            t={t}
            onRowsPerPageChange={(value) => {
              setRowsPerPage(value);
              setPage(1);
            }}
            onPageChange={setPage}
          />
        </>
      )}
    </div>
  );
}
