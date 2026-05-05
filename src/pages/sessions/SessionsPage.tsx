import { useCallback, useDeferredValue, useEffect, useMemo, useState } from 'react';
import {
  ArrowDownUp,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  RefreshCw,
  Search,
} from 'lucide-react';
import { AppIcon } from '../../components/app-icon/AppIcon';
import { EmptyState } from '../../components/empty-state/EmptyState';
import { buildPageTokens } from '../../lib/pagination';
import { sourceAppLabelKey, type SourceApp } from '../../lib/sourceApps';
import { useEnabledSourceApps } from '../../lib/useEnabledSourceApps';
import { useI18n } from '../../i18n/useI18n';
import { type SessionRecord, type SessionSourceState } from './sessionData';
import {
  fetchSessionsList,
  rescanEnabledSources,
  type SessionSortDirection,
  type SessionSortField,
} from './sessionService';
import './SessionsPage.css';

const sortableHeaders: Array<{
  field: SessionSortField;
  labelKey: string;
}> = [
  { field: 'name', labelKey: 'session.header.name' },
  { field: 'sourceApp', labelKey: 'session.header.source' },
  { field: 'model', labelKey: 'session.header.model' },
  { field: 'inputTokens', labelKey: 'session.header.input' },
  { field: 'outputTokens', labelKey: 'session.header.output' },
  { field: 'estimatedCostUsd', labelKey: 'session.header.estimatedCost' },
  { field: 'lastUpdated', labelKey: 'session.header.updated' },
  { field: 'messages', labelKey: 'session.header.messages' },
  { field: 'sourceState', labelKey: 'session.header.state' },
];

const MODEL_DISPLAY_LIMIT = 20;

