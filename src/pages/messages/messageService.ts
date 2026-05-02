import { normalizeDisplaySourceApp, type DisplaySourceApp } from '../../lib/sourceApps';
import { invokeCommand } from '../../lib/tauri';

export type RequestRecord = {
  id: string;
  sessionId: string;
  sessionName: string;
  sourceApp: DisplaySourceApp;
  sequenceNo: number;
  status: string | null;
  messageCount: number;
  model: string | null;
  inputTokens: number | null;
  outputTokens: number | null;
  totalTokens: number | null;
  cacheReadInputTokens: number | null;
  cacheWriteInputTokens: number | null;
  estimatedCostUsd: number | null;
  tokenConfidence: string | null;
  createdAt: string | null;
  updatedAt: string | null;
  sourceLocatorLabel: string;
};

export type UsageEventRecord = {
  id: string;
  sessionId: string;
  eventTimeUtc: string;
  sourceApp: DisplaySourceApp;
  model: string | null;
  deltaInput: number;
  deltaOutput: number;
  deltaTotal: number;
  cacheReadInputTokens: number;
  cacheWriteInputTokens: number;
  estimatedCostUsd: number | null;
  granularity: string;
  confidence: string;
  sourceEventId: string | null;
  epochNo: number;
};

export type MessageSessionSummary = {
  sessionId: string;
  sessionName: string;
  sourceApp: DisplaySourceApp;
  sessionTotalMessages: number;
  sessionInputTokens: number;
  sessionOutputTokens: number;
  sessionTotalTokens: number;
  sessionCacheReadInputTokens: number;
  sessionCacheWriteInputTokens: number;
  sessionEstimatedCostUsd: number | null;
  sessionLastUpdated: string;
  sessionSourceState: 'synced' | 'archived' | 'deleted' | 'missing';
};

export type SessionMessagesResult = {
  session: MessageSessionSummary | null;
  requests: RequestRecord[];
  usageEvents: UsageEventRecord[];
};

export type SessionMessagesQuery = {
  sessionId: string;
};

type BackendRequest = Omit<RequestRecord, 'sourceApp'> & {
  sourceApp: string;
};

type BackendMessageSessionSummary = Omit<
  MessageSessionSummary,
  'sourceApp' | 'sessionSourceState'
> & {
  sourceApp: string;
  sessionSourceState: string;
};

type BackendUsageEvent = Omit<UsageEventRecord, 'sourceApp'> & {
  sourceApp: string;
};

type BackendMessagesListResponse = {
  session: BackendMessageSessionSummary | null;
  requests: BackendRequest[];
  usageEvents: BackendUsageEvent[];
};

export async function fetchSessionMessages(
  query: SessionMessagesQuery,
): Promise<SessionMessagesResult> {
  const response = await invokeCommand<BackendMessagesListResponse>('messages_list', {
    query: {
      sessionId: query.sessionId,
    },
  });

  return {
    session: response.session ? mapSessionSummary(response.session) : null,
    requests: response.requests.map(mapRequestRecord),
    usageEvents: response.usageEvents.map(mapUsageEventRecord),
  };
}

export async function ensureSessionMessagesIndexed(sessionId: string): Promise<boolean> {
  return invokeCommand<boolean>('messages_ensure_session_index', { sessionId });
}

function mapRequestRecord(item: BackendRequest): RequestRecord {
  return {
    ...item,
    sourceApp: normalizeDisplaySourceApp(item.sourceApp),
  };
}

function mapSessionSummary(item: BackendMessageSessionSummary): MessageSessionSummary {
  return {
    ...item,
    sourceApp: normalizeDisplaySourceApp(item.sourceApp),
    sessionSourceState: normalizeSourceState(item.sessionSourceState),
  };
}

function mapUsageEventRecord(item: BackendUsageEvent): UsageEventRecord {
  return {
    ...item,
    sourceApp: normalizeDisplaySourceApp(item.sourceApp),
  };
}

function normalizeSourceState(value: string): 'synced' | 'archived' | 'deleted' | 'missing' {
  if (value === 'synced' || value === 'archived' || value === 'deleted') {
    return value;
  }

  return 'missing';
}
