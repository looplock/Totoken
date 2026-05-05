import { useEffect, useMemo, useRef, useState } from 'react';
import { AppIcon } from '../../components/app-icon/AppIcon';
import { EmptyState } from '../../components/empty-state/EmptyState';
import { Switch } from '../../components/switch/Switch';
import { useI18n } from '../../i18n/useI18n';
import type { SourceApp } from '../../lib/sourceApps';
import { isTauriRuntime } from '../../lib/tauri';
import { useRefreshEnabledSourceApps } from '../../lib/useEnabledSourceApps';
import type { SourceRecord, SourceState } from './sourceData';
import { fetchSources, updateSource } from './sourceService';
import './SourcesPage.css';

type ScopeEntry = {
  key: string;
  label: string;
  path: string;
  exists: boolean;
};

const experimentalSourceApps = new Set<SourceApp>(['cursor', 'kilocode', 'kiro']);

export function SourcesPage() {
  const { t } = useI18n();
  const refreshEnabledSourceApps = useRefreshEnabledSourceApps();
  const [sources, setSources] = useState<SourceRecord[]>([]);
  const [selectedSourceId, setSelectedSourceId] = useState('');
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState('');
  const [saveError, setSaveError] = useState('');
  const requestSeqRef = useRef(0);

  const selectedSource = useMemo(
    () => sources.find((source) => source.id === selectedSourceId) ?? sources[0] ?? null,
    [selectedSourceId, sources],
  );

  const scopeEntries = selectedSource ? buildScopeEntries(selectedSource, t) : [];

  useEffect(() => {
    let cancelled = false;
    const requestSeq = ++requestSeqRef.current;

    async function load() {
      setLoading(true);
      setLoadError('');

      try {
        const state = await fetchSources();
        if (!cancelled && requestSeq === requestSeqRef.current) {
          applySourceState(state);
        }
      } catch (error) {
        if (!cancelled && requestSeq === requestSeqRef.current) {
          setLoadError(error instanceof Error ? error.message : t('sources.feedback.loadFailed'));
        }
      } finally {
        if (!cancelled && requestSeq === requestSeqRef.current) {
          setLoading(false);
        }
      }
    }

    if (!isTauriRuntime()) {
      setLoading(false);
      setLoadError(t('sources.feedback.runtimeRequired'));
      return undefined;
    }

    void load();
    return () => {
      cancelled = true;
    };
  }, [t]);

  function applySourceState(state: SourceState) {
    setSources(state.items);
    setSelectedSourceId((current) =>
      state.items.some((item) => item.id === current) ? current : (state.items[0]?.id ?? ''),
    );
    setSaveError('');
  }

  async function handleToggle(source: SourceRecord) {
    const requestSeq = ++requestSeqRef.current;
    try {
      const state = await updateSource(source.id, { enabled: !source.enabled });
      if (requestSeq === requestSeqRef.current) {
        applySourceState(state);
        try {
          await refreshEnabledSourceApps();
        } catch (error) {
          console.error('failed to refresh enabled source apps', error);
        }
      }
    } catch (error) {
      if (requestSeq === requestSeqRef.current) {
        setSaveError(error instanceof Error ? error.message : t('sources.feedback.saveFailed'));
      }
    }
  }

  const detailRows = selectedSource ? buildSourceDetailRows(selectedSource, t) : [];

  return (
    <div className="sources-page">
      <section className="sources-hero">
        <div>
          <h1 className="page-title">{t('sources.title')}</h1>
          <p className="page-subtitle">{t('sources.subtitle')}</p>
        </div>
      </section>

      <div className="sources-layout">
        <section className="sources-card sources-config-card">
          <header className="sources-card-header">
            <div>
              <h2 className="sources-card-title">{t('sources.config.title')}</h2>
              <p className="sources-card-subtitle">{t('sources.config.note')}</p>
            </div>
          </header>

          <div className="sources-table-wrap">
            <table className="sources-table">
              <colgroup>
                <col className="sources-col-name" />
                <col className="sources-col-enabled" />
                <col className="sources-col-path" />
              </colgroup>
              <thead>
                <tr>
                  <th>{t('sources.header.name')}</th>
                  <th>{t('sources.header.enabled')}</th>
                  <th>{t('sources.header.path')}</th>
                </tr>
              </thead>
              <tbody>
                {sources.map((source) => {
                  const active = selectedSource?.id === source.id;
                  const sourceName = formatSourceName(source.app, t);
                  return (
                    <tr
                      key={source.id}
                      className={active ? 'sources-row-active' : undefined}
                      onClick={() => setSelectedSourceId(source.id)}
                    >
                      <td>
                        <span className="sources-app-cell">
                          <AppIcon app={source.app} label={sourceName} />
                          <SourceNameWithBadges source={source} sourceName={sourceName} t={t} />
                        </span>
                      </td>
                      <td>
                        <Switch
                          checked={source.enabled}
                          onToggle={() => void handleToggle(source)}
                          label={t('sources.header.enabled')}
                        />
                      </td>
                      <td>
                        <div
                          className={
                            source.rootPathExists
                              ? 'sources-path-cell'
                              : 'sources-path-cell sources-path-cell-missing'
                          }
                        >
                          <span className="sources-path-text" title={source.rootPath}>
                            {source.rootPath}
                          </span>
                        </div>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>

          <div className="sources-card-note">
            {loadError || saveError || t('sources.config.note')}
          </div>
        </section>

        <div className="sources-side">
          <section className="sources-card sources-overview-card">
            <header className="sources-overview-header">
              <div>
                <h2 className="sources-overview-title">
                  {selectedSource ? (
                    <SourceNameWithBadges
                      source={selectedSource}
                      sourceName={formatSourceName(selectedSource.app, t)}
                      t={t}
                    />
                  ) : (
                    t('sources.empty')
                  )}
                </h2>
                <p className="sources-overview-path">{selectedSource?.rootPath}</p>
              </div>
            </header>

            <div className="sources-overview-body">
              {loading ? (
                <EmptyState>{t('sources.feedback.loading')}</EmptyState>
              ) : selectedSource ? (
                <>
                  <div className="sources-status-grid">
                    <StatusCard
                      label={t('sources.detail.pathStatus')}
                      value={
                        selectedSource.rootPathExists
                          ? t('sources.status.available')
                          : t('sources.status.missing')
                      }
                      tone={selectedSource.rootPathExists ? 'success' : 'danger'}
                    />
                    <StatusCard
                      label={t('sources.detail.enabled')}
                      value={
                        selectedSource.enabled
                          ? t('sources.status.enabled')
                          : t('sources.status.disabled')
                      }
                      tone={selectedSource.enabled ? 'success' : 'neutral'}
                    />
                    <StatusCard
                      label={t('sources.detail.scanSupported')}
                      value={
                        selectedSource.scanSupported
                          ? t('sources.status.supported')
                          : t('sources.status.unsupported')
                      }
                      tone={selectedSource.scanSupported ? 'success' : 'neutral'}
                    />
                  </div>

                  <section className="sources-panel">
                    <div className="sources-panel-header">
                      <h3 className="sources-panel-title">{t('sources.scope.title')}</h3>
                      <p className="sources-panel-subtitle">{t('sources.scope.note')}</p>
                    </div>

                    <div className="sources-scope-list">
                      {scopeEntries.map((entry) => (
                        <div key={entry.key} className="sources-scope-item">
                          <div className="sources-scope-item-top">
                            <div className="sources-scope-label-wrap">
                              <span className="sources-scope-label">{entry.label}</span>
                            </div>
                            <div className="sources-scope-badges">
                              <span
                                className={
                                  entry.exists
                                    ? 'sources-scope-badge sources-scope-badge-available'
                                    : 'sources-scope-badge sources-scope-badge-missing'
                                }
                              >
                                {entry.exists
                                  ? t('sources.status.available')
                                  : t('sources.status.missing')}
                              </span>
                            </div>
                          </div>
                          <div className="sources-scope-path">{entry.path}</div>
                        </div>
                      ))}
                    </div>
                  </section>
                </>
              ) : (
                <EmptyState>{t('sources.empty')}</EmptyState>
              )}
            </div>
          </section>

          <section className="sources-card sources-detail-card">
            <header className="sources-detail-header">{t('sources.detail.title')}</header>
            {selectedSource ? (
              <div className="sources-detail-grid">
                {detailRows.map((row) => (
                  <div key={row.label} className="sources-detail-item">
                    <span className="sources-detail-label">{row.label}</span>
                    <span
                      className={
                        row.tone === 'success'
                          ? 'sources-detail-value sources-detail-value-success'
                          : row.tone === 'danger'
                            ? 'sources-detail-value sources-detail-value-danger'
                            : 'sources-detail-value'
                      }
                    >
                      {row.value}
                    </span>
                  </div>
                ))}
              </div>
            ) : (
              <EmptyState>{t('sources.empty')}</EmptyState>
            )}
          </section>
        </div>
      </div>
    </div>
  );
}

function SourceNameWithBadges({
  source,
  sourceName,
  t,
}: {
  source: SourceRecord;
  sourceName: string;
  t: (key: string) => string;
}) {
  return (
    <span className="sources-name-wrap">
      <span className="sources-name-text">{sourceName}</span>
      {experimentalSourceApps.has(source.app) ? (
        <span className="sources-experimental-badge" title={t('sources.experimental.tooltip')}>
          {t('sources.experimental.badge')}
        </span>
      ) : null}
    </span>
  );
}

function StatusCard({
  label,
  value,
  tone,
}: {
  label: string;
  value: string;
  tone: 'success' | 'danger' | 'neutral';
}) {
  return (
    <div className="sources-status-card">
      <span className="sources-status-label">{label}</span>
      <strong
        className={
          tone === 'success'
            ? 'sources-status-value sources-status-value-success'
            : tone === 'danger'
              ? 'sources-status-value sources-status-value-danger'
              : 'sources-status-value'
        }
      >
        {value}
      </strong>
    </div>
  );
}

function buildScopeEntries(source: SourceRecord, t: (key: string) => string): ScopeEntry[] {
  return source.scanPaths.map((scanPath, index) => ({
    key: `${source.id}-scan-path-${index}`,
    label:
      source.scanPaths.length === 1
        ? t('sources.scope.path')
        : `${t('sources.scope.path')} ${index + 1}`,
    path: scanPath.path,
    exists: scanPath.exists,
  }));
}

function buildSourceDetailRows(
  source: SourceRecord,
  t: (key: string) => string,
): Array<{ label: string; value: string; tone?: 'success' | 'danger' }> {
  const rows: Array<{ label: string; value: string; tone?: 'success' | 'danger' }> = [
    { label: t('sources.detail.name'), value: formatSourceName(source.app, t) },
    { label: t('sources.detail.path'), value: source.rootPath },
    {
      label: t('sources.detail.pathStatus'),
      value: source.rootPathExists ? t('sources.status.available') : t('sources.status.missing'),
      tone: source.rootPathExists ? 'success' : 'danger',
    },
    {
      label: t('sources.detail.enabled'),
      value: source.enabled ? t('sources.status.enabled') : t('sources.status.disabled'),
      tone: source.enabled ? 'success' : undefined,
    },
    {
      label: t('sources.detail.scanSupported'),
      value: source.scanSupported ? t('sources.status.supported') : t('sources.status.unsupported'),
    },
    {
      label: t('sources.scope.title'),
      value: String(source.scanPaths.length),
    },
  ];

  return rows;
}

function formatSourceName(sourceApp: SourceRecord['app'], t: (key: string) => string): string {
  return t(`session.source.${sourceApp}`);
}
