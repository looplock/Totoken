export type ModelCapability = 'coding' | 'reasoning' | 'tool_use' | 'vision' | 'long_context';

export type ModelStatus = 'active' | 'new' | 'experimental' | 'preview' | 'deprecated' | 'unknown';

export type ModelContextTier = 'compact' | 'standard' | 'extended' | 'ultra';
export type ModelPricingTier = 'economy' | 'balanced' | 'premium';

export type ModelCatalogSyncRun = {
  id: string;
  source: string;
  startedAt: string;
  endedAt?: string;
  status: 'running' | 'success' | 'failed';
  modelsSeen: number;
  modelsInserted: number;
  modelsUpdated: number;
  errorCount: number;
  errorMessage?: string;
};

export type ModelCatalogSyncStatus = {
  totalModels: number;
  latestSuccessfulSyncAt?: string;
  latestRun?: ModelCatalogSyncRun;
};

export type ModelCatalogListItem = {
  id: string;
  canonicalKey: string;
  provider: string;
  apiFamily: string;
  modelId: string;
  displayName: string;
  description?: string;
  contextWindow?: number;
  maxOutputTokens?: number;
  inputModalities: string[];
  outputModalities: string[];
  capabilities: string[];
  supportedParameters: string[];
  pricingInputUsdPerMtok?: number;
  pricingOutputUsdPerMtok?: number;
  pricingCacheReadUsdPerMtok?: number;
  pricingCacheWriteUsdPerMtok?: number;
  docsUrl?: string;
  status: string;
  rawSource: string;
  tokenUsageTotal: number;
  lastSeenAt?: string;
  contextTier: string;
  pricingTier: string;
  lastSyncedAt?: string;
};

export type ModelCatalogListResponse = {
  items: ModelCatalogListItem[];
  totalItems: number;
  syncStatus: ModelCatalogSyncStatus;
};

export type ModelCatalogListQuery = {
  q?: string;
  provider?: string;
  capability?: string;
  status?: string;
  contextTier?: string;
  pricingTier?: string;
  sortBy?: 'name' | 'provider' | 'recent' | 'usage' | 'price';
};

export type ModelRecord = {
  id: string;
  canonicalKey: string;
  modelId: string;
  name: string;
  provider: string;
  capabilities: ModelCapability[];
  contextWindow: string;
  maxOutput: string;
  inputPrice: number | null;
  outputPrice: number | null;
  lastSeenAt?: string;
  tokenUsage: number;
  description: string;
  status: ModelStatus;
  contextTier: ModelContextTier;
  pricingTier: ModelPricingTier;
  docsUrl?: string;
};
