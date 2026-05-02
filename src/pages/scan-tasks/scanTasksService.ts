import { invokeCommand } from '../../lib/tauri';
import type { ScanRunRecord, ScanRunStatus, ScanRunTriggerType } from './scanTasksData';

type BackendScanRunRecord = {
  id: string;
  triggerType: string;
  status: string;
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

type BackendScanRecordsListResponse = {
  items: BackendScanRunRecord[];
};

export async function fetchScanRecords(limit = 18): Promise<{ items: ScanRunRecord[] }> {
  const response = await invokeCommand<BackendScanRecordsListResponse>('scan_records_list', {
    query: { limit },
  });

  return {
    items: response.items.map(mapScanRunRecord),
  };
}

function mapScanRunRecord(item: BackendScanRunRecord): ScanRunRecord {
  return {
    id: item.id,
    triggerType: normalizeTriggerType(item.triggerType),
    status: normalizeStatus(item.status),
    startedAt: item.startedAt,
    endedAt: item.endedAt,
    durationMs: item.durationMs,
    filesSeen: item.filesSeen,
    filesParsed: item.filesParsed,
    filesSkipped: item.filesSkipped,
    filesFailed: item.filesFailed,
    sessionsChanged: item.sessionsChanged,
    errorCount: item.errorCount,
  };
}

function normalizeTriggerType(value: string): ScanRunTriggerType {
  if (value === 'manual' || value === 'auto') {
    return value;
  }

  return 'other';
}

function normalizeStatus(value: string): ScanRunStatus {
  if (value === 'running' || value === 'completed' || value === 'failed') {
    return value;
  }

  return 'unknown';
}
