import type { Locale } from './messages';

export type MessageValues = Record<string, string | number | boolean | null | undefined>;

const localeTags: Record<Locale, string> = {
  zh: 'zh-CN',
  en: 'en-US',
};

export function formatMessage(template: string, locale: Locale, values?: MessageValues): string {
  if (!template.includes('{')) {
    return template;
  }

  let output = '';
  let cursor = 0;

  while (cursor < template.length) {
    const openIndex = template.indexOf('{', cursor);
    if (openIndex === -1) {
      output += template.slice(cursor);
      break;
    }

    output += template.slice(cursor, openIndex);
    const closeIndex = findMatchingBrace(template, openIndex);
    if (closeIndex === -1) {
      output += template.slice(openIndex);
      break;
    }

    const expression = template.slice(openIndex + 1, closeIndex).trim();
    output += formatExpression(expression, locale, values);
    cursor = closeIndex + 1;
  }

  return output;
}

function formatExpression(expression: string, locale: Locale, values?: MessageValues): string {
  const parts = splitTopLevel(expression, ',');
  if (parts.length === 0) {
    return '';
  }

  const key = parts[0]?.trim();
  if (!key) {
    return '';
  }

  if (parts.length === 1) {
    return formatPrimitive(values?.[key], locale);
  }

  const type = parts[1]?.trim();
  const optionsSource = parts.slice(2).join(',').trim();

  if (type === 'plural') {
    return formatPlural(key, optionsSource, locale, values);
  }

  if (type === 'select') {
    return formatSelect(key, optionsSource, locale, values);
  }

  return formatPrimitive(values?.[key], locale);
}

function formatPlural(
  key: string,
  optionsSource: string,
  locale: Locale,
  values?: MessageValues,
): string {
  const rawValue = values?.[key];
  const numericValue = typeof rawValue === 'number' ? rawValue : Number(rawValue ?? 0);
  const options = parseOptions(optionsSource);
  const exactMatch = options[`=${numericValue}`];

  let template =
    exactMatch ??
    (numericValue === 0 ? options.zero : undefined) ??
    (numericValue === 1 ? options.one : undefined) ??
    options.other;

  if (!template) {
    return '';
  }

  template = template.replace(/#/g, formatNumber(numericValue, locale));
  return formatMessage(template, locale, values);
}

function formatSelect(
  key: string,
  optionsSource: string,
  locale: Locale,
  values?: MessageValues,
): string {
  const rawValue = String(values?.[key] ?? 'other');
  const options = parseOptions(optionsSource);
  const template = options[rawValue] ?? options.other;
  return template ? formatMessage(template, locale, values) : '';
}

function parseOptions(source: string): Record<string, string> {
  const options: Record<string, string> = {};
  let cursor = 0;

  while (cursor < source.length) {
    cursor = skipWhitespace(source, cursor);
    if (cursor >= source.length) {
      break;
    }

    const keyStart = cursor;
    while (cursor < source.length && !/\s|\{/.test(source[cursor] ?? '')) {
      cursor += 1;
    }

    const optionKey = source.slice(keyStart, cursor).trim();
    cursor = skipWhitespace(source, cursor);
    if (!optionKey || source[cursor] !== '{') {
      break;
    }

    const closeIndex = findMatchingBrace(source, cursor);
    if (closeIndex === -1) {
      break;
    }

    options[optionKey] = source.slice(cursor + 1, closeIndex);
    cursor = closeIndex + 1;
  }

  return options;
}

function splitTopLevel(source: string, separator: string): string[] {
  const parts: string[] = [];
  let depth = 0;
  let segmentStart = 0;

  for (let index = 0; index < source.length; index += 1) {
    const character = source[index];
    if (character === '{') {
      depth += 1;
      continue;
    }

    if (character === '}') {
      depth = Math.max(0, depth - 1);
      continue;
    }

    if (character === separator && depth === 0) {
      parts.push(source.slice(segmentStart, index));
      segmentStart = index + 1;
    }
  }

  parts.push(source.slice(segmentStart));
  return parts;
}

function findMatchingBrace(source: string, openIndex: number): number {
  let depth = 0;

  for (let index = openIndex; index < source.length; index += 1) {
    if (source[index] === '{') {
      depth += 1;
    } else if (source[index] === '}') {
      depth -= 1;
      if (depth === 0) {
        return index;
      }
    }
  }

  return -1;
}

function skipWhitespace(source: string, cursor: number): number {
  let nextCursor = cursor;
  while (nextCursor < source.length && /\s/.test(source[nextCursor] ?? '')) {
    nextCursor += 1;
  }

  return nextCursor;
}

function formatPrimitive(
  value: string | number | boolean | null | undefined,
  locale: Locale,
): string {
  if (value === null || value === undefined) {
    return '';
  }

  if (typeof value === 'number') {
    return formatNumber(value, locale);
  }

  if (typeof value === 'boolean') {
    return value ? 'true' : 'false';
  }

  return String(value);
}

function formatNumber(value: number, locale: Locale): string {
  return new Intl.NumberFormat(localeTags[locale]).format(value);
}
