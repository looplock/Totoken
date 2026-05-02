import { useDeferredValue, useEffect, useMemo, useState } from 'react';
import { ChevronDown, ChevronLeft, ChevronRight, Clock3, RefreshCw, Search } from 'lucide-react';
import { useI18n } from '../../i18n/useI18n';
import { EmptyState } from '../../components/empty-state/EmptyState';
import { buildPageTokens } from '../../lib/pagination';
import { isTauriRuntime } from '../../lib/tauri';
import type { AutoScanStatus } from '../settings/settingsData';
import { fetchAutoScanStatus } from '../settings/settingsService';
import type { ScanRunRecord, ScanRunStatus, ScanRunTriggerType } from './scanTasksData';
import { fetchScanRecords } from './scanTasksService';
import './ScanTasksPage.css';

const RECENT_RUN_LIMIT = 50;

type FilterStatus = 'all' | ScanRunStatus;
type FilterTrigger = 'all' | ScanRunTriggerType;

const pageCopyByLocale = {
  zh: {
    title: '扫描记录',
    subtitle: '集中查看最近扫描批次、增量跳过情况和运行结果。',
    actions: {
      refresh: '刷新记录',
      refreshing: '刷新中...',
      clearFilters: '清空筛选',
    },
    runtime: {
      running: '自动扫描运行中',
      busy: '当前有扫描任务占用执行器',
      idle: '扫描器空闲',
    },
    filters: {
      searchPlaceholder: '按触发方式、状态或批次时间搜索',
      allStatus: '全部状态',
      allTriggers: '全部触发',
      status: {
        running: '运行中',
        completed: '已完成',
        failed: '失败',
        unknown: '未知',
      },
      trigger: {
        manual: '手动',
        auto: '自动',
        other: '其他',
      },
    },
    list: {
      title: '最近批次',
      empty: '当前筛选条件下没有扫描记录。',
      filesSeen: '扫描文件',
      parsed: '解析',
      skipped: '跳过',
      changed: '会话变更',
      errors: '错误',
      total: '共 {count} 条',
      retention: '当前列表仅展示最近 {count} 条扫描记录。',
      rows: '每页',
      previous: '上一页',
      next: '下一页',
      pageTotal: '共 {count} 条',
    },
    table: {
      started: '开始时间',
      duration: '耗时',
      status: '状态',
      trigger: '触发方式',
      failed: '失败',
    },
    common: {
      loading: '正在加载扫描记录...',
      runtimeRequired: '当前环境不可用，扫描记录仅支持在 Tauri 应用内查看。',
      unknown: '--',
    },
  },
  en: {
    title: 'Scan Records',
    subtitle:
      'Review recent scan batches, incremental skips, and runtime outcomes in one operational view.',
    actions: {
      refresh: 'Refresh Records',
      refreshing: 'Refreshing...',
      clearFilters: 'Clear Filters',
    },
    runtime: {
      running: 'Auto scan is running',
      busy: 'A scan task is holding the executor',
      idle: 'Scanner is idle',
    },
    filters: {
      searchPlaceholder: 'Search by trigger, status, or batch time',
      allStatus: 'All Statuses',
      allTriggers: 'All Triggers',
      status: {
        running: 'Running',
        completed: 'Completed',
        failed: 'Failed',
        unknown: 'Unknown',
      },
      trigger: {
        manual: 'Manual',
        auto: 'Auto',
        other: 'Other',
      },
    },
    list: {
      title: 'Recent Batches',
      empty: 'No scan records match the current filters.',
      filesSeen: 'Files seen',
      parsed: 'Parsed',
      skipped: 'Skipped',
      changed: 'Sessions changed',
      errors: 'Errors',
      total: '{count} total',
      retention: 'The current list shows the most recent {count} scan records.',
      rows: 'Rows',
      previous: 'Previous',
      next: 'Next',
      pageTotal: '{count} total',
    },
    table: {
      started: 'Started',
      duration: 'Duration',
      status: 'Status',
      trigger: 'Trigger',
      failed: 'Failed',
    },
    common: {
      loading: 'Loading scan records...',
      runtimeRequired: 'Scan records are only available inside the Tauri runtime.',
      unknown: '--',
    },
  },
} as const;

type PageCopy = (typeof pageCopyByLocale)['zh'] | (typeof pageCopyByLocale)['en'];

