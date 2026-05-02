import { useEffect, useMemo, useRef, useState } from 'react';
import { ChevronDown, ChevronLeft, ChevronRight, Layers3, Workflow } from 'lucide-react';
import { EmptyState } from '../../components/empty-state/EmptyState';
import { useI18n } from '../../i18n/useI18n';
import { buildPageTokens } from '../../lib/pagination';
import type { SourceApp } from '../../lib/sourceApps';
import { useEnabledSourceApps } from '../../lib/useEnabledSourceApps';
import { fetchSessionsList } from '../sessions/sessionService';
import type { SessionRecord } from '../sessions/sessionData';
import {
  ensureSessionMessagesIndexed,
  fetchSessionMessages,
  type MessageSessionSummary,
  type RequestRecord,
  type UsageEventRecord,
} from './messageService';
import './MessagesPage.css';

type ViewMode = 'turn' | 'call';
type ViewRow = { kind: 'turn' | 'request'; request: RequestRecord };
type MessagesFeedbackKey = keyof ReturnType<typeof buildRequestCallsCopy>['feedback'];
type MessagesFeedback =
  | { kind: 'literal'; text: string }
  | { kind: 'key'; key: MessagesFeedbackKey }
  | null;

const requestViewModes: ViewMode[] = ['turn', 'call'];

function isIndexedSourceState(
  value: SessionRecord['sourceState'] | MessageSessionSummary['sessionSourceState'],
) {
  return value === 'synced' || value === 'archived';
}

