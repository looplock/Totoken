import { DEFAULT_THEME, type ThemeMode } from '../../theme/themes';

export type { ThemeMode } from '../../theme/themes';

export type ScanMode = 'auto' | 'manual';
export type CloseAction = 'quit' | 'tray';

export type StorageConfig = {
  bootstrapDir: string;
  dataDir: string;
  dbPath: string;
  usingDefaultDir: boolean;
  restartRequired: boolean;
  pendingCleanupPath: string | null;
};

export type SettingsState = {
  scheduler: {
    scanMode: ScanMode;
    baseInterval: number;
    minInterval: number;
    maxInterval: number;
    adaptiveScanning: boolean;
    ewmaAlpha: number;
    changeRateThreshold: number;
  };
  uiPreferences: {
    theme: ThemeMode;
    language: 'zh-CN' | 'en-US';
    notifications: boolean;
    localizedTokenUnits: boolean;
    closeAction: CloseAction;
  };
};

export type ScanSummary = {
  triggerType: string;
  rootPath: string;
  filesSeen: number;
  filesParsed: number;
  filesSkipped: number;
  filesFailed: number;
  sessionsChanged: number;
  errorCount: number;
};

export type AutoScanStatus = {
  sourceApp: string;
  rootPath: string;
  scannerBusy: boolean;
  activeTriggerType: string | null;
  isAutoScanRunning: boolean;
  currentIntervalSeconds: number | null;
  nextScanAt: string | null;
  lastScanStartedAt: string | null;
  lastScanEndedAt: string | null;
  lastSuccessfulScanAt: string | null;
  lastError: string | null;
  currentEwmaChangeRatePercent: number | null;
  consecutiveIdleRuns: number;
  lastSummary: ScanSummary | null;
};

export type SchedulerPreview = {
  series: number[];
  threshold: number;
  unit: string;
  basedOnLiveTelemetry: boolean;
};

export function createDefaultSettings(): SettingsState {
  return {
    scheduler: {
      scanMode: 'auto',
      baseInterval: 60,
      minInterval: 15,
      maxInterval: 300,
      adaptiveScanning: true,
      ewmaAlpha: 0.3,
      changeRateThreshold: 5,
    },
    uiPreferences: {
      theme: DEFAULT_THEME,
      language: 'en-US',
      notifications: true,
      localizedTokenUnits: true,
      closeAction: 'quit',
    },
  };
}
