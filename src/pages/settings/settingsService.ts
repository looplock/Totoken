import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { invokeCommand } from '../../lib/tauri';
import { normalizeThemeMode } from '../../theme/themes';
import type {
  AutoScanStatus,
  SchedulerPreview,
  SettingsState,
  StorageConfig,
} from './settingsData';

function normalizeSettings(settings: SettingsState): SettingsState {
  return {
    ...settings,
    scheduler: {
      ...settings.scheduler,
      scanMode: settings.scheduler.scanMode === 'manual' ? 'manual' : 'auto',
    },
    uiPreferences: {
      ...settings.uiPreferences,
      theme: normalizeThemeMode(settings.uiPreferences.theme),
      localizedTokenUnits: settings.uiPreferences.localizedTokenUnits ?? true,
    },
  };
}

export async function fetchSettings(): Promise<SettingsState> {
  return normalizeSettings(await invokeCommand<SettingsState>('settings_get'));
}

export async function saveSettings(settings: SettingsState): Promise<SettingsState> {
  return normalizeSettings(await invokeCommand<SettingsState>('settings_update', { settings }));
}

export async function resetSettings(): Promise<SettingsState> {
  return normalizeSettings(await invokeCommand<SettingsState>('settings_reset'));
}

export async function fetchAutoScanStatus(): Promise<AutoScanStatus> {
  return invokeCommand<AutoScanStatus>('settings_auto_scan_status');
}

export async function fetchSchedulerPreview(
  scheduler: SettingsState['scheduler'],
): Promise<SchedulerPreview> {
  return invokeCommand<SchedulerPreview>('settings_scheduler_preview', { scheduler });
}

export async function fetchStorageConfig(): Promise<StorageConfig> {
  return invokeCommand<StorageConfig>('get_storage_config');
}

export async function setStorageDataDir(dataDir: string | null): Promise<StorageConfig> {
  return invokeCommand<StorageConfig>('set_storage_data_dir', { dataDir });
}

export async function pickStorageDataDir(title: string): Promise<string | null> {
  const selected = await openDialog({
    directory: true,
    multiple: false,
    title,
  });
  if (selected == null) {
    return null;
  }
  return Array.isArray(selected) ? (selected[0] ?? null) : selected;
}
