import type { DisplaySourceApp } from '../../lib/sourceApps';

export type SessionSourceState = 'synced' | 'archived' | 'deleted' | 'missing';

export type SessionRecord = {
  id: string;
  name: string;
  sourceApp: DisplaySourceApp;
  model: string;
  inputTokens: number;
  outputTokens: number;
  totalTokens: number;
  estimatedCostUsd: number | null;
  tokenConfidence?: 'high' | 'low';
  lastUpdated: string;
  messages: number;
  sourceState: SessionSourceState;
};
