import './KindBadge.css';

export type ProviderKindValue =
  | 'openai_compatible'
  | 'anthropic'
  | 'gemini'
  | 'azure_openai'
  | 'custom';

export function KindBadge({ kind, label }: { kind: string; label?: string }) {
  const tone = badgeTone(kind);
  return (
    <span className={`kind-badge kind-badge-${tone}`} data-kind={kind}>
      {label ?? defaultLabel(kind)}
    </span>
  );
}

function badgeTone(kind: string): string {
  switch (kind) {
    case 'openai_compatible':
      return 'sky';
    case 'anthropic':
      return 'violet';
    case 'gemini':
      return 'amber';
    case 'azure_openai':
      return 'indigo';
    default:
      return 'neutral';
  }
}

function defaultLabel(kind: string): string {
  switch (kind) {
    case 'openai_compatible':
      return 'OpenAI Compatible';
    case 'anthropic':
      return 'Anthropic';
    case 'gemini':
      return 'Gemini';
    case 'azure_openai':
      return 'Azure OpenAI';
    case 'custom':
      return 'Custom';
    default:
      return kind;
  }
}
