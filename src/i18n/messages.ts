import { enMessages } from './locales/en';
import { zhMessages } from './locales/zh';

export type Locale = 'zh' | 'en';
export type Messages = Record<string, string>;

export const messages: Record<Locale, Messages> = {
  zh: zhMessages,
  en: enMessages,
};
