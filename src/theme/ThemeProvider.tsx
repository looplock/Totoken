import { useLayoutEffect, useMemo, useState, type ReactNode } from 'react';
import { ThemeContext } from './themeContext';
import { composeThemeMode, normalizeThemeMode, splitThemeMode, type ThemeMode } from './themes';

const STORAGE_KEY = 'totoken.theme';

function detectTheme(): ThemeMode {
  const stored = localStorage.getItem(STORAGE_KEY);
  if (stored) {
    return normalizeThemeMode(stored);
  }

  const appearance = window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
  return composeThemeMode('blue', appearance);
}

function applyTheme(theme: ThemeMode) {
  const { family, appearance } = splitThemeMode(theme);
  document.documentElement.dataset.theme = theme;
  document.documentElement.dataset.themeFamily = family;
  document.documentElement.dataset.themeMode = appearance;
  document.documentElement.style.colorScheme = appearance === 'dark' ? 'dark' : 'light';
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setThemeState] = useState<ThemeMode>(() => detectTheme());

  useLayoutEffect(() => {
    const normalizedTheme = normalizeThemeMode(theme);
    applyTheme(normalizedTheme);
    localStorage.setItem(STORAGE_KEY, normalizedTheme);
  }, [theme]);

  const value = useMemo(
    () => ({
      theme,
      setTheme: setThemeState,
    }),
    [theme],
  );

  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}
