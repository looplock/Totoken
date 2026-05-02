import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Archive,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  CircleAlert,
  CircleHelp,
  Copy,
  Database,
  FileText,
  Folder,
  HardDrive,
  Info,
  RefreshCw,
  Trash2,
  Wrench,
} from 'lucide-react';
import { useI18n } from '../../i18n/useI18n';
import { EmptyState } from '../../components/empty-state/EmptyState';
import { ConfirmDialog } from '../../components/confirm-dialog/ConfirmDialog';
import { IconButton } from '../../components/icon-button/IconButton';
import { isTauriRuntime } from '../../lib/tauri';
import { fetchAppDataItemDetail, fetchAppDataOverview, runAppDataAction } from './appDataService';
import type {
  AppDataItem,
  AppDataItemDetail,
  AppDataMaintenanceAction,
  AppDataOverview,
} from './appDataTypes';
import './AppDataPage.css';

type NoticeTone = 'error' | 'success' | 'info';

function buildAppDataCopy(t: (key: string) => string) {
  return {
    title: t('appData.title'),
    subtitle: t('appData.subtitle'),
    runtimeRequired: t('appData.runtimeRequired'),
    loading: t('appData.loading'),
    loadFailed: t('appData.loadFailed'),
    detailFailed: t('appData.detailFailed'),
    refresh: t('appData.refresh'),
    copyPath: t('appData.copyPath'),
    copySuccess: t('appData.copySuccess'),
    restartRequired: t('appData.restartRequired'),
    sectionFolder: t('appData.sectionFolder'),
    sectionPreview: t('appData.sectionPreview'),
    editorHint: t('appData.editorHint'),
    emptySelection: t('appData.emptySelection'),
    emptyPreview: t('appData.emptyPreview'),
    treeHint: t('appData.treeHint'),
    collapseFolder: t('appData.collapseFolder'),
    expandFolder: t('appData.expandFolder'),
    clearCachesConfirmTitle: t('appData.clearCachesConfirmTitle'),
    clearCachesConfirmDescription: t('appData.clearCachesConfirmDescription'),
    clearCachesConfirmAccept: t('appData.clearCachesConfirmAccept'),
    clearCachesConfirmCancel: t('appData.clearCachesConfirmCancel'),
    reclaimedBytesLabel: t('appData.reclaimedBytesLabel'),
    actionSuccess: {
      create_backup: t('appData.actionSuccess.createBackup'),
      vacuum_database: t('appData.actionSuccess.vacuumDatabase'),
      rebuild_indexes: t('appData.actionSuccess.rebuildIndexes'),
      clear_caches: t('appData.actionSuccess.clearCaches'),
    } satisfies Record<AppDataMaintenanceAction, string>,
    actionLabel: {
      create_backup: t('appData.action.createBackup'),
      vacuum_database: t('appData.action.vacuumDatabase'),
      rebuild_indexes: t('appData.action.rebuildIndexes'),
      clear_caches: t('appData.action.clearCaches'),
    } satisfies Record<AppDataMaintenanceAction, string>,
  };
}

