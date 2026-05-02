import { normalizeDisplaySourceApp } from '../../lib/sourceApps';
import { invokeCommand } from '../../lib/tauri';
import type {
  StatisticsDetailRow,
  StatisticsDistributionRow,
  StatisticsOverview,
  StatisticsQuery,
} from './statisticsTypes';

type BackendStatisticsOverview = {
  summary: StatisticsOverview['summary'];
  trend: StatisticsOverview['trend'];
  activity: StatisticsOverview['activity'];
  distribution: Array<Omit<StatisticsDistributionRow, 'app'> & { app: string }>;
  detailRows: Array<
    Omit<StatisticsDetailRow, 'app' | 'trendDirection'> & {
      app: string;
      trendDirection: string;
    }
  >;
  availableModels: string[];
  range: StatisticsOverview['range'];
};

export async function fetchStatisticsOverview(query: StatisticsQuery): Promise<StatisticsOverview> {
  const payload = {
    q: query.q?.trim() || undefined,
    sourceApp: query.sourceApp,
    model: query.model?.trim() || undefined,
    period: query.period,
    granularity: query.granularity,
    startDate: query.startDate,
    endDate: query.endDate,
  };

  const response = await invokeCommand<BackendStatisticsOverview>('statistics_get', {
    query: payload,
  });

  return {
    ...response,
    distribution: response.distribution.map((row) => ({
      ...row,
      app: normalizeDisplaySourceApp(row.app),
    })),
    detailRows: response.detailRows.map((row) => ({
      ...row,
      app: normalizeDisplaySourceApp(row.app),
      trendDirection: row.trendDirection === 'down' ? 'down' : 'up',
    })),
  };
}