export function ScanRecordsPage() {
  const { locale } = useI18n();
  const language = locale === 'zh' ? 'zh-CN' : 'en-US';
  const copy = pageCopyByLocale[locale];
  const numberFormatter = new Intl.NumberFormat(language);

  const [runs, setRuns] = useState<ScanRunRecord[]>([]);
  const [autoScanStatus, setAutoScanStatus] = useState<AutoScanStatus | null>(null);
  const [search, setSearch] = useState('');
  const [statusFilter, setStatusFilter] = useState<FilterStatus>('all');
  const [triggerFilter, setTriggerFilter] = useState<FilterTrigger>('all');
  const [page, setPage] = useState(1);
  const [rowsPerPage, setRowsPerPage] = useState(25);
  const [isLoading, setIsLoading] = useState(true);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [refreshNonce, setRefreshNonce] = useState(0);
  const deferredSearch = useDeferredValue(search);

  const hasActiveFilters =
    deferredSearch.trim().length > 0 || statusFilter !== 'all' || triggerFilter !== 'all';

  const filteredRuns = useMemo(() => {
    const normalized = deferredSearch.trim().toLowerCase();

    return runs.filter((run) => {
      if (statusFilter !== 'all' && run.status !== statusFilter) {
        return false;
      }

      if (triggerFilter !== 'all' && run.triggerType !== triggerFilter) {
        return false;
      }

      if (!normalized) {
        return true;
      }

      const haystack = [run.triggerType, run.status, run.startedAt, run.endedAt ?? '']
        .join(' ')
        .toLowerCase();
      return haystack.includes(normalized);
    });
  }, [deferredSearch, runs, statusFilter, triggerFilter]);

  const totalItems = filteredRuns.length;
  const totalPages = Math.max(1, Math.ceil(totalItems / rowsPerPage));
  const pageRuns = useMemo(() => {
    const startIndex = (page - 1) * rowsPerPage;
    return filteredRuns.slice(startIndex, startIndex + rowsPerPage);
  }, [filteredRuns, page, rowsPerPage]);
  const pageTokens = buildPageTokens(page, totalPages);

  useEffect(() => {
    setPage(1);
  }, [deferredSearch, rowsPerPage, statusFilter, triggerFilter]);

  useEffect(() => {
    if (page > totalPages) {
      setPage(totalPages);
    }
  }, [page, totalPages]);

  useEffect(() => {
    let cancelled = false;

    async function loadData(showLoading: boolean) {
      if (showLoading) {
        setIsLoading(true);
      } else {
        setIsRefreshing(true);
      }
      setErrorMessage(null);

      try {
        const [recordsResult, runtimeResult] = await Promise.allSettled([
          fetchScanRecords(RECENT_RUN_LIMIT),
          fetchAutoScanStatus(),
        ]);

        if (recordsResult.status === 'rejected') {
          throw recordsResult.reason;
        }

        if (cancelled) {
          return;
        }

        const records = recordsResult.value;
        setRuns(records.items);
        setAutoScanStatus(runtimeResult.status === 'fulfilled' ? runtimeResult.value : null);
      } catch (error) {
        if (cancelled) {
          return;
        }

        const message = error instanceof Error ? error.message : copy.common.runtimeRequired;
        setErrorMessage(message);
        setRuns([]);
      } finally {
        if (!cancelled) {
          setIsLoading(false);
          setIsRefreshing(false);
        }
      }
    }

    if (!isTauriRuntime()) {
      setRuns([]);
      setAutoScanStatus(null);
      setErrorMessage(copy.common.runtimeRequired);
      setIsLoading(false);
      setIsRefreshing(false);
      return undefined;
    }

    void loadData(true);
    const intervalId = window.setInterval(() => {
      void loadData(false);
    }, 15000);

    return () => {
      cancelled = true;
      window.clearInterval(intervalId);
    };
  }, [copy.common.runtimeRequired, refreshNonce]);

  const runtimeMessage = getRuntimeMessage(autoScanStatus, copy);

  return (
    <div className="scan-records-page">
      <section className="scan-records-hero">
        <div>
          <h1 className="page-title">{copy.title}</h1>
          <p className="page-subtitle">{copy.subtitle}</p>
        </div>

        <div className="scan-records-hero-actions">
          <button
            type="button"
            className="scan-records-btn scan-records-btn-primary"
            onClick={() => setRefreshNonce((current) => current + 1)}
            disabled={isRefreshing}
          >
            <RefreshCw size={16} />
            <span>{isRefreshing ? copy.actions.refreshing : copy.actions.refresh}</span>
          </button>
        </div>
      </section>

      <section className="scan-records-toolbar">
        <label className="scan-records-search" aria-label={copy.filters.searchPlaceholder}>
          <Search size={18} />
          <input
            type="search"
            value={search}
            placeholder={copy.filters.searchPlaceholder}
            onChange={(event) => setSearch(event.target.value)}
          />
        </label>

        <div className="scan-records-toolbar-controls">
          <div className="scan-records-select-wrap">
            <select
              className="scan-records-select"
              value={statusFilter}
              onChange={(event) => setStatusFilter(event.target.value as FilterStatus)}
            >
              <option value="all">{copy.filters.allStatus}</option>
              <option value="running">{copy.filters.status.running}</option>
              <option value="completed">{copy.filters.status.completed}</option>
              <option value="failed">{copy.filters.status.failed}</option>
              <option value="unknown">{copy.filters.status.unknown}</option>
            </select>
            <ChevronDown size={16} />
          </div>

          <div className="scan-records-select-wrap">
            <select
              className="scan-records-select"
              value={triggerFilter}
              onChange={(event) => setTriggerFilter(event.target.value as FilterTrigger)}
            >
              <option value="all">{copy.filters.allTriggers}</option>
              <option value="manual">{copy.filters.trigger.manual}</option>
              <option value="auto">{copy.filters.trigger.auto}</option>
              <option value="other">{copy.filters.trigger.other}</option>
            </select>
            <ChevronDown size={16} />
          </div>

          {hasActiveFilters ? (
            <button
              type="button"
              className="scan-records-btn"
              onClick={() => {
                setSearch('');
                setStatusFilter('all');
                setTriggerFilter('all');
              }}
            >
              {copy.actions.clearFilters}
            </button>
          ) : null}
        </div>
      </section>

      <div className="scan-records-layout">
        <section className="scan-records-card">
          <header className="scan-records-card-header">
            <div className="scan-records-card-title-wrap">
              <Clock3 size={16} />
              <h2 className="scan-records-card-title">
                {copy.list.title} /{' '}
                {copy.list.total.replace('{count}', numberFormatter.format(filteredRuns.length))}
              </h2>
            </div>
            <div className="scan-records-card-header-meta">
              <span className={buildRuntimeStateClassName(autoScanStatus)}>{runtimeMessage}</span>
              <span className="scan-records-card-caption">
                {copy.list.retention.replace('{count}', numberFormatter.format(RECENT_RUN_LIMIT))}
              </span>
            </div>
          </header>

          {errorMessage ? (
            <EmptyState>{errorMessage}</EmptyState>
          ) : isLoading ? (
            <EmptyState>{copy.common.loading}</EmptyState>
          ) : pageRuns.length > 0 ? (
            <>
              <div className="scan-records-table-wrap">
                <table className="scan-records-table">
                  <thead>
                    <tr>
                      <th>{copy.table.started}</th>
                      <th>{copy.table.duration}</th>
                      <th>{copy.table.status}</th>
                      <th>{copy.table.trigger}</th>
                      <th>{copy.list.filesSeen}</th>
                      <th>{copy.list.parsed}</th>
                      <th>{copy.list.skipped}</th>
                      <th>{copy.table.failed}</th>
                      <th>{copy.list.changed}</th>
                      <th>{copy.list.errors}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {pageRuns.map((run) => (
                      <tr key={run.id} className="scan-records-table-row">
                        <td className="scan-records-started-cell">
                          <span className="scan-records-started-value" title={run.startedAt}>
                            {formatDateTime(run.startedAt, language, copy.common.unknown)}
                          </span>
                        </td>
                        <td className="scan-records-table-metric">
                          {formatRunDuration(run.durationMs, language, copy.common.unknown)}
                        </td>
                        <td>
                          <span className={buildStatusClassName(run.status)}>
                            {copy.filters.status[run.status]}
                          </span>
                        </td>
                        <td>
                          <span className="scan-records-pill scan-records-pill-trigger">
                            {copy.filters.trigger[run.triggerType]}
                          </span>
                        </td>
                        <td className="scan-records-table-metric">
                          {numberFormatter.format(run.filesSeen)}
                        </td>
                        <td className="scan-records-table-metric">
                          {numberFormatter.format(run.filesParsed)}
                        </td>
                        <td className="scan-records-table-metric">
                          {numberFormatter.format(run.filesSkipped)}
                        </td>
                        <td className="scan-records-table-metric">
                          {numberFormatter.format(run.filesFailed)}
                        </td>
                        <td className="scan-records-table-metric">
                          {numberFormatter.format(run.sessionsChanged)}
                        </td>
                        <td className="scan-records-table-metric">
                          {numberFormatter.format(run.errorCount)}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>

              <footer className="scan-records-pagination">
                <div className="scan-records-page-size">
                  <span>{copy.list.rows}:</span>
                  <div className="scan-records-select-wrap">
                    <select
                      className="scan-records-select scan-records-page-select"
                      value={rowsPerPage}
                      onChange={(event) => setRowsPerPage(Number(event.target.value))}
                    >
                      <option value={10}>10</option>
                      <option value={25}>25</option>
                      <option value={50}>50</option>
                    </select>
                    <ChevronDown size={16} />
                  </div>
                  <div className="scan-records-count">
                    {copy.list.pageTotal.replace('{count}', numberFormatter.format(totalItems))}
                  </div>
                </div>

                <div className="scan-records-page-controls">
                  <button
                    type="button"
                    className="scan-records-page-btn"
                    disabled={page === 1}
                    onClick={() => setPage((current) => Math.max(1, current - 1))}
                  >
                    <ChevronLeft size={16} />
                    <span>{copy.list.previous}</span>
                  </button>

                  {pageTokens.map((token) =>
                    typeof token === 'number' ? (
                      <button
                        key={token}
                        type="button"
                        className="scan-records-page-btn"
                        aria-current={page === token ? 'page' : undefined}
                        onClick={() => setPage(token)}
                      >
                        {token}
                      </button>
                    ) : (
                      <span key={token} className="scan-records-page-ellipsis">
                        ...
                      </span>
                    ),
                  )}

                  <button
                    type="button"
                    className="scan-records-page-btn"
                    disabled={page === totalPages}
                    onClick={() => setPage((current) => Math.min(totalPages, current + 1))}
                  >
                    <span>{copy.list.next}</span>
                    <ChevronRight size={16} />
                  </button>
                </div>
              </footer>
            </>
          ) : (
            <EmptyState>{copy.list.empty}</EmptyState>
          )}
        </section>
      </div>
    </div>
  );
}

function getRuntimeMessage(status: AutoScanStatus | null, copy: PageCopy) {
  if (status?.isAutoScanRunning) {
    return copy.runtime.running;
  }

  if (status?.scannerBusy) {
    return copy.runtime.busy;
  }

  return copy.runtime.idle;
}

function buildStatusClassName(status: ScanRunStatus) {
  return `scan-records-pill scan-records-pill-status-${status}`;
}

function buildRuntimeStateClassName(status: AutoScanStatus | null) {
  if (status?.isAutoScanRunning) {
    return 'scan-records-runtime-state scan-records-runtime-state-running';
  }

  if (status?.scannerBusy) {
    return 'scan-records-runtime-state scan-records-runtime-state-busy';
  }

  return 'scan-records-runtime-state';
}

function formatDateTime(value: string | null, language: 'zh-CN' | 'en-US', fallback: string) {
  if (!value) {
    return fallback;
  }

  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return fallback;
  }

  return new Intl.DateTimeFormat(language, {
    dateStyle: 'medium',
    timeStyle: 'short',
  }).format(date);
}

function formatRunDuration(
  value: number | null,
  locale: 'zh' | 'en' | 'zh-CN' | 'en-US',
  fallback: string,
) {
  if (value == null || value < 0) {
    return fallback;
  }

  if (value < 1000) {
    return `${value} ms`;
  }

  const totalSeconds = Math.round(value / 1000);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;

  if (minutes === 0) {
    return locale === 'zh' || locale === 'zh-CN' ? `${seconds}秒` : `${seconds}s`;
  }

  if (locale === 'zh' || locale === 'zh-CN') {
    return `${minutes}分 ${seconds}秒`;
  }

  return `${minutes}m ${seconds}s`;
}
