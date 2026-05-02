import { memo, useDeferredValue, useEffect, useMemo, useRef, useState } from 'react';
import {
  ChevronDown,
  Eye,
  LayoutGrid,
  List,
  Palette,
  RefreshCw,
  Search,
  Sparkles,
  Square,
} from 'lucide-react';
import {
  ModelProviderIcon,
  type ModelProviderIconTheme,
} from '../../components/model-provider-icon/ModelProviderIcon';
import { EmptyState } from '../../components/empty-state/EmptyState';
import { useI18n } from '../../i18n/useI18n';
import { fetchModelCatalog, refreshModelCatalog } from './modelsService';
import type {
  ModelCapability,
  ModelCatalogListItem,
  ModelContextTier,
  ModelPricingTier,
  ModelRecord,
  ModelStatus,
} from './modelTypes';
import './ModelsPage.css';

type ViewMode = 'card' | 'list';
type SortMode = 'name' | 'provider' | 'recent' | 'usage' | 'price';

type CopyBundle = {
  title: string;
  subtitle: string;
  searchPlaceholder: string;
  sortByName: string;
  sortByProvider: string;
  sortByRecent: string;
  sortByUsage: string;
  sortByPrice: string;
  modelsCount: (count: number) => string;
  viewEnabled: (mode: ViewMode) => string;
  sync: string;
  syncing: string;
  syncSuccess: (count: number) => string;
  syncFailed: string;
  cardView: string;
  listView: string;
  monoIcons: string;
  logoIcons: string;
  brandIcons: string;
  iconThemeEnabled: (theme: ModelProviderIconTheme) => string;
  allProviders: string;
  allCapabilities: string;
  allStatuses: string;
  allContexts: string;
  allPricing: string;
  contextCompact: string;
  contextStandard: string;
  contextExtended: string;
  contextUltra: string;
  pricingEconomy: string;
  pricingBalanced: string;
  pricingPremium: string;
  status: Record<ModelStatus, string>;
  capability: Record<ModelCapability, string>;
  listHeader: {
    model: string;
    provider: string;
    capabilities: string;
    context: string;
    pricing: string;
    usage: string;
    status: string;
    actions: string;
  };
  card: {
    contextWindow: string;
    maxOutput: string;
    inputPrice: string;
    outputPrice: string;
    viewDetails: string;
  };
  feedback: {
    loading: string;
    loadingFailed: string;
    empty: string;
    emptyUnfiltered: string;
    unknown: string;
  };
};

const zhCopy: CopyBundle = {
  title: '模型',
  subtitle: '浏览和管理本地模型库与模型元信息',
  searchPlaceholder: '搜索模型名称、提供商、标签...',
  sortByName: '按名称排序',
  sortByProvider: '按提供商排序',
  sortByRecent: '按最近使用排序',
  sortByUsage: '按用量排序',
  sortByPrice: '按价格排序',
  modelsCount: (count) => `${count} 个模型`,
  viewEnabled: (mode) => (mode === 'card' ? '卡片视图已启用' : '列表视图已启用'),
  sync: '拉取模型数据',
  syncing: '拉取中...',
  syncSuccess: (count) => `同步完成，当前缓存 ${count} 个模型`,
  syncFailed: '拉取失败',
  cardView: '卡片视图',
  listView: '列表视图',
  monoIcons: '单色图标',
  logoIcons: 'Logo 图标',
  brandIcons: '品牌图标',
  iconThemeEnabled: (theme) =>
    theme === 'brand' ? '品牌图标已启用' : theme === 'logo' ? 'Logo 图标已启用' : '单色图标已启用',
  allProviders: '全部提供商',
  allCapabilities: '全部能力',
  allStatuses: '全部状态',
  allContexts: '全部上下文',
  allPricing: '全部价格等级',
  contextCompact: '64K 及以下',
  contextStandard: '128K',
  contextExtended: '200K',
  contextUltra: '1M+',
  pricingEconomy: '经济型',
  pricingBalanced: '均衡型',
  pricingPremium: '高阶型',
  status: {
    active: '启用',
    new: '新',
    experimental: '实验',
    preview: '预览',
    deprecated: '弃用',
    unknown: '未知',
  },
  capability: {
    coding: '编码',
    reasoning: '推理',
    tool_use: '工具调用',
    vision: '视觉',
    long_context: '长上下文',
  },
  listHeader: {
    model: '模型',
    provider: '提供商',
    capabilities: '能力',
    context: '上下文',
    pricing: '价格',
    usage: 'Token 用量',
    status: '状态',
    actions: '操作',
  },
  card: {
    contextWindow: '上下文窗口',
    maxOutput: '最大输出',
    inputPrice: '输入 / 1M',
    outputPrice: '输出 / 1M',
    viewDetails: '查看详情',
  },
  feedback: {
    loading: '正在读取本地模型缓存...',
    loadingFailed: '读取模型缓存失败',
    empty: '当前筛选条件下没有模型。',
    emptyUnfiltered: '本地模型缓存为空，点击“拉取模型数据”从 OpenRouter 同步。',
    unknown: '未知',
  },
};

