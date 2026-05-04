export type DashboardSection = 'scanner' | 'statistics';

export type DashboardScannerSnapshot = {
  lastRunAt: string | null;
  status: 'running' | 'completed' | 'failed' | 'unknown' | null;
  errorCount: number;
};

export type DashboardKpiDelta = {
  value: number;
  deltaPercent: number | null;
  sparkline: number[];
};

export type DashboardKpiCostDelta = {
  valueUsd: number;
  deltaPercent: number | null;
  sparkline: number[];
};

export type DashboardKpi = {
  todayTokens: DashboardKpiDelta;
  todaySessions: DashboardKpiDelta;
  todayCostUsd: DashboardKpiCostDelta;
};

export type DashboardTrendPoint = {
  bucketStart: string;
  tokens: number;
  inputTokens: number;
  outputTokens: number;
  costUsd: number;
};

export type DashboardTopRow = {
  label: string;
  value: number;
  share: number;
};

export type DashboardViewModel = {
  scanner: DashboardScannerSnapshot;
  kpi: DashboardKpi;
  trendToday: DashboardTrendPoint[];
  topModels: DashboardTopRow[];
  topApps: DashboardTopRow[];
  partialFailures: DashboardSection[];
};
