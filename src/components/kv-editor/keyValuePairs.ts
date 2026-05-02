import type { KeyValuePair } from './KeyValueEditor';

export function pairsFromRecord(record: Record<string, string>): KeyValuePair[] {
  return Object.entries(record).map(([key, value], index) => ({
    id: `kv-${index}-${key}`,
    key,
    value,
  }));
}

export function recordFromPairs(pairs: KeyValuePair[]): Record<string, string> {
  const result: Record<string, string> = {};
  for (const pair of pairs) {
    const key = pair.key.trim();
    if (key.length === 0) continue;
    result[key] = pair.value;
  }
  return result;
}
