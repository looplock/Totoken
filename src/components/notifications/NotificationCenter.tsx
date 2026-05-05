import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { AlertCircle, CheckCircle2, LoaderCircle, X } from 'lucide-react';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useI18n } from '../../i18n/useI18n';
import { IconButton } from '../icon-button/IconButton';
import {
  UI_PREFERENCES_UPDATED_EVENT,
  type UiPreferencesUpdatedDetail,
} from '../../lib/settingsEvents';
import { isTauriRuntime } from '../../lib/tauri';
import { fetchSettings } from '../../pages/settings/settingsService';
import './NotificationCenter.css';

type ScanNotificationStatus = 'started' | 'completed' | 'failed';
type ToastPhase = 'enter' | 'active' | 'leaving';

type ScanNotificationEvent = {
  id: string;
  status: ScanNotificationStatus;
  triggerType: string;
  sourceApp: string | null;
  rootPath: string;
  startedAt: string;
  endedAt: string | null;
  filesParsed: number | null;
  sessionsChanged: number | null;
  errorCount: number | null;
  errorMessage: string | null;
};

type ToastNotice = {
  id: string;
  correlationKey: string;
  noticeKey: string;
  status: ScanNotificationStatus;
  title: string;
  description: string;
  phase: ToastPhase;
};

const MAX_NOTICES = 4;
const ENTER_DELAY_MS = 24;
const EXIT_DURATION_MS = 240;

function shortenPath(path: string) {
  const normalizedPath = path.replace(/^\\\\\?\\/, '');

  if (normalizedPath.length <= 48) {
    return normalizedPath;
  }

  return `...${normalizedPath.slice(-45)}`;
}

function sourceLabel(sourceApp: string | null, t: (key: string) => string) {
  switch (sourceApp) {
    case 'opencode':
      return 'OpenCode';
    case 'claude_code':
      return 'Claude Code';
    case 'codex':
      return 'Codex';
    case 'cursor':
      return 'Cursor';
    case 'kilocode':
      return 'Kilo Code';
    case 'kiro':
      return 'Kiro';
    default:
      return t('notifications.source.scanner');
  }
}

function normalizeNoticePath(path: string) {
  return path
    .replace(/^\\\\\?\\/, '')
    .replace(/\\/g, '/')
    .replace(/\/+$/, '')
    .toLowerCase();
}

function buildCorrelationKey(payload: ScanNotificationEvent) {
  return [
    payload.triggerType,
    payload.sourceApp ?? 'unknown',
    normalizeNoticePath(payload.rootPath),
  ].join('::');
}

function buildNoticeKey(payload: ScanNotificationEvent) {
  return `${buildCorrelationKey(payload)}::${payload.status}::${payload.id}`;
}

