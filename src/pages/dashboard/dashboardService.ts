import { invokeCommand } from '../../lib/tauri';
import type { ScanRunRecord } from '../scan-tasks/scanTasksData';
import type {
  StatisticsDetailRow,
  StatisticsDistributionRow,
  StatisticsOverview,
} from '../statistics/statisticsTypes';
import type {
  DashboardKpi,
  DashboardKpiCostDelta,
  DashboardKpiDelta,
  DashboardScannerSnapshot,
  DashboardSection,
  DashboardTopRow,
  DashboardTrendPoint,
  DashboardViewModel,
} from './dashboardTypes';

const TOP_N = 5;

type BackendScanRecordsResponse = {
  items: Array<
    Omit<ScanRunRecord, 'status' | 'triggerType'> & { status: string; triggerType: string }
  >;
};

export async function loadDashboard(): Promise<DashboardViewModel> {
  const partialFailures: DashboardSection[] = [];
  const settled = await Promise.allSettled([
    invokeCommand<BackendScanRecordsResponse>('scan_records_list', {
      query: { limit: 1 },
    }),
    invokeCommand<StatisticsOverview>('statistics_get', {
      query: { period: '1d', granularity: 'hour' },
    }),
  ]);

  const [scanRecordsResult, statisticsResult] = settled;

  const scanner =
    scanRecordsResult.status === 'fulfilled'
      ? mapScanner(scanRecordsResult.value)
      : (recordFailure(partialFailures, 'scanner'), emptyScanner());

  const statistics =
    statisticsResult.status === 'fulfilled'
      ? statisticsResult.value
      : (recordFailure(partialFailures, 'statistics'), null);

  const kpi = buildKpi(statistics);
  const trendToday = statistics ? buildTrend(statistics) : [];
  const topModels = statistics ? buildTopModels(statistics.detailRows) : [];
  const topApps = statistics ? buildTopApps(statistics.distribution) : [];

  return {
    scanner,
    kpi,
    trendToday,
    topModels,
    topApps,
    partialFailures,
  };
}

function recordFailure(list: DashboardSection[], section: DashboardSection) {
  if (!list.includes(section)) list.push(section);
}

function mapScanner(response: BackendScanRecordsResponse): DashboardScannerSnapshot {
  const last = response.items[0];
  if (!last) return emptyScanner();
  return {
    lastRunAt: last.endedAt ?? last.startedAt,
    status: normalizeScanStatus(last.status),
    errorCount: last.errorCount ?? 0,
  };
}

function emptyScanner(): DashboardScannerSnapshot {
  return { lastRunAt: null, status: null, errorCount: 0 };
}

function normalizeScanStatus(value: string): DashboardScannerSnapshot['status'] {
  if (value === 'running' || value === 'completed' || value === 'failed') return value;
  return 'unknown';
}

function buildKpi(statistics: StatisticsOverview | null): DashboardKpi {
  const todayTokens: DashboardKpiDelta = statistics
    ? {
        value: statistics.summary.totalTokens.value,
        deltaPercent: normalizeDelta(statistics.summary.totalTokens.deltaPercent),
        sparkline: statistics.trend.total,
      }
    : { value: 0, deltaPercent: null, sparkline: [] };

  const todaySessions: DashboardKpiDelta = statistics
    ? {
        value: statistics.summary.totalSessions.value,
        deltaPercent: normalizeDelta(statistics.summary.totalSessions.deltaPercent),
        sparkline: buildSessionSparkline(statistics),
      }
    : { value: 0, deltaPercent: null, sparkline: [] };

  const todayCostUsd: DashboardKpiCostDelta = {
    valueUsd: statistics?.summary.estimatedCostUsd.value ?? 0,
    deltaPercent: statistics
      ? normalizeDelta(statistics.summary.estimatedCostUsd.deltaPercent)
      : null,
    sparkline: statistics?.trend.costUsd ?? [],
  };

  return { todayTokens, todaySessions, todayCostUsd };
}

function normalizeDelta(value: number | null | undefined): number | null {
  if (value === null || value === undefined) return null;
  if (!Number.isFinite(value)) return null;
  return value;
}

function buildSessionSparkline(statistics: StatisticsOverview): number[] {
  const matrix = statistics.activity.sessions.matrix;
  return statistics.trend.bucketStarts.map((bucketStart) => {
    const date = new Date(bucketStart);
    const dayIndex = (date.getDay() + 6) % 7;
    const hourIndex = date.getHours();
    return matrix[dayIndex]?.[hourIndex] ?? 0;
  });
}

function buildTrend(statistics: StatisticsOverview): DashboardTrendPoint[] {
  const { bucketStarts, input, output, total, costUsd } = statistics.trend;
  const length = Math.min(
    bucketStarts.length,
    input.length,
    output.length,
    total.length,
    costUsd.length,
  );
  const points: DashboardTrendPoint[] = [];
  for (let i = 0; i < length; i += 1) {
    points.push({
      bucketStart: bucketStarts[i],
      tokens: total[i] ?? 0,
      inputTokens: input[i] ?? 0,
      outputTokens: output[i] ?? 0,
      costUsd: costUsd[i] ?? 0,
    });
  }
  return points;
}

function buildTopModels(detailRows: StatisticsDetailRow[]): DashboardTopRow[] {
  const tally = new Map<string, number>();
  let total = 0;
  for (const row of detailRows) {
    const key = row.model || '(unknown)';
    const tokens = (row.inputTokens ?? 0) + (row.outputTokens ?? 0);
    total += tokens;
    tally.set(key, (tally.get(key) ?? 0) + tokens);
  }
  return finishTop(tally, total);
}

function buildTopApps(distribution: StatisticsDistributionRow[]): DashboardTopRow[] {
  const tally = new Map<string, number>();
  let total = 0;
  for (const row of distribution) {
    const tokens = row.totalTokens ?? 0;
    total += tokens;
    tally.set(row.app, (tally.get(row.app) ?? 0) + tokens);
  }
  return finishTop(tally, total);
}

function finishTop(tally: Map<string, number>, total: number): DashboardTopRow[] {
  const entries = Array.from(tally.entries())
    .filter(([, value]) => value > 0)
    .sort((a, b) => b[1] - a[1])
    .slice(0, TOP_N);
  if (total <= 0) return [];
  return entries.map(([label, value]) => ({
    label,
    value,
    share: value / total,
  }));
}
