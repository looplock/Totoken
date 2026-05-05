import type { DisplaySourceApp, SourceApp } from '../../lib/sourceApps';

export type StatisticsPeriodFilter = '1d' | '7d' | '30d' | 'custom';
export type StatisticsGranularity = 'hour' | 'day' | 'week' | 'month';
export type StatisticsTrendDirection = 'up' | 'down';

export type StatisticsMetricValue = {
  value: number;
  deltaPercent: number;
};

export type StatisticsCostMetricValue = {
  value: number;
  deltaPercent: number;
};

export type StatisticsSummary = {
  totalTokens: StatisticsMetricValue;
  inputTokens: StatisticsMetricValue;
  outputTokens: StatisticsMetricValue;
  estimatedCostUsd: StatisticsCostMetricValue;
  totalSessions: StatisticsMetricValue;
  activeModels: StatisticsMetricValue;
  avgTokensPerSession: StatisticsMetricValue;
};

export type StatisticsTrend = {
  bucketStarts: string[];
  input: number[];
  output: number[];
  total: number[];
  cacheReadInput: number[];
  cacheWriteInput: number[];
  costUsd: number[];
};

export type StatisticsActivity = {
  sessions: {
    matrix: number[][];
    maxValue: number;
  };
  tokens: {
    matrix: number[][];
    maxValue: number;
  };
  cost: {
    matrix: number[][];
    maxValue: number;
  };
};

export type StatisticsDetailRow = {
  id: string;
  app: DisplaySourceApp;
  model: string;
  sessions: number;
  inputTokens: number;
  outputTokens: number;
  estimatedCostUsd: number;
  avgTokensPerSession: number;
  lastActiveAt: string | null;
  trendPercent: number;
  trendDirection: StatisticsTrendDirection;
  sparkline: number[];
};

export type StatisticsDistributionRow = {
  app: DisplaySourceApp;
  sessions: number;
  totalTokens: number;
  estimatedCostUsd: number;
};

export type StatisticsRange = {
  period: StatisticsPeriodFilter;
  granularity: StatisticsGranularity;
  startAt: string;
  endAt: string;
};

export type StatisticsOverview = {
  summary: StatisticsSummary;
  trend: StatisticsTrend;
  activity: StatisticsActivity;
  distribution: StatisticsDistributionRow[];
  detailRows: StatisticsDetailRow[];
  availableModels: string[];
  range: StatisticsRange;
};

export type StatisticsQuery = {
  q?: string;
  sourceApp?: SourceApp;
  model?: string;
  period?: StatisticsPeriodFilter;
  granularity?: StatisticsGranularity;
  startDate?: string;
  endDate?: string;
};
