export const sourceApps = [
  'claude_code',
  'codex',
  'cursor',
  'opencode',
  'kilocode',
  'kiro',
] as const;

export type SourceApp = (typeof sourceApps)[number];
export type DisplaySourceApp = SourceApp | 'generic';

export function isSourceApp(value: string): value is SourceApp {
  return sourceApps.includes(value as SourceApp);
}

export function normalizeSourceApp(value: string): SourceApp | null {
  const normalized = value.trim().toLowerCase();
  return isSourceApp(normalized) ? normalized : null;
}

export function normalizeDisplaySourceApp(value: string): DisplaySourceApp {
  return normalizeSourceApp(value) ?? 'generic';
}

export function sourceAppLabelKey(app: DisplaySourceApp): string {
  return app === 'generic' ? 'session.source.generic' : `session.source.${app}`;
}
