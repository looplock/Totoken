import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const srcRoot = join(repoRoot, 'src');
const tokensPath = join(repoRoot, 'src/styles/tokens.css');
const globalPath = join(repoRoot, 'src/styles/global.css');
const tokenReferencePrefixes = ['--color-', '--shadow-', '--scrollbar-'];

function listCssFiles(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const fullPath = join(dir, entry.name);
    if (entry.isDirectory()) {
      return listCssFiles(fullPath);
    }
    return entry.isFile() && entry.name.endsWith('.css') ? [fullPath] : [];
  });
}

function rel(path) {
  return relative(repoRoot, path).replaceAll('\\', '/');
}

function fail(messages) {
  console.error(`CSS convention check failed:\n${messages.join('\n')}`);
  process.exit(1);
}

if (!existsSync(tokensPath)) {
  fail(['src/styles/tokens.css is missing.']);
}

const globalCss = readFileSync(globalPath, 'utf8');
if (!globalCss.startsWith("@import './tokens.css';")) {
  fail(['src/styles/global.css must import ./tokens.css first.']);
}

const tokensCss = readFileSync(tokensPath, 'utf8');
const tokenDefinitions = new Set(
  [...tokensCss.matchAll(/(--(?:color|shadow|scrollbar)-[\w-]+)\s*:/g)].map((match) => match[1]),
);

const cssFiles = listCssFiles(srcRoot);
const problems = [];

for (const filePath of cssFiles) {
  const content = readFileSync(filePath, 'utf8');
  const displayPath = rel(filePath);
  const openCount = (content.match(/\{/g) ?? []).length;
  const closeCount = (content.match(/\}/g) ?? []).length;

  if (openCount !== closeCount) {
    problems.push(`${displayPath}: unbalanced braces (${openCount} "{" vs ${closeCount} "}").`);
  }

  if (/letter-spacing\s*:\s*-\d/.test(content)) {
    problems.push(`${displayPath}: negative letter-spacing is not allowed.`);
  }

  if (
    filePath !== tokensPath &&
    /:root[^{]*\{[^}]*--(?:color|shadow|scrollbar)-[\w-]+\s*:/s.test(content)
  ) {
    problems.push(`${displayPath}: global design tokens must live in src/styles/tokens.css.`);
  }

  for (const match of content.matchAll(/var\(\s*(--[\w-]+)/g)) {
    const tokenName = match[1];
    if (
      tokenReferencePrefixes.some((prefix) => tokenName.startsWith(prefix)) &&
      !tokenDefinitions.has(tokenName)
    ) {
      problems.push(`${displayPath}: ${tokenName} is referenced but not defined in tokens.css.`);
    }
  }
}

if (problems.length > 0) {
  fail(problems);
}

console.log(
  `CSS conventions pass (${cssFiles.length} files, ${tokenDefinitions.size} shared tokens).`,
);
