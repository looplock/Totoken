import { useCallback, useEffect, useMemo, useState } from 'react';
import { ChevronDown, FolderOpen, FolderCog, Info, RotateCcw, Search } from 'lucide-react';
import { useI18n } from '../../i18n/useI18n';
import { EmptyState } from '../../components/empty-state/EmptyState';
import { Switch } from '../../components/switch/Switch';
import { InfoTooltip } from '../../components/tooltip/InfoTooltip';
import { emitUiPreferencesUpdated } from '../../lib/settingsEvents';
import { useTheme } from '../../theme/useTheme';
import { composeThemeMode, splitThemeMode, type ThemeFamily } from '../../theme/themes';
import {
  createDefaultSettings,
  type CloseAction,
  type ScanMode,
  type SchedulerPreview,
  type SettingsState,
  type StorageConfig,
} from './settingsData';
import {
  fetchSchedulerPreview,
  fetchSettings,
  pickStorageDataDir,
  fetchStorageConfig,
  resetSettings,
  saveSettings,
  setStorageDataDir,
} from './settingsService';
import './SettingsPage.css';

type SettingsSectionKey = 'scheduler' | 'preferences' | 'storage';

const THEME_GROUPS: Array<{ family: ThemeFamily; labelKey: string }> = [
  { family: 'blue', labelKey: 'settings.preferences.themeBlue' },
  { family: 'green', labelKey: 'settings.preferences.themeGreen' },
  { family: 'amber', labelKey: 'settings.preferences.themeAmber' },
];

function buildLinePath(values: number[], width: number, height: number, maxValue: number) {
  return values
    .map((value, index) => {
      const x = (index / (values.length - 1)) * width;
      const y = height - (value / maxValue) * height;
      return `${index === 0 ? 'M' : 'L'} ${x.toFixed(2)} ${y.toFixed(2)}`;
    })
    .join(' ');
}

function buildAreaPath(values: number[], width: number, height: number, maxValue: number) {
  const linePath = buildLinePath(values, width, height, maxValue);
  return `${linePath} L ${width} ${height} L 0 ${height} Z`;
}

function normalizeChartSeries(values: number[]) {
  if (values.length === 0) {
    return Array.from({ length: 12 }, () => 0);
  }

  if (values.length === 1) {
    return [values[0], values[0]];
  }

  return values;
}

function matchesSearch(search: string, ...values: string[]) {
  if (!search) {
    return true;
  }

  const haystack = values.join(' ').toLowerCase();
  return haystack.includes(search);
}

function settingsEqual(left: SettingsState, right: SettingsState) {
  return (
    left.scheduler.scanMode === right.scheduler.scanMode &&
    left.scheduler.baseInterval === right.scheduler.baseInterval &&
    left.scheduler.minInterval === right.scheduler.minInterval &&
    left.scheduler.maxInterval === right.scheduler.maxInterval &&
    left.scheduler.adaptiveScanning === right.scheduler.adaptiveScanning &&
    left.scheduler.ewmaAlpha === right.scheduler.ewmaAlpha &&
    left.scheduler.changeRateThreshold === right.scheduler.changeRateThreshold &&
    left.uiPreferences.theme === right.uiPreferences.theme &&
    left.uiPreferences.language === right.uiPreferences.language &&
    left.uiPreferences.notifications === right.uiPreferences.notifications &&
    left.uiPreferences.localizedTokenUnits === right.uiPreferences.localizedTokenUnits &&
    left.uiPreferences.closeAction === right.uiPreferences.closeAction
  );
}

