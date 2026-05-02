export type ScanRunStatus = 'running' | 'completed' | 'failed' | 'unknown';
export type ScanRunTriggerType = 'manual' | 'auto' | 'other';

export type ScanRunRecord = {
  id: string;
  triggerType: ScanRunTriggerType;
  status: ScanRunStatus;
  startedAt: string;
  endedAt: string | null;
  durationMs: number | null;
  filesSeen: number;
  filesParsed: number;
  filesSkipped: number;
  filesFailed: number;
  sessionsChanged: number;
  errorCount: number;
};
