import { useCallback, useMemo, useState, type ReactNode } from 'react';
import { formatMessage, type MessageValues } from './formatMessage';
import { messages, type Locale } from './messages';
import { I18nContext } from './i18nContext';

const STORAGE_KEY = 'totoken.locale';

function detectLocale(): Locale {
  const stored = localStorage.getItem(STORAGE_KEY);
  if (stored === 'zh' || stored === 'en') {
    return stored;
  }
  return navigator.language.toLowerCase().startsWith('zh') ? 'zh' : 'en';
}

export function I18nProvider({ children }: { children: ReactNode }) {
  const [locale, setLocaleState] = useState<Locale>(() => detectLocale());

  const setLocale = useCallback((next: Locale) => {
    setLocaleState(next);
    localStorage.setItem(STORAGE_KEY, next);
  }, []);

  const t = useCallback(
    (key: string, values?: MessageValues): string => {
      const template = messages[locale][key] ?? messages.en[key] ?? key;
      return formatMessage(template, locale, values);
    },
    [locale],
  );

  const value = useMemo(
    () => ({
      locale,
      setLocale,
      t,
    }),
    [locale, setLocale, t],
  );

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}