const enCopy: CopyBundle = {
  title: 'Models',
  subtitle: 'Explore and manage the local model catalog and metadata',
  searchPlaceholder: 'Search model name, provider, or tags...',
  sortByName: 'Sort by Name',
  sortByProvider: 'Sort by Provider',
  sortByRecent: 'Sort by Recent Activity',
  sortByUsage: 'Sort by Usage',
  sortByPrice: 'Sort by Price',
  modelsCount: (count) => `${count} models`,
  viewEnabled: (mode) => (mode === 'card' ? 'Card View enabled' : 'List View enabled'),
  sync: 'Fetch Model Data',
  syncing: 'Fetching...',
  syncSuccess: (count) => `Sync completed, ${count} models cached locally`,
  syncFailed: 'Fetch failed',
  cardView: 'Card View',
  listView: 'List View',
  monoIcons: 'Mono Icons',
  logoIcons: 'Logo Icons',
  brandIcons: 'Brand Icons',
  iconThemeEnabled: (theme) =>
    theme === 'brand'
      ? 'Brand icons enabled'
      : theme === 'logo'
        ? 'Logo icons enabled'
        : 'Mono icons enabled',
  allProviders: 'All Providers',
  allCapabilities: 'All Capabilities',
  allStatuses: 'All Statuses',
  allContexts: 'All Context Windows',
  allPricing: 'All Pricing Tiers',
  contextCompact: '64K and below',
  contextStandard: '128K',
  contextExtended: '200K',
  contextUltra: '1M+',
  pricingEconomy: 'Economy',
  pricingBalanced: 'Balanced',
  pricingPremium: 'Premium',
  status: {
    active: 'Active',
    new: 'New',
    experimental: 'Experimental',
    preview: 'Preview',
    deprecated: 'Deprecated',
    unknown: 'Unknown',
  },
  capability: {
    coding: 'Coding',
    reasoning: 'Reasoning',
    tool_use: 'Tool Use',
    vision: 'Vision',
    long_context: 'Long Context',
  },
  listHeader: {
    model: 'Model',
    provider: 'Provider',
    capabilities: 'Capabilities',
    context: 'Context',
    pricing: 'Pricing',
    usage: 'Token Usage',
    status: 'Status',
    actions: 'Actions',
  },
  card: {
    contextWindow: 'Context Window',
    maxOutput: 'Max Output',
    inputPrice: 'Input / 1M',
    outputPrice: 'Output / 1M',
    viewDetails: 'View Details',
  },
  feedback: {
    loading: 'Loading local model cache...',
    loadingFailed: 'Failed to load local model cache',
    empty: 'No models match the current filters.',
    emptyUnfiltered:
      'Local model cache is empty. Click "Fetch Model Data" to sync from OpenRouter.',
    unknown: 'Unknown',
  },
};

const knownCapabilities: ModelCapability[] = [
  'coding',
  'reasoning',
  'tool_use',
  'vision',
  'long_context',
];

const knownStatuses: ModelStatus[] = [
  'active',
  'new',
  'experimental',
  'preview',
  'deprecated',
  'unknown',
];

