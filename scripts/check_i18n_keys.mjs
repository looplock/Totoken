import { readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const srcRoot = join(repoRoot, 'src');

function loadKeys(relativePath) {
  const content = readFileSync(join(repoRoot, relativePath), 'utf8');
  return [...content.matchAll(/^\s*['"]([^'"]+)['"]\s*:/gm)].map((match) => match[1]).sort();
}

function listSourceFiles(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const fullPath = join(dir, entry.name);
    if (entry.isDirectory()) {
      return listSourceFiles(fullPath);
    }
    return entry.isFile() && /\.(ts|tsx)$/.test(entry.name) ? [fullPath] : [];
  });
}

function loadUsedKeys() {
  const keys = new Set();
  for (const filePath of listSourceFiles(srcRoot)) {
    const content = readFileSync(filePath, 'utf8');

    for (const match of content.matchAll(/\bt\(\s*['"]([^'"]+)['"]/g)) {
      keys.add(match[1]);
    }

    for (const match of content.matchAll(/labelKey:\s*['"]([^'"]+)['"]/g)) {
      keys.add(match[1]);
    }
  }

  return [...keys].sort();
}

function diff(left, right) {
  const rightSet = new Set(right);
  return left.filter((key) => !rightSet.has(key));
}

const enKeys = loadKeys('src/i18n/locales/en.ts');
const zhKeys = loadKeys('src/i18n/locales/zh.ts');
const missingInZh = diff(enKeys, zhKeys);
const missingInEn = diff(zhKeys, enKeys);
const usedKeys = loadUsedKeys();
const missingUsedKeys = usedKeys.filter((key) => !enKeys.includes(key) || !zhKeys.includes(key));

if (missingInZh.length > 0 || missingInEn.length > 0 || missingUsedKeys.length > 0) {
  if (missingInZh.length > 0) {
    console.error(`Missing zh locale keys:\n${missingInZh.join('\n')}`);
  }
  if (missingInEn.length > 0) {
    console.error(`Missing en locale keys:\n${missingInEn.join('\n')}`);
  }
  if (missingUsedKeys.length > 0) {
    console.error(`Missing locale keys used in source:\n${missingUsedKeys.join('\n')}`);
  }
  process.exit(1);
}

console.log(`i18n locale keys match (${enKeys.length} keys, ${usedKeys.length} static usages).`);
