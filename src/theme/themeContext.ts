import { createContext } from 'react';
import type { ThemeMode } from './themes';

export type ThemeContextValue = {
  theme: ThemeMode;
  setTheme: (next: ThemeMode) => void;
};

export const ThemeContext = createContext<ThemeContextValue | null>(null);