const knownContextTiers: ModelContextTier[] = ['compact', 'standard', 'extended', 'ultra'];
const knownPricingTiers: ModelPricingTier[] = ['economy', 'balanced', 'premium'];
const CARD_BATCH_SIZE = 18;
const LIST_BATCH_SIZE = 40;
const ICON_THEME_STORAGE_KEY = 'models.iconTheme';

function readInitialIconTheme(): ModelProviderIconTheme {
  if (typeof window === 'undefined') return 'mono';
  try {
    const stored = window.localStorage.getItem(ICON_THEME_STORAGE_KEY);
    return stored === 'brand' || stored === 'logo' ? stored : 'mono';
  } catch {
    return 'mono';
  }
}

export const ModelsPage = memo(function ModelsPage() {
  const { locale } = useI18n();
  const copy = locale === 'zh' ? zhCopy : enCopy;
  const [records, setRecords] = useState<ModelRecord[]>([]);
  const [search, setSearch] = useState('');
  const [provider, setProvider] = useState<string>('all');
  const [capability, setCapability] = useState<ModelCapability | 'all'>('all');
  const [status, setStatus] = useState<ModelStatus | 'all'>('all');
  const [contextTier, setContextTier] = useState<ModelContextTier | 'all'>('all');
  const [pricingTier, setPricingTier] = useState<ModelPricingTier | 'all'>('all');
  const [sortMode, setSortMode] = useState<SortMode>('name');
  const [viewMode, setViewMode] = useState<ViewMode>('card');
  const [iconTheme, setIconTheme] = useState<ModelProviderIconTheme>(readInitialIconTheme);
  const [isLoading, setIsLoading] = useState(true);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [syncMessage, setSyncMessage] = useState<string | null>(null);
  const deferredSearch = useDeferredValue(search);
  const loadMoreRef = useRef<HTMLDivElement | null>(null);
  const initialAutoSyncStartedRef = useRef(false);
  const batchSize = viewMode === 'card' ? CARD_BATCH_SIZE : LIST_BATCH_SIZE;
  const [visibleCount, setVisibleCount] = useState(batchSize);

  useEffect(() => {
    if (typeof window === 'undefined') return;
    try {
      window.localStorage.setItem(ICON_THEME_STORAGE_KEY, iconTheme);
    } catch {
      // localStorage can be unavailable in restricted browser contexts.
    }
  }, [iconTheme]);

  const currencyFormatter = useMemo(
    () =>
      new Intl.NumberFormat('en-US', {
        style: 'currency',
        currency: 'USD',
        minimumFractionDigits: 2,
        maximumFractionDigits: 2,
      }),
    [],
  );

  useEffect(() => {
    let cancelled = false;

    async function loadLocalCatalog() {
      setIsLoading(true);
      setErrorMessage(null);

      try {
        const response = await fetchModelCatalog();
        if (cancelled) {
          return;
        }

        setRecords(response.items.map(mapCatalogItemToRecord));

        if (!response.syncStatus.latestRun && !initialAutoSyncStartedRef.current) {
          initialAutoSyncStartedRef.current = true;
          setIsRefreshing(true);
          setSyncMessage(null);

          try {
            await refreshModelCatalog();
            if (cancelled) {
              return;
            }

            const refreshedResponse = await fetchModelCatalog();
            if (cancelled) {
              return;
            }

            setRecords(refreshedResponse.items.map(mapCatalogItemToRecord));
            setSyncMessage(copy.syncSuccess(refreshedResponse.totalItems));
          } catch (error) {
            if (cancelled) {
              return;
            }

            const message = error instanceof Error ? error.message : copy.syncFailed;
            setErrorMessage(message);
          } finally {
            if (!cancelled) {
              setIsRefreshing(false);
            }
          }
        }
      } catch (error) {
        if (cancelled) {
          return;
        }

        const message = error instanceof Error ? error.message : copy.feedback.loadingFailed;
        setErrorMessage(message);
      } finally {
        if (!cancelled) {
          setIsLoading(false);
        }
      }
    }

    void loadLocalCatalog();

    return () => {
      cancelled = true;
    };
  }, [copy]);

  const providerOptions = useMemo(() => {
    const providers = Array.from(new Set(records.map((record) => record.provider))).sort(
      (left, right) => left.localeCompare(right),
    );

    return [
      { value: 'all', label: copy.allProviders },
      ...providers.map((item) => ({
        value: item,
        label: formatProviderLabel(item),
      })),
    ];
  }, [copy.allProviders, records]);

  const statusOptions = useMemo(
    () => [
      { value: 'all' as const, label: copy.allStatuses },
      ...knownStatuses.map((item) => ({
        value: item,
        label: copy.status[item],
      })),
    ],
    [copy.allStatuses, copy.status],
  );

  const filteredModels = useMemo(() => {
    const query = deferredSearch.trim().toLowerCase();
    const models = records.filter((record) => {
      const matchesSearch =
        query.length === 0 ||
        record.name.toLowerCase().includes(query) ||
        formatProviderLabel(record.provider).toLowerCase().includes(query) ||
        record.modelId.toLowerCase().includes(query) ||
        record.capabilities
          .map((item) => copy.capability[item])
          .some((item) => item.toLowerCase().includes(query));

      return (
        matchesSearch &&
        (provider === 'all' || record.provider === provider) &&
        (capability === 'all' || record.capabilities.includes(capability)) &&
        (status === 'all' || record.status === status) &&
        (contextTier === 'all' || record.contextTier === contextTier) &&
        (pricingTier === 'all' || record.pricingTier === pricingTier)
      );
    });

    return models.sort((left, right) => {
      switch (sortMode) {
        case 'provider':
          return formatProviderLabel(left.provider).localeCompare(
            formatProviderLabel(right.provider),
          );
        case 'recent':
          return isoDateToMillis(right.lastSeenAt) - isoDateToMillis(left.lastSeenAt);
        case 'usage':
          return right.tokenUsage - left.tokenUsage;
        case 'price':
          return averagePrice(left) - averagePrice(right);
        case 'name':
        default:
          return left.name.localeCompare(right.name);
      }
    });
  }, [
    capability,
    contextTier,
    copy.capability,
    deferredSearch,
    pricingTier,
    provider,
    records,
    sortMode,
    status,
  ]);

  const visibleModels = useMemo(
    () => filteredModels.slice(0, visibleCount),
    [filteredModels, visibleCount],
  );
  const hasMoreModels = visibleCount < filteredModels.length;

  useEffect(() => {
    setVisibleCount(batchSize);
  }, [
    batchSize,
    capability,
    contextTier,
    deferredSearch,
    filteredModels.length,
    pricingTier,
    provider,
    sortMode,
    status,
    viewMode,
  ]);

  useEffect(() => {
    const target = loadMoreRef.current;
    if (!target || !hasMoreModels) {
      return;
    }

    const observer = new IntersectionObserver(
      (entries) => {
        if (!entries.some((entry) => entry.isIntersecting)) {
          return;
        }

        setVisibleCount((current) => Math.min(current + batchSize, filteredModels.length));
      },
      {
        root: null,
        rootMargin: '720px 0px',
        threshold: 0,
      },
    );

    observer.observe(target);

    return () => {
      observer.disconnect();
    };
  }, [batchSize, filteredModels.length, hasMoreModels]);

  const handleRefresh = async () => {
    setIsRefreshing(true);
    setErrorMessage(null);
    setSyncMessage(null);

    try {
      await refreshModelCatalog();
      const response = await fetchModelCatalog();
      setRecords(response.items.map(mapCatalogItemToRecord));
      setSyncMessage(copy.syncSuccess(response.totalItems));
    } catch (error) {
      const message = error instanceof Error ? error.message : copy.syncFailed;
      setErrorMessage(message);
    } finally {
      setIsRefreshing(false);
    }
  };

  return (
    <div className="models-page">
      <section className="models-hero">
        <div>
          <h1 className="page-title">{copy.title}</h1>
          <p className="page-subtitle">{copy.subtitle}</p>
        </div>
      </section>

      <section className="models-toolbar">
        <div className="models-toolbar-top">
          <label className="models-search" aria-label={copy.searchPlaceholder}>
            <Search size={18} />
            <input
              type="search"
              value={search}
              placeholder={copy.searchPlaceholder}
              onChange={(event) => setSearch(event.target.value)}
            />
          </label>

          <div className="models-toolbar-actions">
            <div className="models-segmented-controls">
              <div
                className="models-view-toggle"
                role="tablist"
                aria-label={copy.viewEnabled(viewMode)}
              >
                <button
                  type="button"
                  className={
                    viewMode === 'card'
                      ? 'models-view-btn models-view-btn-active'
                      : 'models-view-btn'
                  }
                  onClick={() => setViewMode('card')}
                >
                  <LayoutGrid size={15} />
                  <span>{copy.cardView}</span>
                </button>
                <button
                  type="button"
                  className={
                    viewMode === 'list'
                      ? 'models-view-btn models-view-btn-active'
                      : 'models-view-btn'
                  }
                  onClick={() => setViewMode('list')}
                >
                  <List size={15} />
                  <span>{copy.listView}</span>
                </button>
              </div>

              <div
                className="models-view-toggle"
                role="tablist"
                aria-label={copy.iconThemeEnabled(iconTheme)}
              >
                <button
                  type="button"
                  className={
                    iconTheme === 'mono'
                      ? 'models-view-btn models-view-btn-active'
                      : 'models-view-btn'
                  }
                  onClick={() => setIconTheme('mono')}
                >
                  <Square size={15} />
                  <span>{copy.monoIcons}</span>
                </button>
                <button
                  type="button"
                  className={
                    iconTheme === 'logo'
                      ? 'models-view-btn models-view-btn-active'
                      : 'models-view-btn'
                  }
                  onClick={() => setIconTheme('logo')}
                >
                  <Sparkles size={15} />
                  <span>{copy.logoIcons}</span>
                </button>
                <button
                  type="button"
                  className={
                    iconTheme === 'brand'
                      ? 'models-view-btn models-view-btn-active'
                      : 'models-view-btn'
                  }
                  onClick={() => setIconTheme('brand')}
                >
                  <Palette size={15} />
                  <span>{copy.brandIcons}</span>
                </button>
              </div>
            </div>

            <button
              type="button"
              className="models-btn"
              onClick={() => void handleRefresh()}
              disabled={isRefreshing}
            >
              <RefreshCw size={16} className={isRefreshing ? 'models-btn-spin' : undefined} />
              <span>{isRefreshing ? copy.syncing : copy.sync}</span>
            </button>
          </div>
        </div>

        <div className="models-filter-group">
          <SelectBox value={provider} onChange={setProvider} options={providerOptions} />
          <SelectBox
            value={capability}
            onChange={setCapability}
            options={[
              { value: 'all', label: copy.allCapabilities },
              ...knownCapabilities.map((item) => ({
                value: item,
                label: copy.capability[item],
              })),
            ]}
          />
          <SelectBox
            value={status}
            onChange={(value) => setStatus(value as 'all' | ModelStatus)}
            options={statusOptions}
          />
          <SelectBox
            value={contextTier}
            onChange={setContextTier}
            options={[
              { value: 'all', label: copy.allContexts },
              ...knownContextTiers.map((item) => ({
                value: item,
                label: contextTierLabel(item, copy),
              })),
            ]}
          />
          <SelectBox
            value={pricingTier}
            onChange={setPricingTier}
            options={[
              { value: 'all', label: copy.allPricing },
              ...knownPricingTiers.map((item) => ({
                value: item,
                label: pricingTierLabel(item, copy),
              })),
            ]}
          />
          <SelectBox
            value={sortMode}
            onChange={setSortMode}
            options={[
              { value: 'name', label: copy.sortByName },
              { value: 'provider', label: copy.sortByProvider },
              { value: 'recent', label: copy.sortByRecent },
              { value: 'usage', label: copy.sortByUsage },
              { value: 'price', label: copy.sortByPrice },
            ]}
          />
        </div>
      </section>

      {errorMessage ? (
        <section className="models-notice models-notice-error">{errorMessage}</section>
      ) : null}
      {syncMessage ? (
        <section className="models-notice models-notice-success">{syncMessage}</section>
      ) : null}

      <section className="models-meta">
        <span className="models-meta-count">{copy.modelsCount(filteredModels.length)}</span>
        <span className="models-meta-bullet" aria-hidden="true" />
        <span className="models-meta-view">{copy.viewEnabled(viewMode)}</span>
      </section>

      {isLoading ? (
        <EmptyState as="section" variant="card">
          {copy.feedback.loading}
        </EmptyState>
      ) : filteredModels.length === 0 ? (
        <EmptyState as="section" variant="card">
          {records.length === 0 ? copy.feedback.emptyUnfiltered : copy.feedback.empty}
        </EmptyState>
      ) : viewMode === 'card' ? (
        <>
          <section className="models-grid">
            {visibleModels.map((record) => (
              <ModelCard
                key={record.id}
                record={record}
                copy={copy}
                currencyFormatter={currencyFormatter}
                iconTheme={iconTheme}
              />
            ))}
          </section>
          {hasMoreModels ? (
            <div ref={loadMoreRef} className="models-load-sentinel" aria-hidden="true" />
          ) : null}
        </>
      ) : (
        <>
          <section className="models-list-card">
            <div className="models-list-wrap">
              <table className="models-list-table">
                <thead>
                  <tr>
                    <th>{copy.listHeader.model}</th>
                    <th>{copy.listHeader.provider}</th>
                    <th>{copy.listHeader.capabilities}</th>
                    <th>{copy.listHeader.context}</th>
                    <th>{copy.listHeader.pricing}</th>
                    <th>{copy.listHeader.usage}</th>
                    <th>{copy.listHeader.status}</th>
                    <th>{copy.listHeader.actions}</th>
                  </tr>
                </thead>
                <tbody>
                  {visibleModels.map((record) => (
                    <tr key={record.id}>
                      <td>
                        <div className="models-list-model">
                          <ModelProviderIcon provider={record.provider} theme={iconTheme} />
                          <div>
                            <div className="models-list-model-name">{record.name}</div>
                            <div className="models-list-model-subtitle">
                              {formatLastSeen(record.lastSeenAt, locale, copy)}
                            </div>
                          </div>
                        </div>
                      </td>
                      <td className="models-list-muted">{formatProviderLabel(record.provider)}</td>
                      <td>
                        <div className="models-list-tags">
                          {record.capabilities.slice(0, 3).map((item) => (
                            <span key={item} className="models-capability-chip">
                              {copy.capability[item]}
                            </span>
                          ))}
                        </div>
                      </td>
                      <td className="models-list-muted">
                        {record.contextWindow} / {record.maxOutput}
                      </td>
                      <td className="models-list-muted">
                        {formatModelPrice(record.inputPrice, currencyFormatter)} /{' '}
                        {formatModelPrice(record.outputPrice, currencyFormatter)}
                      </td>
                      <td className="models-list-muted">{formatTokenUsage(record.tokenUsage)}</td>
                      <td>
                        <StatusBadge status={record.status} label={copy.status[record.status]} />
                      </td>
                      <td>
                        <div className="models-list-actions">
                          <a
                            className={
                              record.docsUrl
                                ? 'models-inline-btn'
                                : 'models-inline-btn models-inline-btn-disabled'
                            }
                            href={record.docsUrl ?? undefined}
                            target="_blank"
                            rel="noreferrer"
                            aria-disabled={record.docsUrl ? undefined : true}
                            onClick={(event) => {
                              if (!record.docsUrl) {
                                event.preventDefault();
                              }
                            }}
                          >
                            <Eye size={15} />
                            <span>{copy.card.viewDetails}</span>
                          </a>
                        </div>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </section>
          {hasMoreModels ? (
            <div ref={loadMoreRef} className="models-load-sentinel" aria-hidden="true" />
          ) : null}
        </>
      )}
    </div>
  );
});

type ModelCardProps = {
  record: ModelRecord;
  copy: CopyBundle;
  currencyFormatter: Intl.NumberFormat;
  iconTheme: ModelProviderIconTheme;
};

const ModelCard = memo(function ModelCard({
  record,
  copy,
  currencyFormatter,
  iconTheme,
}: ModelCardProps) {
  return (
    <article className="models-card">
      <header className="models-card-header">
        <div className="models-card-identity">
          <ModelProviderIcon provider={record.provider} theme={iconTheme} />
          <div className="models-card-titles">
            <h2>{record.name}</h2>
            <p>{formatProviderLabel(record.provider)}</p>
          </div>
        </div>
        <div className="models-card-header-actions">
          <StatusBadge status={record.status} label={copy.status[record.status]} />
        </div>
      </header>

      <div className="models-capabilities">
        {record.capabilities.map((item) => (
          <span key={item} className="models-capability-chip">
            {copy.capability[item]}
          </span>
        ))}
      </div>

      <div className="models-stats-grid">
        <StatBlock label={copy.card.contextWindow} value={record.contextWindow} />
        <StatBlock label={copy.card.maxOutput} value={record.maxOutput} />
        <StatBlock
          label={copy.card.inputPrice}
          value={formatModelPrice(record.inputPrice, currencyFormatter)}
          separated
        />
        <StatBlock
          label={copy.card.outputPrice}
          value={formatModelPrice(record.outputPrice, currencyFormatter)}
        />
      </div>

      <p className="models-description">{record.description}</p>
    </article>
  );
});

function SelectBox<T extends string>({
  value,
  onChange,
  options,
}: {
  value: T;
  onChange: (value: T) => void;
  options: Array<{ value: T; label: string }>;
}) {
  return (
    <label className="models-select-wrap">
      <select value={value} onChange={(event) => onChange(event.target.value as T)}>
        {options.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
      <ChevronDown size={16} />
    </label>
  );
}

const StatusBadge = memo(function StatusBadge({
  status,
  label,
}: {
  status: ModelStatus;
  label: string;
}) {
  return (
    <span className={`models-status-badge models-status-${status}`}>
      <span className="models-status-dot" aria-hidden="true" />
      <span>{label}</span>
    </span>
  );
});

const StatBlock = memo(function StatBlock({
  label,
  value,
  separated,
}: {
  label: string;
  value: string;
  separated?: boolean;
}) {
  return (
    <div
      className={separated ? 'models-stat-block models-stat-block-separated' : 'models-stat-block'}
    >
      <span className="models-stat-value">{value}</span>
      <span className="models-stat-label">{label}</span>
    </div>
  );
});

function mapCatalogItemToRecord(item: ModelCatalogListItem): ModelRecord {
  return {
    id: item.id,
    canonicalKey: item.canonicalKey,
    modelId: item.modelId,
    name: item.displayName,
    provider: item.provider,
    capabilities: item.capabilities
      .map(normalizeCapability)
      .filter((value): value is ModelCapability => value !== null),
    contextWindow: formatContextAmount(item.contextWindow),
    maxOutput: formatContextAmount(item.maxOutputTokens),
    inputPrice: normalizeCatalogPrice(item.pricingInputUsdPerMtok),
    outputPrice: normalizeCatalogPrice(item.pricingOutputUsdPerMtok),
    lastSeenAt: item.lastSeenAt,
    tokenUsage: item.tokenUsageTotal,
    description: item.description?.trim() || item.canonicalKey,
    status: normalizeStatus(item.status),
    contextTier: normalizeContextTier(item.contextTier),
    pricingTier: normalizePricingTier(item.pricingTier),
    docsUrl: item.docsUrl,
  };
}

function normalizeCatalogPrice(value: number | undefined): number | null {
  if (value === undefined || value === null || Number.isNaN(value) || value < 0) {
    return null;
  }
  return value;
}

function formatModelPrice(value: number | null, currencyFormatter: Intl.NumberFormat): string {
  if (value === null) {
    return '--';
  }
  return currencyFormatter.format(value);
}

function averagePrice(record: Pick<ModelRecord, 'inputPrice' | 'outputPrice'>): number {
  const prices = [record.inputPrice, record.outputPrice].filter(
    (value): value is number => value !== null,
  );
  if (prices.length === 0) {
    return Number.POSITIVE_INFINITY;
  }
  return prices.reduce((sum, value) => sum + value, 0) / prices.length;
}

function normalizeCapability(value: string): ModelCapability | null {
  const normalized = value.trim().toLowerCase();
  return knownCapabilities.includes(normalized as ModelCapability)
    ? (normalized as ModelCapability)
    : null;
}

function normalizeStatus(value: string): ModelStatus {
  const normalized = value.trim().toLowerCase();
  return knownStatuses.includes(normalized as ModelStatus)
    ? (normalized as ModelStatus)
    : 'unknown';
}

function normalizeContextTier(value: string): ModelContextTier {
  const normalized = value.trim().toLowerCase();
  return knownContextTiers.includes(normalized as ModelContextTier)
    ? (normalized as ModelContextTier)
    : 'standard';
}

function normalizePricingTier(value: string): ModelPricingTier {
  const normalized = value.trim().toLowerCase();
  return knownPricingTiers.includes(normalized as ModelPricingTier)
    ? (normalized as ModelPricingTier)
    : 'balanced';
}

function contextTierLabel(tier: ModelContextTier, copy: CopyBundle) {
  switch (tier) {
    case 'compact':
      return copy.contextCompact;
    case 'standard':
      return copy.contextStandard;
    case 'extended':
      return copy.contextExtended;
    case 'ultra':
      return copy.contextUltra;
  }
}

function pricingTierLabel(tier: ModelPricingTier, copy: CopyBundle) {
  switch (tier) {
    case 'economy':
      return copy.pricingEconomy;
    case 'balanced':
      return copy.pricingBalanced;
    case 'premium':
      return copy.pricingPremium;
  }
}

function formatProviderLabel(provider: string) {
  const normalized = provider.trim().toLowerCase();
  switch (normalized) {
    case 'openai':
      return 'OpenAI';
    case 'anthropic':
      return 'Anthropic';
    case 'google':
      return 'Google';
    case 'x-ai':
      return 'xAI';
    case 'deepseek':
      return 'DeepSeek';
    default:
      return provider
        .replace(/[-_]+/g, ' ')
        .replace(/\b\w/g, (character) => character.toUpperCase());
  }
}

function formatLastSeen(value: string | undefined, locale: 'zh' | 'en', copy: CopyBundle) {
  if (!value) {
    return copy.feedback.unknown;
  }

  const timestamp = Date.parse(value);
  if (Number.isNaN(timestamp)) {
    return copy.feedback.unknown;
  }

  const diffMinutes = Math.max(0, Math.floor((Date.now() - timestamp) / 60000));
  if (diffMinutes < 60) {
    return locale === 'zh' ? `${diffMinutes} 分钟前` : `${diffMinutes}m ago`;
  }

  const diffHours = Math.floor(diffMinutes / 60);
  if (diffHours < 24) {
    return locale === 'zh' ? `${diffHours} 小时前` : `${diffHours}h ago`;
  }

  const diffDays = Math.floor(diffHours / 24);
  return locale === 'zh' ? `${diffDays} 天前` : `${diffDays}d ago`;
}

function formatTokenUsage(value: number) {
  if (value >= 1_000_000) {
    return `${(value / 1_000_000).toFixed(2)}M`;
  }

  if (value >= 1_000) {
    return `${Math.round(value / 1_000)}K`;
  }

  return String(value);
}

function formatContextAmount(value: number | undefined) {
  if (value == null || value <= 0) {
    return '--';
  }

  if (value >= 1_000_000) {
    return `${stripTrailingZero((value / 1_000_000).toFixed(1))}M`;
  }

  if (value >= 1_000) {
    return `${stripTrailingZero((value / 1_000).toFixed(value % 1_000 === 0 ? 0 : 1))}K`;
  }

  return String(value);
}

function stripTrailingZero(value: string) {
  return value.endsWith('.0') ? value.slice(0, -2) : value;
}

function isoDateToMillis(value: string | undefined) {
  if (!value) {
    return 0;
  }

  const timestamp = Date.parse(value);
  return Number.isNaN(timestamp) ? 0 : timestamp;
}