export function MessagesPage() {
  const { locale, t } = useI18n();
  const enabledSourceApps = useEnabledSourceApps();
  const isZh = locale === 'zh';
  const numberFormatter = useMemo(() => new Intl.NumberFormat(isZh ? 'zh-CN' : 'en-US'), [isZh]);
  const dateFormatter = useMemo(
    () =>
      new Intl.DateTimeFormat(isZh ? 'zh-CN' : 'en-US', {
        year: 'numeric',
        month: 'short',
        day: 'numeric',
        hour: '2-digit',
        minute: '2-digit',
      }),
    [isZh],
  );
  const copy = useMemo(() => buildRequestCallsCopy(t), [t]);

  const [selectedApp, setSelectedApp] = useState<SourceApp | ''>('');
  const [sessionOptions, setSessionOptions] = useState<SessionRecord[]>([]);
  const [selectedSessionId, setSelectedSessionId] = useState('');
  const [viewMode, setViewMode] = useState<ViewMode>('turn');
  const [page, setPage] = useState(1);
  const [rowsPerPage, setRowsPerPage] = useState(25);
  const [requests, setRequests] = useState<RequestRecord[]>([]);
  const [usageEvents, setUsageEvents] = useState<UsageEventRecord[]>([]);
  const [sessionSummary, setSessionSummary] = useState<MessageSessionSummary | null>(null);
  const [isSessionsLoading, setIsSessionsLoading] = useState(false);
  const [isMessagesLoading, setIsMessagesLoading] = useState(false);
  const [isAutoScanRunning, setIsAutoScanRunning] = useState(false);
  const [sessionsError, setSessionsError] = useState<MessagesFeedback>(null);
  const [messagesError, setMessagesError] = useState<MessagesFeedback>(null);
  const [messagesInfo, setMessagesInfo] = useState<MessagesFeedback>(null);
  const autoScannedSessionsRef = useRef<Set<string>>(new Set());

  useEffect(() => {
    if (selectedApp && !enabledSourceApps.includes(selectedApp)) {
      setSelectedApp('');
    }
  }, [enabledSourceApps, selectedApp]);

  useEffect(() => {
    setSelectedSessionId('');
    setRequests([]);
    setUsageEvents([]);
    setSessionSummary(null);
    setMessagesInfo(null);

    if (!selectedApp) {
      setSessionOptions([]);
      setSessionsError(null);
      return;
    }

    let cancelled = false;

    async function loadSessions() {
      try {
        setIsSessionsLoading(true);
        setSessionsError(null);
        const result = await fetchSessionsList({
          page: 1,
          pageSize: 100,
          sourceApps: selectedApp ? [selectedApp] : undefined,
          sortBy: 'lastUpdated',
          sortOrder: 'desc',
        });

        if (cancelled) {
          return;
        }

        setSessionOptions(result.items);
      } catch (error) {
        if (cancelled) {
          return;
        }

        setSessionOptions([]);
        setSessionsError(
          error instanceof Error
            ? { kind: 'literal', text: error.message }
            : { kind: 'key', key: 'loadSessionsFailed' },
        );
      } finally {
        if (!cancelled) {
          setIsSessionsLoading(false);
        }
      }
    }

    void loadSessions();

    return () => {
      cancelled = true;
    };
  }, [selectedApp]);

  useEffect(() => {
    if (!selectedSessionId) {
      setRequests([]);
      setUsageEvents([]);
      setSessionSummary(null);
      setMessagesError(null);
      setMessagesInfo(null);
      return;
    }

    let cancelled = false;
    const activeSession =
      sessionOptions.find((session) => session.id === selectedSessionId) ?? null;
    const sessionAttemptKey = `${selectedApp}:${selectedSessionId}`;
    const canAutoScan =
      !!selectedApp &&
      !!activeSession &&
      isIndexedSourceState(activeSession.sourceState) &&
      !autoScannedSessionsRef.current.has(sessionAttemptKey);

    async function loadMessages() {
      try {
        setIsMessagesLoading(true);
        setMessagesError(null);
        setMessagesInfo(null);

        let result = await fetchSessionMessages({
          sessionId: selectedSessionId,
        });

        let autoScanRan = false;
        let autoScanFailed = false;
        const needsBackfill = result.requests.length === 0 || result.usageEvents.length === 0;

        if (canAutoScan && needsBackfill && selectedApp) {
          autoScannedSessionsRef.current.add(sessionAttemptKey);
          autoScanRan = true;
          setIsAutoScanRunning(true);
          setMessagesInfo(
            result.requests.length === 0
              ? { kind: 'key', key: 'rebuildMissingIndex' }
              : { kind: 'key', key: 'rebuildMissingCalls' },
          );

          try {
            const ensured = await ensureSessionMessagesIndexed(selectedSessionId);
            if (cancelled) {
              return;
            }

            if (!ensured) {
              autoScanFailed = true;
              setMessagesInfo({ kind: 'key', key: 'rebuildSourceMissing' });
              return;
            }

            result = await fetchSessionMessages({
              sessionId: selectedSessionId,
            });
          } catch (error) {
            if (cancelled) {
              return;
            }

            autoScanFailed = true;
            setMessagesInfo(
              error instanceof Error
                ? { kind: 'literal', text: error.message }
                : { kind: 'key', key: 'rebuildFailed' },
            );
          } finally {
            if (!cancelled) {
              setIsAutoScanRunning(false);
            }
          }
        }

        if (cancelled) {
          return;
        }

        setRequests(result.requests);
        setUsageEvents(result.usageEvents);
        setSessionSummary(result.session);

        if (result.requests.length > 0 || result.usageEvents.length > 0) {
          setMessagesInfo(null);
        } else if (activeSession?.sourceState && !isIndexedSourceState(activeSession.sourceState)) {
          setMessagesInfo({ kind: 'key', key: 'unsynced' });
        } else if (autoScanRan && !autoScanFailed) {
          setMessagesInfo({ kind: 'key', key: 'rebuildFinishedEmpty' });
        } else if ((result.session?.sessionTotalMessages ?? activeSession?.messages ?? 0) === 0) {
          setMessagesInfo({ kind: 'key', key: 'emptySession' });
        } else if (viewMode === 'call' && result.usageEvents.length === 0) {
          setMessagesInfo({ kind: 'key', key: 'noCallUsage' });
        }
      } catch (error) {
        if (cancelled) {
          return;
        }

        setRequests([]);
        setUsageEvents([]);
        setSessionSummary(null);
        setMessagesError(
          error instanceof Error
            ? { kind: 'literal', text: error.message }
            : { kind: 'key', key: 'loadMessagesFailed' },
        );
      } finally {
        if (!cancelled) {
          setIsMessagesLoading(false);
          setIsAutoScanRunning(false);
        }
      }
    }

    void loadMessages();

    return () => {
      cancelled = true;
    };
  }, [selectedApp, selectedSessionId, sessionOptions, viewMode]);

  const sessionsErrorText = resolveMessagesFeedback(sessionsError, copy);
  const messagesErrorText = resolveMessagesFeedback(messagesError, copy);
  const messagesInfoText = resolveMessagesFeedback(messagesInfo, copy);

  const selectedSession = useMemo(
    () => sessionOptions.find((session) => session.id === selectedSessionId) ?? null,
    [selectedSessionId, sessionOptions],
  );

  const requestRows = useMemo<RequestRecord[]>(() => {
    const sessionName = sessionSummary?.sessionName ?? selectedSession?.name ?? '-';

    return usageEvents.map((event, index) => ({
      id: `usage:${event.id}`,
      sessionId: event.sessionId,
      sessionName,
      sourceApp: event.sourceApp,
      sequenceNo: index + 1,
      status: event.granularity,
      messageCount: 0,
      model: event.model,
      inputTokens: event.deltaInput,
      outputTokens: event.deltaOutput,
      totalTokens: event.deltaTotal,
      cacheReadInputTokens: event.cacheReadInputTokens,
      cacheWriteInputTokens: event.cacheWriteInputTokens,
      estimatedCostUsd: event.estimatedCostUsd,
      tokenConfidence: event.confidence,
      createdAt: event.eventTimeUtc,
      updatedAt: event.eventTimeUtc,
      sourceLocatorLabel: describeUsageEventLabel(event, index, copy),
    }));
  }, [copy, selectedSession?.name, sessionSummary?.sessionName, usageEvents]);

  const rows = useMemo<ViewRow[]>(
    () =>
      (viewMode === 'call' ? requestRows : requests).map((request) => ({
        kind: viewMode === 'call' ? 'request' : 'turn',
        request,
      })),
    [requestRows, requests, viewMode],
  );

  const totalItems = rows.length;
  const totalPages = Math.max(1, Math.ceil(totalItems / rowsPerPage));
  const pageRows = useMemo(
    () => rows.slice((page - 1) * rowsPerPage, page * rowsPerPage),
    [page, rows, rowsPerPage],
  );
  const pageTokens = useMemo(() => buildPageTokens(page, totalPages), [page, totalPages]);

  useEffect(() => {
    setPage(1);
  }, [selectedApp, selectedSessionId, viewMode, rowsPerPage]);

  useEffect(() => {
    if (page > totalPages) {
      setPage(totalPages);
    }
  }, [page, totalPages]);

  return (
    <div className="messages-page">
      <section className="messages-hero">
        <div className="messages-hero-copy">
          <h1 className="page-title">{t('nav.messages')}</h1>
          <p className="page-subtitle">{copy.subtitle}</p>
        </div>
      </section>

      <section className="messages-filters">
        <div className="messages-select-group">
          <label className="messages-field" aria-label={copy.filters.app}>
            <div className="ui-select-wrap messages-select-wrap">
              <select
                className="messages-select"
                value={selectedApp}
                onChange={(event) => setSelectedApp(event.target.value as SourceApp | '')}
              >
                <option value="">{copy.filters.selectApp}</option>
                {enabledSourceApps.map((app) => (
                  <option key={app} value={app}>
                    {formatAppLabel(app, t)}
                  </option>
                ))}
              </select>
              <ChevronDown size={16} />
            </div>
          </label>

          <label className="messages-field" aria-label={copy.filters.session}>
            <div className="ui-select-wrap messages-select-wrap">
              <select
                className="messages-select"
                value={selectedSessionId}
                onChange={(event) => setSelectedSessionId(event.target.value)}
                disabled={!selectedApp || isSessionsLoading || sessionOptions.length === 0}
              >
                <option value="">
                  {!selectedApp
                    ? copy.filters.selectAppFirst
                    : isSessionsLoading
                      ? copy.filters.loadingSessions
                      : copy.filters.selectSession}
                </option>
                {sessionOptions.map((session) => (
                  <option key={session.id} value={session.id} title={session.name}>
                    {truncateSessionSelectLabel(session.name)}
                  </option>
                ))}
              </select>
              <ChevronDown size={16} />
            </div>
          </label>
        </div>

        <div
          className="ui-segmented messages-role-tabs"
          role="tablist"
          aria-label={copy.sections.streamTitle}
        >
          {requestViewModes.map((mode) => (
            <button
              key={mode}
              type="button"
              className={viewMode === mode ? 'ui-segmented-active' : undefined}
              onClick={() => setViewMode(mode)}
              disabled={!selectedSessionId}
            >
              {describeRequestViewMode(mode, copy)}
            </button>
          ))}
        </div>
      </section>

      {sessionsErrorText ? <div className="messages-feedback">{sessionsErrorText}</div> : null}
      {messagesErrorText ? <div className="messages-feedback">{messagesErrorText}</div> : null}
      {messagesInfoText ? (
        <div className="messages-feedback messages-feedback-info">{messagesInfoText}</div>
      ) : null}

      <section className="messages-layout">
        <div className="messages-stream-card">
          <header className="messages-card-header">
            <div>
              <div className="messages-card-heading">
                <h2>{describeRequestViewMode(viewMode, copy)}</h2>
              </div>
              <span>
                {selectedSession
                  ? truncateSessionSelectLabel(selectedSession.name, 48)
                  : copy.sections.noSessionSelected}
              </span>
            </div>
          </header>

          {!selectedApp ? (
            <EmptyState variant="fill" icon={<Layers3 size={20} />}>
              {copy.empty.selectApp}
            </EmptyState>
          ) : null}

          {selectedApp && !selectedSessionId && !isSessionsLoading ? (
            <EmptyState variant="fill" icon={<Workflow size={20} />}>
              {copy.empty.selectSession}
            </EmptyState>
          ) : null}

          {selectedSessionId && isMessagesLoading ? (
            <EmptyState variant="fill">
              {isAutoScanRunning ? copy.empty.rebuilding : copy.empty.loading}
            </EmptyState>
          ) : null}

          {selectedSessionId && !isMessagesLoading && rows.length === 0 ? (
            <EmptyState variant="fill">
              {viewMode === 'call' ? copy.empty.noCalls : copy.empty.noRequests}
            </EmptyState>
          ) : null}

          {selectedSessionId && !isMessagesLoading && rows.length > 0 ? (
            <>
              <div className="messages-table-wrap">
                <table className="messages-grid-table">
                  <thead>
                    <tr>
                      <th>#</th>
                      <th>{copy.detail.level}</th>
                      <th>{copy.detail.locator}</th>
                      <th>{copy.detail.status}</th>
                      <th>{copy.detail.context}</th>
                      <th>{copy.detail.model}</th>
                      <th>{copy.detail.inputTokens}</th>
                      <th>{copy.detail.outputTokens}</th>
                      <th>{copy.detail.totalTokens}</th>
                      <th>{copy.detail.estimatedCost}</th>
                      <th>{copy.detail.updated}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {pageRows.map((row) => {
                      const rowId = getRowId(row);

                      return (
                        <tr key={rowId} className="messages-table-row">
                          <td className="messages-table-seq-cell">
                            <span className="messages-table-seq-chip">
                              {getRowSequenceLabel(row)}
                            </span>
                          </td>
                          <td>
                            <span className={getRowLevelClassName(row)}>
                              {getRowLevelLabel(row, copy)}
                            </span>
                          </td>
                          <td className="messages-table-summary-cell">
                            <div
                              className="messages-table-summary-main"
                              title={row.request.sourceLocatorLabel}
                            >
                              {truncateText(row.request.sourceLocatorLabel, 42)}
                            </div>
                          </td>
                          <td
                            className="messages-table-status-cell"
                            title={getRowStatusText(row, copy)}
                          >
                            {getRowStatusText(row, copy)}
                          </td>
                          <td
                            className="messages-table-context-cell"
                            title={getRowContextText(row, copy, numberFormatter)}
                          >
                            {getRowContextText(row, copy, numberFormatter)}
                          </td>
                          <td className="messages-table-model-cell" title={getRowModelText(row)}>
                            {getRowModelText(row)}
                          </td>
                          <td className="messages-table-number-cell">
                            {formatNumber(row.request.inputTokens, numberFormatter)}
                          </td>
                          <td className="messages-table-number-cell">
                            {formatNumber(row.request.outputTokens, numberFormatter)}
                          </td>
                          <td className="messages-table-number-cell">
                            {formatNumber(row.request.totalTokens, numberFormatter)}
                          </td>
                          <td className="messages-table-number-cell">
                            {formatCurrency(row.request.estimatedCostUsd)}
                          </td>
                          <td className="messages-table-updated-cell">
                            {formatDate(
                              row.request.updatedAt ?? row.request.createdAt,
                              dateFormatter,
                            )}
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>

              <footer className="messages-pagination">
                <div className="messages-page-size">
                  <span>{t('session.rows')}:</span>
                  <div className="messages-pagination-select ui-select-wrap ui-select-wrap-compact">
                    <select
                      value={rowsPerPage}
                      onChange={(event) => setRowsPerPage(Number(event.target.value))}
                    >
                      <option value={10}>10</option>
                      <option value={25}>25</option>
                      <option value={50}>50</option>
                    </select>
                    <ChevronDown size={16} />
                  </div>
                  <div className="messages-count">
                    {numberFormatter.format(totalItems)}{' '}
                    {viewMode === 'call' ? copy.sections.callsUnit : copy.sections.requestsUnit}
                  </div>
                </div>

                <div className="messages-page-controls">
                  <button
                    type="button"
                    className="messages-page-btn"
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
                        className="messages-page-btn"
                        aria-current={page === token ? 'page' : undefined}
                        onClick={() => setPage(token)}
                      >
                        {token}
                      </button>
                    ) : (
                      <span key={token} className="messages-page-ellipsis">
                        ...
                      </span>
                    ),
                  )}

                  <button
                    type="button"
                    className="messages-page-btn"
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
        </div>
      </section>
    </div>
  );
}

function getRowId(row: ViewRow) {
  return `${row.kind}:${row.request.id}`;
}

function getRowSequenceLabel(row: ViewRow) {
  return `${row.kind === 'request' ? 'R' : 'T'}${row.request.sequenceNo}`;
}

function getRowLevelLabel(row: ViewRow, copy: ReturnType<typeof buildRequestCallsCopy>) {
  return row.kind === 'request' ? copy.detail.levelCall : copy.detail.levelRequest;
}

function getRowLevelClassName(row: ViewRow) {
  return row.kind === 'request'
    ? 'messages-level-pill messages-level-pill-call'
    : 'messages-level-pill messages-level-pill-request';
}

function getRowStatusText(row: ViewRow, copy: ReturnType<typeof buildRequestCallsCopy>) {
  return row.request.status ?? copy.detail.unknown;
}

function getRowContextText(
  row: ViewRow,
  copy: ReturnType<typeof buildRequestCallsCopy>,
  formatter: Intl.NumberFormat,
) {
  if (row.kind === 'request') {
    return row.request.tokenConfidence
      ? `${copy.row.confidence} ${row.request.tokenConfidence}`
      : '-';
  }

  return row.request.messageCount > 0
    ? `${copy.chip.messages} ${formatter.format(row.request.messageCount)}`
    : '-';
}

function getRowModelText(row: ViewRow) {
  return row.request.model ?? '-';
}

function describeRequestViewMode(mode: ViewMode, copy: ReturnType<typeof buildRequestCallsCopy>) {
  return mode === 'call' ? copy.view.call : copy.view.turn;
}

function describeUsageEventLabel(
  event: UsageEventRecord,
  index: number,
  copy: ReturnType<typeof buildRequestCallsCopy>,
) {
  const suffix = event.sourceEventId?.trim() || event.model?.trim() || event.granularity;
  return `${copy.row.call} ${index + 1} - ${suffix}`;
}

function formatAppLabel(app: SourceApp, t: (key: string) => string) {
  return t(`session.source.${app}`);
}

function formatDate(value: string | null, formatter: Intl.DateTimeFormat) {
  if (!value) {
    return '-';
  }

  return formatter.format(new Date(value));
}

function formatNumber(value: number | null | undefined, formatter: Intl.NumberFormat) {
  if (value === null || value === undefined) {
    return '-';
  }

  return formatter.format(value);
}

function formatCurrency(value: number | null | undefined) {
  if (value === null || value === undefined) {
    return '-';
  }

  return formatUsdAmount(value);
}

function formatUsdAmount(value: number) {
  const sign = value < 0 ? '-' : '';
  const formatted = new Intl.NumberFormat('en-US', {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }).format(Math.abs(value));
  return `${sign}$${formatted}`;
}

function truncateSessionSelectLabel(value: string, maxLength = 24) {
  if (value.length <= maxLength) {
    return value;
  }

  return `${value.slice(0, maxLength - 3)}...`;
}

function truncateText(value: string, maxLength: number) {
  if (value.length <= maxLength) {
    return value;
  }

  return `${value.slice(0, maxLength - 3)}...`;
}

function resolveMessagesFeedback(
  feedback: MessagesFeedback,
  copy: ReturnType<typeof buildRequestCallsCopy>,
) {
  if (!feedback) {
    return null;
  }

  if (feedback.kind === 'literal') {
    return feedback.text;
  }

  return copy.feedback[feedback.key];
}

function buildRequestCallsCopy(t: (key: string) => string) {
  return {
    subtitle: t('messagePage.subtitle'),
    filters: {
      app: t('messagePage.filters.app'),
      session: t('messagePage.filters.session'),
      selectApp: t('messagePage.filters.selectApp'),
      selectAppFirst: t('messagePage.filters.selectAppFirst'),
      loadingSessions: t('messagePage.filters.loadingSessions'),
      selectSession: t('messagePage.filters.selectSession'),
    },
    feedback: {
      loadSessionsFailed: t('messagePage.feedback.loadSessionsFailed'),
      rebuildMissingIndex: t('messagePage.feedback.rebuildMissingIndex'),
      rebuildMissingCalls: t('messagePage.feedback.rebuildMissingCalls'),
      rebuildSourceMissing: t('messagePage.feedback.rebuildSourceMissing'),
      rebuildFailed: t('messagePage.feedback.rebuildFailed'),
      unsynced: t('messagePage.feedback.unsynced'),
      rebuildFinishedEmpty: t('messagePage.feedback.rebuildFinishedEmpty'),
      emptySession: t('messagePage.feedback.emptySession'),
      noCallUsage: t('messagePage.feedback.noCallUsage'),
      loadMessagesFailed: t('messagePage.feedback.loadMessagesFailed'),
    },
    view: {
      turn: t('messagePage.view.turn'),
      call: t('messagePage.view.call'),
    },
    sections: {
      streamTitle: t('messagePage.sections.streamTitle'),
      noSessionSelected: t('messagePage.sections.noSessionSelected'),
      requestsUnit: t('messagePage.sections.requestsUnit'),
      callsUnit: t('messagePage.sections.callsUnit'),
    },
    empty: {
      selectApp: t('messagePage.empty.selectApp'),
      selectSession: t('messagePage.empty.selectSession'),
      loading: t('messagePage.empty.loading'),
      rebuilding: t('messagePage.empty.rebuilding'),
      noCalls: t('messagePage.empty.noCalls'),
      noRequests: t('messagePage.empty.noRequests'),
    },
    row: {
      call: t('messagePage.row.call'),
      request: t('messagePage.row.request'),
      confidence: t('messagePage.row.confidence'),
    },
    chip: {
      messages: t('messagePage.chip.messages'),
    },
    detail: {
      level: t('messagePage.detail.level'),
      levelCall: t('messagePage.detail.levelCall'),
      levelRequest: t('messagePage.detail.levelRequest'),
      app: t('messagePage.detail.app'),
      session: t('messagePage.detail.session'),
      callNo: t('messagePage.detail.callNo'),
      requestNo: t('messagePage.detail.requestNo'),
      granularity: t('messagePage.detail.granularity'),
      status: t('messagePage.detail.status'),
      context: t('messagePage.detail.context'),
      unknown: t('messagePage.detail.unknown'),
      model: t('messagePage.detail.model'),
      inputTokens: t('messagePage.detail.inputTokens'),
      outputTokens: t('messagePage.detail.outputTokens'),
      totalTokens: t('messagePage.detail.totalTokens'),
      cacheReadTokens: t('messagePage.detail.cacheReadTokens'),
      cacheWriteTokens: t('messagePage.detail.cacheWriteTokens'),
      estimatedCost: t('messagePage.detail.estimatedCost'),
      messageCount: t('messagePage.detail.messageCount'),
      confidence: t('messagePage.detail.confidence'),
      locator: t('messagePage.detail.locator'),
      updated: t('messagePage.detail.updated'),
    },
  };
}