export function AppDataPage() {
  const { locale, t } = useI18n();
  const copy = useMemo(() => buildAppDataCopy(t), [t]);
  const language = locale === 'zh' ? 'zh-CN' : 'en-US';
  const [overview, setOverview] = useState<AppDataOverview | null>(null);
  const [detail, setDetail] = useState<AppDataItemDetail | null>(null);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [expandedPaths, setExpandedPaths] = useState<Set<string>>(new Set());
  const [loading, setLoading] = useState(true);
  const [detailLoading, setDetailLoading] = useState(false);
  const [runningAction, setRunningAction] = useState<AppDataMaintenanceAction | null>(null);
  const [pendingConfirmAction, setPendingConfirmAction] = useState<AppDataMaintenanceAction | null>(
    null,
  );
  const [notice, setNotice] = useState<{ tone: NoticeTone; message: string } | null>(null);
  const [detailReloadKey, setDetailReloadKey] = useState(0);
  const previewLines = useMemo(() => splitPreviewLines(detail?.preview), [detail?.preview]);

  const loadOverview = useCallback(
    async (silent: boolean, nextNotice?: { tone: NoticeTone; message: string } | null) => {
      if (!silent) {
        setLoading(true);
      }

      try {
        const nextOverview = await fetchAppDataOverview();
        setOverview(nextOverview);
        setSelectedPath((current) => {
          if (current && hasPath(nextOverview.items, current)) {
            return current;
          }
          return nextOverview.defaultSelectedPath ?? nextOverview.items[0]?.relativePath ?? null;
        });
        setExpandedPaths((current) =>
          mergeExpandedPaths(current, nextOverview.items, nextOverview.defaultSelectedPath ?? null),
        );
        setDetailReloadKey((current) => current + 1);
        setNotice(nextNotice ?? null);
      } catch (error) {
        setNotice({
          tone: 'error',
          message: error instanceof Error ? error.message : copy.loadFailed,
        });
      } finally {
        if (!silent) {
          setLoading(false);
        }
      }
    },
    [copy.loadFailed],
  );

  useEffect(() => {
    if (!isTauriRuntime()) {
      setLoading(false);
      setNotice({ tone: 'error', message: copy.runtimeRequired });
      return;
    }

    void loadOverview(false);
  }, [copy.runtimeRequired, loadOverview]);

  useEffect(() => {
    if (!selectedPath || !isTauriRuntime()) {
      setDetail(null);
      return;
    }

    let cancelled = false;
    setDetailLoading(true);

    void fetchAppDataItemDetail(selectedPath)
      .then((nextDetail) => {
        if (!cancelled) {
          setDetail(nextDetail);
        }
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setDetail(null);
          setNotice({
            tone: 'error',
            message: error instanceof Error ? error.message : copy.detailFailed,
          });
        }
      })
      .finally(() => {
        if (!cancelled) {
          setDetailLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [copy.detailFailed, detailReloadKey, selectedPath]);

  async function handleAction(action: AppDataMaintenanceAction) {
    if (action === 'clear_caches') {
      setPendingConfirmAction(action);
      return;
    }
    await executeAction(action);
  }

  async function executeAction(action: AppDataMaintenanceAction) {
    setRunningAction(action);
    setNotice(null);

    try {
      const outcome = await runAppDataAction(action);
      const nextOverview = outcome.overview;
      setOverview(nextOverview);
      setSelectedPath((current) => {
        if (current && hasPath(nextOverview.items, current)) {
          return current;
        }
        return nextOverview.defaultSelectedPath ?? nextOverview.items[0]?.relativePath ?? null;
      });
      setExpandedPaths((current) =>
        mergeExpandedPaths(current, nextOverview.items, nextOverview.defaultSelectedPath ?? null),
      );
      setDetailReloadKey((current) => current + 1);
      const successMessage =
        action === 'clear_caches' && outcome.reclaimedBytes != null
          ? `${copy.actionSuccess[action]} ${copy.reclaimedBytesLabel}: ${formatBytes(outcome.reclaimedBytes, language)}`
          : copy.actionSuccess[action];
      setNotice({ tone: 'success', message: successMessage });
    } catch (error) {
      setNotice({
        tone: 'error',
        message: error instanceof Error ? error.message : copy.loadFailed,
      });
      // The action may have partially mutated the data dir before failing —
      // refresh quietly so the tree/preview don't stay stale.
      void loadOverview(true);
    } finally {
      setRunningAction(null);
    }
  }

  async function handleCopyPath() {
    if (!detail?.fullPath || typeof navigator === 'undefined' || !navigator.clipboard) {
      return;
    }

    try {
      await navigator.clipboard.writeText(detail.fullPath);
      setNotice({ tone: 'info', message: copy.copySuccess });
    } catch (error) {
      setNotice({
        tone: 'error',
        message: error instanceof Error ? error.message : copy.detailFailed,
      });
    }
  }

  return (
    <div className="app-data-page">
      <section className="app-data-hero">
        <div>
          <h1 className="page-title">{copy.title}</h1>
          <p className="page-subtitle">{copy.subtitle}</p>
        </div>

        <div className="app-data-hero-actions">
          <button
            type="button"
            className="app-data-btn"
            disabled={loading || runningAction !== null}
            onClick={() => void loadOverview(true)}
          >
            <RefreshCw size={16} />
            <span>{copy.refresh}</span>
          </button>
          <button
            type="button"
            className="app-data-btn"
            disabled={loading || runningAction !== null}
            onClick={() => void handleAction('create_backup')}
          >
            <Archive size={16} />
            <span>{copy.actionLabel.create_backup}</span>
          </button>
          <button
            type="button"
            className="app-data-btn"
            disabled={loading || runningAction !== null}
            onClick={() => void handleAction('vacuum_database')}
          >
            <Database size={16} />
            <span>{copy.actionLabel.vacuum_database}</span>
          </button>
          <button
            type="button"
            className="app-data-btn app-data-btn-primary"
            disabled={loading || runningAction !== null}
            onClick={() => void handleAction('rebuild_indexes')}
          >
            <Wrench size={16} />
            <span>{copy.actionLabel.rebuild_indexes}</span>
          </button>
          <button
            type="button"
            className="app-data-btn app-data-btn-warn"
            disabled={loading || runningAction !== null}
            onClick={() => void handleAction('clear_caches')}
          >
            <Trash2 size={16} />
            <span>{copy.actionLabel.clear_caches}</span>
          </button>
        </div>
      </section>

      {notice ? (
        <div className={`app-data-notice app-data-notice-${notice.tone}`}>
          {notice.tone === 'success' ? (
            <CheckCircle2 size={16} />
          ) : notice.tone === 'error' ? (
            <CircleAlert size={16} />
          ) : (
            <Info size={16} />
          )}
          <span>{notice.message}</span>
        </div>
      ) : null}

      {loading ? (
        <EmptyState variant="card">{copy.loading}</EmptyState>
      ) : overview ? (
        <>
          {overview.storage.restartRequired ? (
            <div className="app-data-notice">
              <CircleHelp size={16} />
              <span>{copy.restartRequired}</span>
            </div>
          ) : null}

          <div className="app-data-layout">
            <section className="app-data-card app-data-tree-card">
              <header className="app-data-card-header">
                <div>
                  <h2>{copy.sectionFolder}</h2>
                  <p>{copy.treeHint}</p>
                </div>
              </header>

              <div className="app-data-tree-body">
                {overview.items.length > 0 ? (
                  overview.items.map((item) => (
                    <TreeItem
                      key={item.relativePath}
                      item={item}
                      depth={0}
                      language={language}
                      copy={copy}
                      selectedPath={selectedPath}
                      expandedPaths={expandedPaths}
                      onSelect={(nextPath) => setSelectedPath(nextPath)}
                      onToggle={(nextPath) =>
                        setExpandedPaths((current) => toggleExpandedPath(current, nextPath))
                      }
                    />
                  ))
                ) : (
                  <EmptyState compact>{copy.emptySelection}</EmptyState>
                )}
              </div>
            </section>

            <section className="app-data-card app-data-editor-card">
              <header className="app-data-card-header">
                <div>
                  <h2>{copy.sectionPreview}</h2>
                  <p>{detailLoading ? copy.loading : copy.editorHint}</p>
                </div>
                <IconButton
                  className="app-data-icon-btn"
                  disabled={!detail}
                  onClick={() => void handleCopyPath()}
                  label={copy.copyPath}
                >
                  <Copy size={15} />
                </IconButton>
              </header>

              {detail ? (
                <div className="app-data-editor-surface">
                  {detail.preview ? (
                    <div className="app-data-editor-scroll">
                      {previewLines.map((line, index) => (
                        <div
                          key={`${detail.relativePath}-${index}`}
                          className="app-data-editor-line"
                        >
                          <span className="app-data-editor-line-number">{index + 1}</span>
                          <code className="app-data-editor-line-content">{line || ' '}</code>
                        </div>
                      ))}
                    </div>
                  ) : (
                    <EmptyState compact>{copy.emptyPreview}</EmptyState>
                  )}
                </div>
              ) : (
                <EmptyState compact>{copy.emptySelection}</EmptyState>
              )}
            </section>
          </div>
        </>
      ) : (
        <EmptyState variant="card">{copy.emptySelection}</EmptyState>
      )}

      <ConfirmDialog
        open={pendingConfirmAction === 'clear_caches'}
        title={copy.clearCachesConfirmTitle}
        description={copy.clearCachesConfirmDescription}
        confirmLabel={copy.clearCachesConfirmAccept}
        cancelLabel={copy.clearCachesConfirmCancel}
        tone="danger"
        pending={runningAction === 'clear_caches'}
        onCancel={() => {
          if (runningAction === 'clear_caches') return;
          setPendingConfirmAction(null);
        }}
        onConfirm={() => {
          const action = pendingConfirmAction;
          if (!action) return;
          setPendingConfirmAction(null);
          void executeAction(action);
        }}
      />
    </div>
  );
}

function TreeItem({
  item,
  depth,
  language,
  copy,
  selectedPath,
  expandedPaths,
  onSelect,
  onToggle,
}: {
  item: AppDataItem;
  depth: number;
  language: string;
  copy: ReturnType<typeof buildAppDataCopy>;
  selectedPath: string | null;
  expandedPaths: Set<string>;
  onSelect: (path: string) => void;
  onToggle: (path: string) => void;
}) {
  const hasChildren = item.children.length > 0;
  const expanded = expandedPaths.has(item.relativePath);
  const active = item.relativePath === selectedPath;

  return (
    <div>
      <div
        className={
          active ? 'app-data-tree-entry app-data-tree-entry-active' : 'app-data-tree-entry'
        }
        style={{ paddingLeft: 12 + depth * 18 }}
      >
        {hasChildren ? (
          <button
            type="button"
            className="app-data-tree-toggle"
            aria-label={expanded ? copy.collapseFolder : copy.expandFolder}
            onClick={() => onToggle(item.relativePath)}
          >
            <span className="app-data-tree-caret">
              {expanded ? <ChevronDown size={15} /> : <ChevronRight size={15} />}
            </span>
          </button>
        ) : (
          <span className="app-data-tree-caret app-data-tree-caret-static">
            <span className="app-data-tree-dot" />
          </span>
        )}

        <button
          type="button"
          className="app-data-tree-row"
          onClick={() => onSelect(item.relativePath)}
        >
          <ItemIcon item={item} />
          <span className="app-data-tree-name">{item.name}</span>
          <span className="app-data-tree-size">{formatBytes(item.sizeBytes, language)}</span>
        </button>
      </div>

      {hasChildren && expanded
        ? item.children.map((child) => (
            <TreeItem
              key={child.relativePath}
              item={child}
              depth={depth + 1}
              language={language}
              copy={copy}
              selectedPath={selectedPath}
              expandedPaths={expandedPaths}
              onSelect={onSelect}
              onToggle={onToggle}
            />
          ))
        : null}
    </div>
  );
}

function ItemIcon({ item }: { item: Pick<AppDataItem, 'category' | 'itemType'> }) {
  if (item.category === 'database') {
    return <Database size={16} className="app-data-item-icon app-data-item-icon-accent" />;
  }
  if (item.category === 'backup' || item.category === 'export') {
    return <Archive size={16} className="app-data-item-icon app-data-item-icon-muted" />;
  }
  if (item.itemType === 'directory') {
    return <Folder size={16} className="app-data-item-icon app-data-item-icon-accent" />;
  }
  if (item.category === 'cache') {
    return <HardDrive size={16} className="app-data-item-icon app-data-item-icon-muted" />;
  }
  return <FileText size={16} className="app-data-item-icon app-data-item-icon-muted" />;
}

function hasPath(items: AppDataItem[], path: string): boolean {
  return items.some((item) => item.relativePath === path || hasPath(item.children, path));
}

function mergeExpandedPaths(
  current: Set<string>,
  items: AppDataItem[],
  selectedPath: string | null,
): Set<string> {
  const validPaths = collectDirectoryPaths(items);
  const next = new Set<string>([...current].filter((path) => validPaths.has(path)));

  if (selectedPath) {
    const parts = selectedPath.split('/');
    let value = '';
    for (const part of parts.slice(0, -1)) {
      value = value ? `${value}/${part}` : part;
      next.add(value);
    }

    if (validPaths.has(selectedPath)) {
      next.add(selectedPath);
    }
  }

  return next;
}

function collectDirectoryPaths(items: AppDataItem[]): Set<string> {
  const paths = new Set<string>();

  for (const item of items) {
    if (item.itemType === 'directory') {
      paths.add(item.relativePath);
    }

    for (const childPath of collectDirectoryPaths(item.children)) {
      paths.add(childPath);
    }
  }

  return paths;
}

function toggleExpandedPath(current: Set<string>, path: string): Set<string> {
  const next = new Set(current);
  if (next.has(path)) {
    next.delete(path);
  } else {
    next.add(path);
  }
  return next;
}

function splitPreviewLines(preview: string | null | undefined): string[] {
  if (!preview) {
    return [];
  }

  return preview.replace(/\r\n/g, '\n').split('\n');
}

function formatBytes(value: number, locale: string) {
  if (!Number.isFinite(value) || value <= 0) {
    return '0 B';
  }

  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let size = value;
  let index = 0;
  while (size >= 1024 && index < units.length - 1) {
    size /= 1024;
    index += 1;
  }

  const decimals = size >= 100 || index === 0 ? 0 : size >= 10 ? 1 : 2;
  return `${new Intl.NumberFormat(locale, {
    maximumFractionDigits: decimals,
    minimumFractionDigits: 0,
  }).format(size)} ${units[index]}`;
}
