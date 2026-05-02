import { useEffect, useMemo, useState } from 'react';
import { UI_PREFERENCES_UPDATED_EVENT, type UiPreferencesUpdatedDetail } from './settingsEvents';
import { isTauriRuntime } from './tauri';
import { fetchSettings } from '../pages/settings/settingsService';

export function useTokenUnitLocale(locale: 'zh' | 'en') {
  const [localizedTokenUnits, setLocalizedTokenUnits] = useState(true);

  useEffect(() => {
    if (!isTauriRuntime()) {
      setLocalizedTokenUnits(true);
      return;
    }

    let cancelled = false;

    async function loadPreference() {
      try {
        const settings = await fetchSettings();
        if (!cancelled) {
          setLocalizedTokenUnits(settings.uiPreferences.localizedTokenUnits);
        }
      } catch {
        if (!cancelled) {
          setLocalizedTokenUnits(true);
        }
      }
    }

    void loadPreference();

    const handlePreferencesUpdated = (event: Event) => {
      const customEvent = event as CustomEvent<UiPreferencesUpdatedDetail>;
      setLocalizedTokenUnits(customEvent.detail.localizedTokenUnits ?? true);
    };

    window.addEventListener(UI_PREFERENCES_UPDATED_EVENT, handlePreferencesUpdated);

    return () => {
      cancelled = true;
      window.removeEventListener(UI_PREFERENCES_UPDATED_EVENT, handlePreferencesUpdated);
    };
  }, []);

  return useMemo(
    () => (localizedTokenUnits && locale === 'zh' ? 'zh-CN' : 'en-US'),
    [locale, localizedTokenUnits],
  );
}
