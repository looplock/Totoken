import {
  normalizeDisplaySourceApp,
  normalizeSourceApp,
  type SourceApp,
} from '../../lib/sourceApps';
import { invokeCommand } from '../../lib/tauri';
import type { SessionRecord, SessionSourceState } from './sessionData';

export type SessionSortField =
  | 'name'
  | 'sourceApp'
  | 'model'
  | 'inputTokens'
  | 'outputTokens'
  | 'totalTokens'
  | 'estimatedCostUsd'
  | 'lastUpdated'
  | 'messages'
  | 'sourceState';

export type SessionSortDirection = 'asc' | 'desc';

export type SessionsListQuery = {
  page: number;
  pageSize: number;
  q?: string;
  sourceApps?: SourceApp[];
  sourceStates?: SessionSourceState[];
  sortBy: SessionSortField;
  sortOrder: SessionSortDirection;
};

export type SessionsListResult = {
  items: SessionRecord[];
  totalItems: number;
  totalPages: number;
};

type BackendSessionListItem = {
  id: string;
  name: string;
  sourceApp: string;
  model: string;
  inputTokens: number;
  outputTokens: number;
  totalTokens: number;
  estimatedCostUsd: number | null;
  lastUpdated: string;
  messages: number;
  sourceState: string;
};

type BackendSessionsListResponse = {
  items: BackendSessionListItem[];
  pagination: {
    page: number;
    pageSize: number;
    totalItems: number;
    totalPages: number;
  };
};

export async function fetchSessionsList(query: SessionsListQuery): Promise<SessionsListResult> {
  if (query.sourceApps && query.sourceApps.length === 0) {
    return {
      items: [],
      totalItems: 0,
      totalPages: 1,
    };
  }

  const payload = {
    page: query.page,
    pageSize: query.pageSize,
    q: query.q?.trim() || undefined,
    sourceApps: query.sourceApps?.length ? query.sourceApps : undefined,
    sourceStates: query.sourceStates?.length ? query.sourceStates : undefined,
    sortBy: query.sortBy,
    sortOrder: query.sortOrder,
  };

  const response = await invokeCommand<BackendSessionsListResponse>('sessions_list', {
    query: payload,
  });

  return {
    items: response.items.map(mapSessionRecord),
    totalItems: response.pagination.totalItems,
    totalPages: response.pagination.totalPages,
  };
}

type BackendSourceScanTarget = {
  app: string;
  rootPath: string;
  scanPaths: Array<{
    path: string;
    exists: boolean;
  }>;
  enabled: boolean;
  scanSupported: boolean;
};

type BackendSourcesListResponse = {
  items: BackendSourceScanTarget[];
};

export async function rescanEnabledSources(sourceApp?: SourceApp): Promise<void> {
  const response = await invokeCommand<BackendSourcesListResponse>('sources_list');
  const targets = response.items.filter((item) => {
    if (!item.enabled || !item.scanSupported) {
      return false;
    }

    return sourceApp ? normalizeSourceApp(item.app) === sourceApp : true;
  });

  if (targets.length === 0) {
    throw new Error('No enabled scan sources are available.');
  }

  for (const target of targets) {
    const fallbackPath =
      target.rootPath.trim() ||
      target.scanPaths.find((item) => item.path.trim().length > 0)?.path?.trim() ||
      '.';

    await invokeCommand<void>('start_scan', {
      path: fallbackPath,
      sourceApp: target.app,
    });
  }
}

function mapSessionRecord(item: BackendSessionListItem): SessionRecord {
  return {
    id: item.id,
    name: item.name,
    sourceApp: normalizeDisplaySourceApp(item.sourceApp),
    model: item.model,
    inputTokens: item.inputTokens,
    outputTokens: item.outputTokens,
    totalTokens: item.totalTokens,
    estimatedCostUsd: item.estimatedCostUsd,
    lastUpdated: item.lastUpdated,
    messages: item.messages,
    sourceState: normalizeSourceState(item.sourceState),
  };
}

function normalizeSourceState(value: string): SessionSourceState {
  if (value === 'synced' || value === 'archived' || value === 'deleted') {
    return value;
  }

  return 'missing';
}