export function NotificationCenter() {
  const { locale, t } = useI18n();
  const [enabled, setEnabled] = useState(true);
  const [notices, setNotices] = useState<ToastNotice[]>([]);
  const noticesRef = useRef<ToastNotice[]>([]);
  const autoDismissTimersRef = useRef<Map<string, number>>(new Map());
  const phaseTimersRef = useRef<Map<string, number>>(new Map());

  const runtimeUnavailable = useMemo(() => !isTauriRuntime(), []);

  useEffect(() => {
    if (runtimeUnavailable) {
      setEnabled(false);
      return;
    }

    let cancelled = false;

    async function loadNotificationPreference() {
      try {
        const settings = await fetchSettings();
        if (!cancelled) {
          setEnabled(settings.uiPreferences.notifications);
        }
      } catch {
        if (!cancelled) {
          setEnabled(true);
        }
      }
    }

    void loadNotificationPreference();

    const handlePreferencesUpdated = (event: Event) => {
      const customEvent = event as CustomEvent<UiPreferencesUpdatedDetail>;
      setEnabled(customEvent.detail.notifications);
    };

    window.addEventListener(UI_PREFERENCES_UPDATED_EVENT, handlePreferencesUpdated);

    return () => {
      cancelled = true;
      window.removeEventListener(UI_PREFERENCES_UPDATED_EVENT, handlePreferencesUpdated);
    };
  }, [runtimeUnavailable]);

  const clearNoticeTimers = useCallback((noticeKey: string) => {
    const autoDismissTimerId = autoDismissTimersRef.current.get(noticeKey);
    if (autoDismissTimerId) {
      window.clearTimeout(autoDismissTimerId);
      autoDismissTimersRef.current.delete(noticeKey);
    }

    const phaseTimerId = phaseTimersRef.current.get(noticeKey);
    if (phaseTimerId) {
      window.clearTimeout(phaseTimerId);
      phaseTimersRef.current.delete(noticeKey);
    }
  }, []);

  const removeNotice = useCallback(
    (noticeKey: string) => {
      clearNoticeTimers(noticeKey);
      setNotices((current) => current.filter((notice) => notice.noticeKey !== noticeKey));
    },
    [clearNoticeTimers],
  );

  const dismissNotice = useCallback(
    (noticeKey: string) => {
      clearNoticeTimers(noticeKey);

      setNotices((current) =>
        current.map((notice) =>
          notice.noticeKey === noticeKey && notice.phase !== 'leaving'
            ? { ...notice, phase: 'leaving' }
            : notice,
        ),
      );

      const phaseTimerId = window.setTimeout(() => {
        removeNotice(noticeKey);
      }, EXIT_DURATION_MS);
      phaseTimersRef.current.set(noticeKey, phaseTimerId);
    },
    [clearNoticeTimers, removeNotice],
  );

  useEffect(() => {
    noticesRef.current = notices;
  }, [notices]);

  useEffect(() => {
    if (enabled) {
      return;
    }

    autoDismissTimersRef.current.forEach((timerId) => window.clearTimeout(timerId));
    autoDismissTimersRef.current.clear();
    phaseTimersRef.current.forEach((timerId) => window.clearTimeout(timerId));
    phaseTimersRef.current.clear();
    setNotices([]);
  }, [enabled]);

  useEffect(() => {
    if (runtimeUnavailable) {
      return;
    }

    let disposed = false;
    let unlistenFn: UnlistenFn | null = null;
    const autoDismissTimers = autoDismissTimersRef.current;
    const phaseTimers = phaseTimersRef.current;

    const activateNotice = (noticeKey: string) => {
      const phaseTimerId = window.setTimeout(() => {
        setNotices((current) =>
          current.map((notice) =>
            notice.noticeKey === noticeKey && notice.phase === 'enter'
              ? { ...notice, phase: 'active' }
              : notice,
          ),
        );
        phaseTimersRef.current.delete(noticeKey);
      }, ENTER_DELAY_MS);
      phaseTimersRef.current.set(noticeKey, phaseTimerId);
    };

    const scheduleAutoDismiss = (noticeKey: string, status: ScanNotificationStatus) => {
      const existing = autoDismissTimersRef.current.get(noticeKey);
      if (existing) {
        window.clearTimeout(existing);
      }

      const timeoutMs = status === 'started' ? 2400 : 3800;
      const timerId = window.setTimeout(() => {
        dismissNotice(noticeKey);
      }, timeoutMs);
      autoDismissTimersRef.current.set(noticeKey, timerId);
    };

    const dismissMatchingStarted = (correlationKey: string) => {
      const startedKeys = noticesRef.current
        .filter(
          (notice) =>
            notice.correlationKey === correlationKey &&
            notice.status === 'started' &&
            notice.phase !== 'leaving',
        )
        .map((notice) => notice.noticeKey);

      startedKeys.forEach((noticeKey) => dismissNotice(noticeKey));
    };

    const dismissMatchingResults = (
      correlationKey: string,
      keepNoticeKey?: string,
      status?: Exclude<ScanNotificationStatus, 'started'>,
    ) => {
      const resultKeys = noticesRef.current
        .filter(
          (notice) =>
            notice.correlationKey === correlationKey &&
            notice.status !== 'started' &&
            (status === undefined || notice.status === status) &&
            (keepNoticeKey === undefined || notice.noticeKey !== keepNoticeKey) &&
            notice.phase !== 'leaving',
        )
        .map((notice) => notice.noticeKey);

      resultKeys.forEach((noticeKey) => dismissNotice(noticeKey));
    };

    const pushNotice = (notice: Omit<ToastNotice, 'phase'>) => {
      let shouldActivate = false;

      setNotices((current) => {
        const nextNotice: ToastNotice = { ...notice, phase: 'enter' };
        const next = [nextNotice, ...current].slice(0, MAX_NOTICES);

        const retainedKeys = new Set(next.map((item) => item.noticeKey));
        current.forEach((item) => {
          if (!retainedKeys.has(item.noticeKey)) {
            clearNoticeTimers(item.noticeKey);
          }
        });

        shouldActivate = true;
        return next;
      });

      if (shouldActivate) {
        activateNotice(notice.noticeKey);
      }
      scheduleAutoDismiss(notice.noticeKey, notice.status);
    };

    const toNotice = (payload: ScanNotificationEvent): Omit<ToastNotice, 'phase'> => {
      const appLabel = sourceLabel(payload.sourceApp, t);
      const pathLabel = shortenPath(payload.rootPath);
      const correlationKey = buildCorrelationKey(payload);
      const noticeKey = buildNoticeKey(payload);

      if (payload.status === 'started') {
        return {
          id: payload.id,
          correlationKey,
          noticeKey,
          status: payload.status,
          title: t('notifications.scan.started.title'),
          description: t('notifications.scan.started.description', {
            app: appLabel,
            path: pathLabel,
          }),
        };
      }

      if (payload.status === 'failed') {
        return {
          id: payload.id,
          correlationKey,
          noticeKey,
          status: payload.status,
          title: t('notifications.scan.failed.title'),
          description:
            payload.errorMessage ?? t('notifications.scan.failed.description', { app: appLabel }),
        };
      }

      const filesParsed = payload.filesParsed ?? 0;
      const sessionsChanged = payload.sessionsChanged ?? 0;

      return {
        id: payload.id,
        correlationKey,
        noticeKey,
        status: payload.status,
        title: t('notifications.scan.completed.title'),
        description: t('notifications.scan.completed.description', {
          app: appLabel,
          filesParsed,
          sessionsChanged,
        }),
      };
    };

    void listen<ScanNotificationEvent>('scan-notification', (event) => {
      if (disposed || !enabled) {
        return;
      }

      const notice = toNotice(event.payload);
      pushNotice(notice);

      if (notice.status === 'started') {
        dismissMatchingResults(notice.correlationKey);
        return;
      }

      dismissMatchingResults(notice.correlationKey, notice.noticeKey, notice.status);
      if (notice.status === 'completed' || notice.status === 'failed') {
        dismissMatchingStarted(notice.correlationKey);
      }
    }).then((unlisten) => {
      if (disposed) {
        unlisten();
        return;
      }
      unlistenFn = unlisten;
    });

    return () => {
      disposed = true;
      if (unlistenFn) {
        unlistenFn();
      }
      autoDismissTimers.forEach((timerId) => window.clearTimeout(timerId));
      autoDismissTimers.clear();
      phaseTimers.forEach((timerId) => window.clearTimeout(timerId));
      phaseTimers.clear();
    };
  }, [clearNoticeTimers, dismissNotice, enabled, locale, runtimeUnavailable, t]);

  if (runtimeUnavailable || !enabled || notices.length === 0) {
    return null;
  }

  return (
    <div className="notification-center" aria-live="polite" aria-atomic="true">
      {notices.map((notice) => (
        <div
          key={notice.noticeKey}
          className={`notification-toast notification-toast-${notice.status} notification-toast-${notice.phase}`}
        >
          <div className="notification-toast-icon">
            {notice.status === 'started' ? (
              <LoaderCircle size={18} className="notification-icon-spin" />
            ) : notice.status === 'completed' ? (
              <CheckCircle2 size={18} />
            ) : (
              <AlertCircle size={18} />
            )}
          </div>

          <div className="notification-toast-body">
            <strong>{notice.title}</strong>
            <span>{notice.description}</span>
          </div>

          <IconButton
            className="notification-toast-close"
            label={t('notifications.dismiss')}
            showTooltip={false}
            onClick={() => dismissNotice(notice.noticeKey)}
          >
            <X size={15} />
          </IconButton>
        </div>
      ))}
    </div>
  );
}