export function SettingsPage() {
  const { locale, setLocale, t } = useI18n();
  const { theme, setTheme } = useTheme();
  const [settings, setSettings] = useState<SettingsState>(() => {
    const defaults = createDefaultSettings();
    defaults.uiPreferences.theme = theme;
    defaults.uiPreferences.language = locale === 'zh' ? 'zh-CN' : 'en-US';
    return defaults;
  });
  const [savedSettings, setSavedSettings] = useState<SettingsState>(() => {
    const defaults = createDefaultSettings();
    defaults.uiPreferences.theme = theme;
    defaults.uiPreferences.language = locale === 'zh' ? 'zh-CN' : 'en-US';
    return defaults;
  });
  const [search, setSearch] = useState('');
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);
  const [hasLoadedSettings, setHasLoadedSettings] = useState(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [schedulerPreview, setSchedulerPreview] = useState<SchedulerPreview | null>(null);
  const [storageConfig, setStorageConfig] = useState<StorageConfig | null>(null);
  const [isUpdatingStorage, setIsUpdatingStorage] = useState(false);

  const chartWidth = 520;
  const chartHeight = 62;
  const chartSeries = normalizeChartSeries(schedulerPreview?.series ?? []);
  const chartThreshold = schedulerPreview?.threshold ?? settings.scheduler.changeRateThreshold;
  const chartMax = Math.max(1, Math.ceil(Math.max(chartThreshold * 1.35, ...chartSeries)));
  const chartPath = buildLinePath(chartSeries, chartWidth, chartHeight, chartMax);
  const chartAreaPath = buildAreaPath(chartSeries, chartWidth, chartHeight, chartMax);
  const thresholdY = chartHeight - (chartThreshold / chartMax) * chartHeight;
  const chartMetaLabel = schedulerPreview
    ? schedulerPreview.basedOnLiveTelemetry
      ? t('settings.scheduler.chartLive')
      : t('settings.scheduler.chartFallback')
    : t('settings.feedback.loading');
  const chartAxisLabels =
    locale === 'zh'
      ? ['-10 分钟', '-8 分钟', '-6 分钟', '-4 分钟', '-2 分钟', '现在']
      : ['-10m', '-8m', '-6m', '-4m', '-2m', 'Now'];
  const settingInfoLabel = t('settings.info.label');

  const dirty = !settingsEqual(settings, savedSettings);
  const normalizedSearch = search.trim().toLowerCase();
  const canEditSettings = hasLoadedSettings && !isLoading;
  const { family: currentThemeFamily, appearance: currentThemeAppearance } = splitThemeMode(
    settings.uiPreferences.theme,
  );

  const visibleSections = useMemo(() => {
    const result = new Set<SettingsSectionKey>();

    if (!normalizedSearch) {
      result.add('scheduler');
      result.add('preferences');
      result.add('storage');
      return result;
    }

    if (
      matchesSearch(
        normalizedSearch,
        t('settings.section.scheduler'),
        t('settings.scheduler.mode'),
        t('settings.scheduler.autoMode'),
        t('settings.scheduler.manualMode'),
        t('settings.scheduler.baseInterval'),
        t('settings.scheduler.adaptive'),
        'ewma threshold interval polling auto manual rescan',
      )
    ) {
      result.add('scheduler');
    }

    if (
      matchesSearch(
        normalizedSearch,
        t('settings.section.storage'),
        t('settings.storage.currentPath'),
        t('settings.storage.browse'),
        t('settings.storage.useDefault'),
        'storage data directory path migrate restart cleanup',
      )
    ) {
      result.add('storage');
    }

    if (
      matchesSearch(
        normalizedSearch,
        t('settings.section.preferences'),
        t('settings.preferences.theme'),
        t('settings.preferences.themeBlue'),
        t('settings.preferences.themeGreen'),
        t('settings.preferences.themeAmber'),
        t('settings.preferences.themeLight'),
        t('settings.preferences.themeDark'),
        t('settings.preferences.language'),
        t('settings.preferences.closeAction'),
        t('settings.preferences.closeActionQuit'),
        t('settings.preferences.closeActionTray'),
        t('settings.preferences.tokenUnits'),
        'ui alerts language theme close quit tray token units blue green amber light dark',
      )
    ) {
      result.add('preferences');
    }

    return result;
  }, [normalizedSearch, t]);

  const hasVisibleSections = visibleSections.size > 0;

  const updateSettings = (updater: (current: SettingsState) => SettingsState) => {
    setErrorMessage(null);
    setSettings((current) => updater(current));
  };

  const applyUiPreferences = useCallback(
    (uiPreferences: SettingsState['uiPreferences']) => {
      setTheme(uiPreferences.theme);
      setLocale(uiPreferences.language === 'zh-CN' ? 'zh' : 'en');
    },
    [setLocale, setTheme],
  );

  const syncUiPreferenceEffects = useCallback(
    (uiPreferences: SettingsState['uiPreferences']) => {
      applyUiPreferences(uiPreferences);
      emitUiPreferencesUpdated({
        notifications: uiPreferences.notifications,
        localizedTokenUnits: uiPreferences.localizedTokenUnits,
      });
    },
    [applyUiPreferences],
  );

  useEffect(() => {
    let cancelled = false;

    async function loadSettings() {
      setIsLoading(true);
      setHasLoadedSettings(false);
      setErrorMessage(null);

      try {
        const loaded = await fetchSettings();
        if (cancelled) {
          return;
        }

        setSettings(loaded);
        setSavedSettings(loaded);
        setHasLoadedSettings(true);
        syncUiPreferenceEffects(loaded.uiPreferences);

        const storage = await fetchStorageConfig();
        if (cancelled) {
          return;
        }
        setStorageConfig(storage);
      } catch (error) {
        if (cancelled) {
          return;
        }

        const message = error instanceof Error ? error.message : t('settings.feedback.loadFailed');
        setHasLoadedSettings(false);
        setErrorMessage(message);
      } finally {
        if (!cancelled) {
          setIsLoading(false);
        }
      }
    }

    void loadSettings();

    return () => {
      cancelled = true;
    };
  }, [syncUiPreferenceEffects, t]);

  useEffect(() => {
    let cancelled = false;
    const timeoutId = window.setTimeout(async () => {
      try {
        const preview = await fetchSchedulerPreview(settings.scheduler);
        if (!cancelled) {
          setSchedulerPreview(preview);
        }
      } catch {
        if (!cancelled) {
          setSchedulerPreview(null);
        }
      }
    }, 180);

    return () => {
      cancelled = true;
      window.clearTimeout(timeoutId);
    };
  }, [settings.scheduler]);

  useEffect(() => {
    if (!hasLoadedSettings || isLoading || !dirty) {
      return;
    }

    let cancelled = false;
    const timeoutId = window.setTimeout(async () => {
      setIsSaving(true);
      setErrorMessage(null);

      try {
        const saved = await saveSettings(settings);
        if (cancelled) {
          return;
        }

        setSavedSettings(saved);
        setSettings((current) => (settingsEqual(current, settings) ? saved : current));
        syncUiPreferenceEffects(saved.uiPreferences);
      } catch (error) {
        if (!cancelled) {
          const message =
            error instanceof Error ? error.message : t('settings.feedback.saveFailed');
          setErrorMessage(message);
        }
      } finally {
        if (!cancelled) {
          setIsSaving(false);
        }
      }
    }, 350);

    return () => {
      cancelled = true;
      window.clearTimeout(timeoutId);
    };
  }, [dirty, hasLoadedSettings, isLoading, settings, syncUiPreferenceEffects, t]);

  const restoreDefaults = async () => {
    if (!hasLoadedSettings) {
      setErrorMessage(t('settings.feedback.loadFailed'));
      return;
    }

    setIsSaving(true);
    setErrorMessage(null);

    try {
      const defaults = await resetSettings();
      setSettings(defaults);
      setSavedSettings(defaults);
      syncUiPreferenceEffects(defaults.uiPreferences);
    } catch (error) {
      const message = error instanceof Error ? error.message : t('settings.feedback.resetFailed');
      setErrorMessage(message);
    } finally {
      setIsSaving(false);
    }
  };

  const updateStoragePath = async (nextPath: string | null) => {
    setIsUpdatingStorage(true);
    setErrorMessage(null);

    try {
      const nextStorage = await setStorageDataDir(nextPath);
      setStorageConfig(nextStorage);
    } catch (error) {
      const message = error instanceof Error ? error.message : t('settings.feedback.saveFailed');
      setErrorMessage(message);
    } finally {
      setIsUpdatingStorage(false);
    }
  };

  const browseStoragePath = async () => {
    setErrorMessage(null);
    try {
      const selectedPath = await pickStorageDataDir(t('settings.storage.pickerTitle'));
      if (selectedPath) {
        if (selectedPath !== storageConfig?.dataDir) {
          await updateStoragePath(selectedPath);
        }
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : t('settings.feedback.saveFailed');
      setErrorMessage(message);
    }
  };

  return (
    <div className="settings-page">
      <section className="settings-hero">
        <div>
          <h1 className="page-title">{t('settings.title')}</h1>
          <p className="page-subtitle">{t('settings.subtitle')}</p>
        </div>

        <div className="settings-hero-actions">
          <label className="settings-search" aria-label={t('settings.search.placeholder')}>
            <Search size={18} />
            <input
              type="search"
              value={search}
              placeholder={t('settings.search.placeholder')}
              onChange={(event) => setSearch(event.target.value)}
            />
          </label>

          <button
            type="button"
            className="settings-btn"
            onClick={() => void restoreDefaults()}
            disabled={!canEditSettings || isSaving}
          >
            <RotateCcw size={16} />
            <span>{t('settings.actions.restore')}</span>
          </button>
        </div>
      </section>

      {errorMessage ? (
        <div className="settings-notice">
          <Info size={17} />
          <span>{errorMessage}</span>
        </div>
      ) : null}

      {isLoading ? (
        <EmptyState variant="card">{t('settings.feedback.loading')}</EmptyState>
      ) : !hasLoadedSettings ? (
        <EmptyState variant="card">{t('settings.feedback.loadFailed')}</EmptyState>
      ) : hasVisibleSections ? (
        <div className="settings-layout">
          <div className="settings-column">
            {visibleSections.has('scheduler') ? (
              <section className="settings-card settings-scheduler-section">
                <header className="settings-card-header">
                  <div className="settings-card-title-wrap">
                    <h2 className="settings-card-title">{t('settings.section.scheduler')}</h2>
                    <InfoTooltip label={settingInfoLabel} content={t('settings.info.scheduler')} />
                  </div>
                </header>

                <div className="settings-card-content">
                  <div className="settings-field settings-control-card settings-control-card-field">
                    <span className="settings-field-label">{t('settings.scheduler.mode')}</span>
                    <div className="settings-segmented settings-segmented-wide">
                      <button
                        type="button"
                        className={settings.scheduler.scanMode === 'auto' ? 'is-active' : undefined}
                        onClick={() =>
                          updateSettings((current) => ({
                            ...current,
                            scheduler: {
                              ...current.scheduler,
                              scanMode: 'auto' as ScanMode,
                            },
                          }))
                        }
                      >
                        {t('settings.scheduler.autoMode')}
                      </button>
                      <button
                        type="button"
                        className={
                          settings.scheduler.scanMode === 'manual' ? 'is-active' : undefined
                        }
                        onClick={() =>
                          updateSettings((current) => ({
                            ...current,
                            scheduler: {
                              ...current.scheduler,
                              scanMode: 'manual' as ScanMode,
                            },
                          }))
                        }
                      >
                        {t('settings.scheduler.manualMode')}
                      </button>
                    </div>
                    <span className="settings-field-hint">
                      {settings.scheduler.scanMode === 'auto'
                        ? t('settings.scheduler.autoModeHint')
                        : t('settings.scheduler.manualModeHint')}
                    </span>
                  </div>

                  <div className="settings-divider" />

                  <div className="settings-fields-grid">
                    <label className="settings-field">
                      <span className="settings-field-label">
                        {t('settings.scheduler.baseInterval')}
                      </span>
                      <input
                        className="settings-input"
                        type="number"
                        min={5}
                        value={settings.scheduler.baseInterval}
                        onChange={(event) =>
                          updateSettings((current) => ({
                            ...current,
                            scheduler: {
                              ...current.scheduler,
                              baseInterval: Number(event.target.value),
                            },
                          }))
                        }
                      />
                    </label>

                    <label className="settings-field">
                      <span className="settings-field-label">
                        {t('settings.scheduler.minInterval')}
                      </span>
                      <input
                        className="settings-input"
                        type="number"
                        min={1}
                        value={settings.scheduler.minInterval}
                        onChange={(event) =>
                          updateSettings((current) => ({
                            ...current,
                            scheduler: {
                              ...current.scheduler,
                              minInterval: Number(event.target.value),
                            },
                          }))
                        }
                      />
                    </label>

                    <label className="settings-field">
                      <span className="settings-field-label">
                        {t('settings.scheduler.maxInterval')}
                      </span>
                      <input
                        className="settings-input"
                        type="number"
                        min={10}
                        value={settings.scheduler.maxInterval}
                        onChange={(event) =>
                          updateSettings((current) => ({
                            ...current,
                            scheduler: {
                              ...current.scheduler,
                              maxInterval: Number(event.target.value),
                            },
                          }))
                        }
                      />
                    </label>
                  </div>

                  <div className="settings-divider" />

                  <div className="settings-toggle-row">
                    <div className="settings-toggle-content">
                      <p className="settings-toggle-title">{t('settings.scheduler.adaptive')}</p>
                      <p className="settings-toggle-hint">{t('settings.scheduler.adaptiveHint')}</p>
                    </div>
                    <Switch
                      size="sm"
                      checked={settings.scheduler.adaptiveScanning}
                      onToggle={() =>
                        updateSettings((current) => ({
                          ...current,
                          scheduler: {
                            ...current.scheduler,
                            adaptiveScanning: !current.scheduler.adaptiveScanning,
                          },
                        }))
                      }
                      label={t('settings.scheduler.adaptive')}
                    />
                  </div>

                  <div className="settings-divider" />

                  <div className="settings-fields-grid-two">
                    <label className="settings-field">
                      <div className="settings-field-header">
                        <span className="settings-field-label">{t('settings.scheduler.ewma')}</span>
                        <InfoTooltip label={settingInfoLabel} content={t('settings.info.ewma')} />
                      </div>
                      <input
                        className="settings-input"
                        type="number"
                        min={0.05}
                        max={1}
                        step={0.01}
                        value={settings.scheduler.ewmaAlpha}
                        onChange={(event) =>
                          updateSettings((current) => ({
                            ...current,
                            scheduler: {
                              ...current.scheduler,
                              ewmaAlpha: Number(event.target.value),
                            },
                          }))
                        }
                      />
                    </label>

                    <label className="settings-field">
                      <div className="settings-field-header">
                        <span className="settings-field-label">
                          {t('settings.scheduler.threshold')}
                        </span>
                        <InfoTooltip
                          label={settingInfoLabel}
                          content={t('settings.info.threshold')}
                        />
                      </div>
                      <input
                        className="settings-input"
                        type="number"
                        min={1}
                        max={100}
                        value={settings.scheduler.changeRateThreshold}
                        onChange={(event) =>
                          updateSettings((current) => ({
                            ...current,
                            scheduler: {
                              ...current.scheduler,
                              changeRateThreshold: Number(event.target.value),
                            },
                          }))
                        }
                      />
                    </label>
                  </div>

                  <div className="settings-chart">
                    <div className="settings-chart-legend">
                      <span className="settings-chart-line-key">
                        {t('settings.scheduler.chartRate')}
                      </span>
                      <span className="settings-chart-dash-key">
                        {t('settings.scheduler.chartThreshold')}
                      </span>
                    </div>

                    <div className="settings-chart-frame">
                      <div className="settings-chart-axis-y">
                        <span>{`${chartMax}%`}</span>
                        <span>{`${(chartMax / 2).toFixed(1)}%`}</span>
                        <span>0%</span>
                      </div>

                      <div>
                        <div className="settings-chart-meta">
                          <span>{chartMetaLabel}</span>
                        </div>
                        <svg
                          className="settings-chart-svg"
                          viewBox={`0 0 ${chartWidth} ${chartHeight}`}
                          aria-hidden="true"
                        >
                          <path className="settings-chart-area" d={chartAreaPath} />
                          <line
                            className="settings-chart-grid"
                            x1="0"
                            y1="0.5"
                            x2={chartWidth}
                            y2="0.5"
                          />
                          <line
                            className="settings-chart-grid"
                            x1="0"
                            y1={chartHeight / 2}
                            x2={chartWidth}
                            y2={chartHeight / 2}
                          />
                          <line
                            className="settings-chart-grid"
                            x1="0"
                            y1={chartHeight - 0.5}
                            x2={chartWidth}
                            y2={chartHeight - 0.5}
                          />
                          <line
                            className="settings-chart-threshold"
                            x1="0"
                            y1={thresholdY}
                            x2={chartWidth}
                            y2={thresholdY}
                          />
                          <path className="settings-chart-path" d={chartPath} />
                        </svg>

                        <div className="settings-chart-axis-x">
                          {chartAxisLabels.map((label) => (
                            <span key={label}>{label}</span>
                          ))}
                        </div>
                      </div>
                    </div>
                  </div>
                </div>
              </section>
            ) : null}
          </div>

          <div className="settings-column">
            {visibleSections.has('storage') ? (
              <section className="settings-card settings-storage-section">
                <header className="settings-card-header">
                  <div className="settings-card-title-wrap">
                    <h2 className="settings-card-title">{t('settings.section.storage')}</h2>
                    <InfoTooltip label={settingInfoLabel} content={t('settings.info.storage')} />
                  </div>
                </header>

                <div className="settings-card-content">
                  <div className="settings-storage-card">
                    <div className="settings-storage-head">
                      <span className="settings-storage-icon">
                        <FolderCog size={18} />
                      </span>
                      <div className="settings-storage-copy">
                        <strong>{t('settings.storage.currentPath')}</strong>
                        <span>{storageConfig?.dataDir ?? t('settings.feedback.loading')}</span>
                      </div>
                    </div>

                    <div className="settings-storage-actions">
                      <button
                        type="button"
                        className="settings-btn"
                        disabled={isUpdatingStorage}
                        onClick={() => void browseStoragePath()}
                      >
                        <FolderOpen size={16} />
                        <span>{t('settings.storage.browse')}</span>
                      </button>
                      <button
                        type="button"
                        className="settings-btn"
                        disabled={isUpdatingStorage}
                        onClick={() => void updateStoragePath(null)}
                      >
                        <RotateCcw size={16} />
                        <span>{t('settings.storage.useDefault')}</span>
                      </button>
                    </div>

                    {storageConfig?.restartRequired ? (
                      <div className="settings-notice">
                        <Info size={17} />
                        <span>{t('settings.storage.restartNotice')}</span>
                      </div>
                    ) : null}

                    {storageConfig?.pendingCleanupPath ? (
                      <div className="settings-notice">
                        <Info size={17} />
                        <span>
                          {t('settings.storage.pendingCleanup')} {storageConfig.pendingCleanupPath}
                        </span>
                      </div>
                    ) : null}
                  </div>
                </div>
              </section>
            ) : null}

            {visibleSections.has('preferences') ? (
              <section className="settings-card settings-preferences-section">
                <header className="settings-card-header">
                  <div className="settings-card-title-wrap">
                    <h2 className="settings-card-title">{t('settings.section.preferences')}</h2>
                    <InfoTooltip
                      label={settingInfoLabel}
                      content={t('settings.info.preferences')}
                    />
                  </div>
                </header>

                <div className="settings-card-content">
                  <div className="settings-preferences-grid">
                    <div className="settings-preference-item settings-theme-panel">
                      <div className="settings-theme-controls">
                        <label className="settings-field settings-control-card settings-control-card-field settings-theme-select">
                          <span className="settings-field-label">
                            {t('settings.preferences.theme')}
                          </span>
                          <div className="settings-select-wrap">
                            <select
                              className="settings-select"
                              value={currentThemeFamily}
                              onChange={(event) => {
                                const nextTheme = composeThemeMode(
                                  event.target.value as ThemeFamily,
                                  currentThemeAppearance,
                                );
                                applyUiPreferences({
                                  ...settings.uiPreferences,
                                  theme: nextTheme,
                                });
                                updateSettings((current) => ({
                                  ...current,
                                  uiPreferences: {
                                    ...current.uiPreferences,
                                    theme: nextTheme,
                                  },
                                }));
                              }}
                            >
                              {THEME_GROUPS.map(({ family, labelKey }) => (
                                <option key={family} value={family}>
                                  {t(labelKey)}
                                </option>
                              ))}
                            </select>
                            <ChevronDown size={16} />
                          </div>
                        </label>

                        <div className="settings-theme-mode settings-control-card settings-control-card-toggle">
                          <div className="settings-toggle-content settings-theme-mode-copy">
                            <p className="settings-toggle-title">
                              {t('settings.preferences.themeDark')}
                            </p>
                            <p className="settings-toggle-hint">
                              {currentThemeAppearance === 'dark'
                                ? t('settings.preferences.themeDark')
                                : t('settings.preferences.themeLight')}
                            </p>
                          </div>
                          <Switch
                            size="sm"
                            checked={currentThemeAppearance === 'dark'}
                            onToggle={() => {
                              const nextTheme = composeThemeMode(
                                currentThemeFamily,
                                currentThemeAppearance === 'dark' ? 'light' : 'dark',
                              );
                              applyUiPreferences({
                                ...settings.uiPreferences,
                                theme: nextTheme,
                              });
                              updateSettings((current) => ({
                                ...current,
                                uiPreferences: {
                                  ...current.uiPreferences,
                                  theme: nextTheme,
                                },
                              }));
                            }}
                            label={t('settings.preferences.themeDark')}
                          />
                        </div>
                      </div>
                    </div>

                    <label className="settings-field settings-preference-item settings-control-card settings-control-card-field">
                      <span className="settings-field-label">
                        {t('settings.preferences.language')}
                      </span>
                      <div className="settings-select-wrap">
                        <select
                          className="settings-select"
                          value={settings.uiPreferences.language}
                          onChange={(event) => {
                            const nextLanguage = event.target.value as 'zh-CN' | 'en-US';
                            applyUiPreferences({
                              ...settings.uiPreferences,
                              language: nextLanguage,
                            });
                            updateSettings((current) => ({
                              ...current,
                              uiPreferences: {
                                ...current.uiPreferences,
                                language: nextLanguage,
                              },
                            }));
                          }}
                        >
                          <option value="en-US">English (US)</option>
                          <option value="zh-CN">简体中文</option>
                        </select>
                        <ChevronDown size={16} />
                      </div>
                    </label>

                    <label className="settings-field settings-preference-item settings-control-card settings-control-card-field">
                      <span className="settings-field-label">
                        {t('settings.preferences.closeAction')}
                      </span>
                      <div className="settings-select-wrap">
                        <select
                          className="settings-select"
                          value={settings.uiPreferences.closeAction}
                          onChange={(event) => {
                            const nextCloseAction = event.target.value as CloseAction;
                            updateSettings((current) => ({
                              ...current,
                              uiPreferences: {
                                ...current.uiPreferences,
                                closeAction: nextCloseAction,
                              },
                            }));
                          }}
                        >
                          <option value="quit">{t('settings.preferences.closeActionQuit')}</option>
                          <option value="tray">{t('settings.preferences.closeActionTray')}</option>
                        </select>
                        <ChevronDown size={16} />
                      </div>
                    </label>

                    <div className="settings-preference-toggle settings-preference-item settings-control-card settings-control-card-toggle">
                      <div className="settings-toggle-content">
                        <p className="settings-toggle-title">
                          {t('settings.preferences.notifications')}
                        </p>
                        <p className="settings-toggle-hint">
                          {t('settings.preferences.notificationsHint')}
                        </p>
                      </div>
                      <Switch
                        size="sm"
                        checked={settings.uiPreferences.notifications}
                        onToggle={() =>
                          updateSettings((current) => ({
                            ...current,
                            uiPreferences: {
                              ...current.uiPreferences,
                              notifications: !current.uiPreferences.notifications,
                            },
                          }))
                        }
                        label={t('settings.preferences.notifications')}
                      />
                    </div>

                    <div className="settings-preference-toggle settings-preference-item settings-control-card settings-control-card-toggle">
                      <div className="settings-toggle-content">
                        <p className="settings-toggle-title">
                          {t('settings.preferences.tokenUnits')}
                        </p>
                        <p className="settings-toggle-hint">
                          {t('settings.preferences.tokenUnitsHint')}
                        </p>
                      </div>
                      <Switch
                        size="sm"
                        checked={settings.uiPreferences.localizedTokenUnits}
                        onToggle={() =>
                          updateSettings((current) => ({
                            ...current,
                            uiPreferences: {
                              ...current.uiPreferences,
                              localizedTokenUnits: !current.uiPreferences.localizedTokenUnits,
                            },
                          }))
                        }
                        label={t('settings.preferences.tokenUnits')}
                      />
                    </div>
                  </div>
                </div>
              </section>
            ) : null}
          </div>
        </div>
      ) : (
        <EmptyState variant="card">{t('settings.search.empty')}</EmptyState>
      )}
    </div>
  );
}
