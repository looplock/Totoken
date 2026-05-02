export const THEME_FAMILIES = ['blue', 'green', 'amber'] as const;
export const THEME_APPEARANCES = ['light', 'dark'] as const;

export type ThemeFamily = (typeof THEME_FAMILIES)[number];
export type ThemeAppearance = (typeof THEME_APPEARANCES)[number];
export type ThemeMode = `${ThemeFamily}-${ThemeAppearance}`;

export const DEFAULT_THEME: ThemeMode = 'blue-light';

const LEGACY_THEME_MAP: Record<'bright' | 'dark', ThemeMode> = {
  bright: 'blue-light',
  dark: 'blue-dark',
};

export function isThemeFamily(value: string): value is ThemeFamily {
  return (THEME_FAMILIES as readonly string[]).includes(value);
}

export function isThemeAppearance(value: string): value is ThemeAppearance {
  return (THEME_APPEARANCES as readonly string[]).includes(value);
}

export function isThemeMode(value: string): value is ThemeMode {
  const [family, appearance] = value.split('-');
  return isThemeFamily(family) && isThemeAppearance(appearance);
}

export function normalizeThemeMode(value: string | null | undefined): ThemeMode {
  if (value && isThemeMode(value)) {
    return value;
  }

  if (value === 'bright' || value === 'dark') {
    return LEGACY_THEME_MAP[value];
  }

  return DEFAULT_THEME;
}

export function splitThemeMode(theme: ThemeMode): {
  family: ThemeFamily;
  appearance: ThemeAppearance;
} {
  const [family, appearance] = theme.split('-') as [ThemeFamily, ThemeAppearance];
  return { family, appearance };
}

export function composeThemeMode(family: ThemeFamily, appearance: ThemeAppearance): ThemeMode {
  return `${family}-${appearance}` as ThemeMode;
}