export function SessionsPage() {
  const { locale, t } = useI18n();
  const enabledSourceApps = useEnabledSourceApps();
  const [sessions, setSessions] = useState<SessionRecord[]>([]);
  const [search, setSearch] = useState('');
  const [sourceFilter, setSourceFilter] = useState<'all' | SourceApp>('all');
  const [stateFilter, setStateFilter] = useState<'all' | SessionSourceState>('all');
  const [sortField, setSortField] = useState<SessionSortField>('lastUpdated');
  const [sortDirection, setSortDirection] = useState<SessionSortDirection>('desc');
  const [page, setPage] = useState(1);
  const [rowsPerPage, setRowsPerPage] = useState(25);
  const [totalPages, setTotalPages] = useState(1);
  const [totalItems, setTotalItems] = useState(0);
  const [isLoading, setIsLoading] = useState(false);
  const [isRescanning, setIsRescanning] = useState(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const deferredSearch = useDeferredValue(search);

  const numberFormatter = new Intl.NumberFormat(locale === 'zh' ? 'zh-CN' : 'en-US');
  const dateFormatter = new Intl.DateTimeFormat(locale === 'zh' ? 'zh-CN' : 'en-US', {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
  const pageRows = sessions;
  const pageTokens = buildPageTokens(page, totalPages);
  const sourceTabs = useMemo<Array<{ key: 'all' | SourceApp; labelKey: string }>>(
    () => [
      { key: 'all', labelKey: 'session.tabs.all' },
      ...enabledSourceApps.map((app) => ({
        key: app,
        labelKey: `session.source.${app}`,
      })),
    ],
    [enabledSourceApps],
  );

  useEffect(() => {
    setPage(1);
  }, [deferredSearch, sourceFilter, stateFilter, rowsPerPage]);

  useEffect(() => {
    if (sourceFilter !== 'all' && !enabledSourceApps.includes(sourceFilter)) {
      setSourceFilter('all');
    }
  }, [enabledSourceApps, sourceFilter]);

  useEffect(() => {
    if (page > totalPages) {
      setPage(totalPages);
    }
  }, [page, totalPages]);

  const loadSessions = useCallback(
    async (isCancelled: () => boolean = () => false) => {
      setIsLoading(true);
      setErrorMessage(null);

      try {
        const result = await fetchSessionsList({
          page,
          pageSize: rowsPerPage,
          q: deferredSearch,
          sourceApps: sourceFilter === 'all' ? enabledSourceApps : [sourceFilter],
          sourceStates: stateFilter === 'all' ? undefined : [stateFilter],
          sortBy: sortField,
          sortOrder: sortDirection,
        });

        if (!isCancelled()) {
          setSessions(result.items);
          setTotalItems(result.totalItems);
          setTotalPages(Math.max(1, result.totalPages));
        }
      } catch (error) {
        if (isCancelled()) {
          return;
        }

        const message = error instanceof Error ? error.message : t('session.feedback.loadFailed');
        setSessions([]);
        setTotalItems(0);
        setTotalPages(1);
        setErrorMessage(message);
      } finally {
        if (!isCancelled()) {
          setIsLoading(false);
        }
      }
    },
    [
      deferredSearch,
      enabledSourceApps,
      page,
      rowsPerPage,
      sortDirection,
      sortField,
      sourceFilter,
      stateFilter,
      t,
    ],
  );

  useEffect(() => {
    let cancelled = false;

    void loadSessions(() => cancelled);

    return () => {
      cancelled = true;
    };
  }, [loadSessions]);

  const handleHeaderSort = (field: SessionSortField) => {
    if (sortField === field) {
      setSortDirection((current) => (current === 'asc' ? 'desc' : 'asc'));
      return;
    }

    setSortField(field);
    setSortDirection(field === 'name' || field === 'model' ? 'asc' : 'desc');
  };

  const handleRescan = async () => {
    try {
      setIsRescanning(true);
      setErrorMessage(null);
      await rescanEnabledSources(sourceFilter === 'all' ? undefined : sourceFilter);
      await loadSessions();
    } catch (error) {
      setErrorMessage(error instanceof Error ? error.message : t('session.feedback.rescanFailed'));
    } finally {
      setIsRescanning(false);
    }
  };

  return (
    <div className="sessions-page">
      <section className="sessions-hero">
        <div>
          <h1 className="page-title">{t('session.title')}</h1>
          <p className="page-subtitle">{t('session.subtitle')}</p>
        </div>
      </section>

      <section className="sessions-toolbar">
        <label className="sessions-search" aria-label={t('session.search.placeholder')}>
          <Search size={18} />
          <input
            type="search"
            value={search}
            placeholder={t('session.search.placeholder')}
            onChange={(event) => setSearch(event.target.value)}
          />
        </label>
        <div className="sessions-toolbar-controls">
          <SelectField
            value={stateFilter}
            onChange={(value) => setStateFilter(value as 'all' | SessionSourceState)}
            options={[
              { value: 'all', label: t('session.actions.filter') },
              { value: 'synced', label: t('session.state.synced') },
              { value: 'archived', label: t('session.state.archived') },
              { value: 'deleted', label: t('session.state.deleted') },
              { value: 'missing', label: t('session.state.missing') },
            ]}
          />
          <button
            type="button"
            className="session-btn"
            onClick={() => void handleRescan()}
            disabled={isRescanning}
          >
            <RefreshCw size={16} />
            <span>{t('session.actions.rescan')}</span>
          </button>
        </div>
      </section>

      <section className="sessions-tabs" aria-label={t('session.actions.filter')}>
        {sourceTabs.map((tab) => (
          <button
            key={tab.key}
            type="button"
            className={sourceFilter === tab.key ? 'session-tab session-tab-active' : 'session-tab'}
            onClick={() => setSourceFilter(tab.key)}
          >
            {t(tab.labelKey)}
          </button>
        ))}
      </section>

      <section className="sessions-card">
        <header className="sessions-card-header">
          <h2 className="sessions-card-title">{t('session.list.title')}</h2>
        </header>

        {errorMessage ? <EmptyState variant="fill">{errorMessage}</EmptyState> : null}

        {!errorMessage && isLoading ? (
          <EmptyState variant="fill">{t('session.feedback.loading')}</EmptyState>
        ) : null}

        {!errorMessage && !isLoading && pageRows.length > 0 ? (
          <>
            <div className="sessions-table-wrap">
              <table className="sessions-table">
                <thead>
                  <tr>
                    {sortableHeaders.map((header) => (
                      <th key={header.field}>
                        <button
                          type="button"
                          className="sessions-sort-header"
                          onClick={() => handleHeaderSort(header.field)}
                        >
                          <span>{t(header.labelKey)}</span>
                          <ArrowDownUp size={14} />
                        </button>
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {pageRows.map((session) => (
                    <tr key={session.id}>
                      <td className="session-name">
                        <span className="session-name-label" title={session.name}>
                          {session.name}
                        </span>
                      </td>
                      <td>
                        <span className="session-source">
                          <AppIcon
                            app={session.sourceApp}
                            label={t(sourceAppLabelKey(session.sourceApp))}
                          />
                          <span>{t(sourceAppLabelKey(session.sourceApp))}</span>
                        </span>
                      </td>
                      <td className="session-muted">
                        <span className="session-model-label" title={session.model}>
                          {truncateText(session.model, MODEL_DISPLAY_LIMIT)}
                        </span>
                      </td>
                      <td className="session-metric">
                        <TokenValue value={session.inputTokens} formatter={numberFormatter} />
                      </td>
                      <td className="session-metric">
                        <TokenValue value={session.outputTokens} formatter={numberFormatter} />
                      </td>
                      <td className="session-metric">
                        <CostValue value={session.estimatedCostUsd} />
                      </td>
                      <td className="session-muted">
                        {dateFormatter.format(new Date(session.lastUpdated))}
                      </td>
                      <td className="session-metric">{numberFormatter.format(session.messages)}</td>
                      <td>
                        <span className={`session-state session-state-${session.sourceState}`}>
                          {t(`session.state.${session.sourceState}`)}
                        </span>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>

            <footer className="sessions-pagination">
              <div className="sessions-page-size">
                <span>{t('session.rows')}:</span>
                <SelectField
                  compact
                  value={String(rowsPerPage)}
                  onChange={(value) => setRowsPerPage(Number(value))}
                  options={[
                    { value: '10', label: '10' },
                    { value: '25', label: '25' },
                    { value: '50', label: '50' },
                  ]}
                />
                <div className="sessions-count">
                  {t('session.pagination.total', { count: totalItems })}
                </div>
              </div>

              <div className="sessions-page-controls">
                <button
                  type="button"
                  className="session-page-btn"
                  disabled={page === 1}
                  onClick={() => setPage((current) => Math.max(1, current - 1))}
                >
                  <ChevronLeft size={16} />
                  <span>{t('session.previous')}</span>
                </button>

                {pageTokens.map((token) =>
                  typeof token === 'number' ? (
                    <button
                      key={token}
                      type="button"
                      className="session-page-btn"
                      aria-current={page === token ? 'page' : undefined}
                      onClick={() => setPage(token)}
                    >
                      {token}
                    </button>
                  ) : (
                    <span key={token} className="session-page-ellipsis">
                      ...
                    </span>
                  ),
                )}

                <button
                  type="button"
                  className="session-page-btn"
                  disabled={page === totalPages}
                  onClick={() => setPage((current) => Math.min(totalPages, current + 1))}
                >
                  <span>{t('session.next')}</span>
                  <ChevronRight size={16} />
                </button>
              </div>
            </footer>
          </>
        ) : null}

        {!errorMessage && !isLoading && pageRows.length === 0 ? (
          <EmptyState variant="fill">{t('session.empty')}</EmptyState>
        ) : null}
      </section>
    </div>
  );
}

function TokenValue({ value, formatter }: { value: number; formatter: Intl.NumberFormat }) {
  return <>{formatter.format(value)}</>;
}

function CostValue({ value }: { value: number | null }) {
  if (value === null) {
    return <>-</>;
  }

  return <>{formatUsdAmount(value)}</>;
}

function formatUsdAmount(value: number) {
  const sign = value < 0 ? '-' : '';
  const formatted = new Intl.NumberFormat('en-US', {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }).format(Math.abs(value));
  return `${sign}$${formatted}`;
}

function SelectField({
  value,
  onChange,
  options,
  compact = false,
}: {
  value: string;
  onChange: (value: string) => void;
  options: Array<{ value: string; label: string }>;
  compact?: boolean;
}) {
  return (
    <label
      className={
        compact ? 'session-select-wrap session-select-wrap-compact' : 'session-select-wrap'
      }
    >
      <select
        className="session-select"
        value={value}
        onChange={(event) => onChange(event.target.value)}
      >
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

function truncateText(value: string, maxLength: number): string {
  const characters = Array.from(value);
  if (characters.length <= maxLength) {
    return value;
  }

  if (maxLength <= 3) {
    return '.'.repeat(maxLength);
  }

  return `${characters.slice(0, maxLength - 3).join('')}...`;
}
