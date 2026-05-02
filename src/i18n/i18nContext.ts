import { createContext } from 'react';
import type { MessageValues } from './formatMessage';
import type { Locale } from './messages';

export type I18nContextValue = {
  locale: Locale;
  setLocale: (next: Locale) => void;
  t: (key: string, values?: MessageValues) => string;
};

export const I18nContext = createContext<I18nContextValue | null>(null);
