import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)));

function read(relativePath) {
  return readFileSync(join(repoRoot, relativePath), 'utf8');
}

function fail(messages) {
  console.error(`Frontend smoke check failed:\n${messages.join('\n')}`);
  process.exit(1);
}

function resolveTsxImport(fromRelativePath, importPath) {
  const fromDir = dirname(join(repoRoot, fromRelativePath));
  return normalize(join(fromDir, `${importPath}.tsx`));
}

const problems = [];
const routerPath = 'src/app/router.tsx';
const router = read(routerPath);
const main = read('src/main.tsx');
const appShell = read('src/layouts/AppShell.tsx');

for (const name of ['ThemeProvider', 'I18nProvider', 'RouterProvider']) {
  if (!main.includes(`<${name}`)) {
    problems.push(`src/main.tsx: missing ${name} wrapper.`);
  }
}

for (const name of ['SourceAppsProvider', 'NotificationCenter', 'Sidebar', 'Outlet']) {
  if (!appShell.includes(name)) {
    problems.push(`src/layouts/AppShell.tsx: missing ${name}.`);
  }
}

const pageImports = [
  ...router.matchAll(/from ['"](\.\.\/pages\/[^'"]+)['"]/g),
  ...router.matchAll(/import\(\s*['"](\.\.\/pages\/[^'"]+)['"]\s*\)/g),
].map((match) => match[1]);
for (const importPath of pageImports) {
  const resolved = resolveTsxImport(routerPath, importPath);
  if (!existsSync(resolved)) {
    problems.push(`${routerPath}: page import does not resolve: ${importPath}.`);
  }
}

const routes = [...router.matchAll(/path:\s*['"]([^'"]+)['"]/g)].map((match) => match[1]);
const duplicateRoutes = routes.filter((route, index) => routes.indexOf(route) !== index);
if (duplicateRoutes.length > 0) {
  problems.push(`${routerPath}: duplicate routes: ${[...new Set(duplicateRoutes)].join(', ')}.`);
}

const requiredRoutes = [
  'sessions',
  'messages',
  'sources',
  'management/scan-records',
  'management/app-data',
  'statistics',
  'models',
  'settings',
];
for (const route of requiredRoutes) {
  if (!routes.includes(route)) {
    problems.push(`${routerPath}: missing route "${route}".`);
  }
}

if (
  !/{\s*index:\s*true,\s*element:\s*(?:<DashboardPage\s*\/>|routeElement\(DashboardPage\))\s*}/s.test(
    router,
  )
) {
  problems.push(`${routerPath}: missing dashboard index route.`);
}

if (!router.includes('element: <AppShell />')) {
  problems.push(`${routerPath}: root route must render AppShell.`);
}

if (problems.length > 0) {
  fail(problems);
}

console.log(
  `Frontend smoke checks pass (${pageImports.length} routed pages, ${routes.length} named routes).`,
);
