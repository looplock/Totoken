import { Suspense, lazy, type ComponentType } from 'react';
import { Navigate, createBrowserRouter } from 'react-router-dom';
import { AppShell } from '../layouts/AppShell';
import { RouteFallback } from './RouteFallback';

const DashboardPage = lazyNamed(() => import('../pages/dashboard/DashboardPage'), 'DashboardPage');
const SessionsPage = lazyNamed(() => import('../pages/sessions/SessionsPage'), 'SessionsPage');
const MessagesPage = lazyNamed(() => import('../pages/messages/MessagesPage'), 'MessagesPage');
const SourcesPage = lazyNamed(() => import('../pages/sources/SourcesPage'), 'SourcesPage');
const ScanRecordsPage = lazyNamed(
  () => import('../pages/scan-tasks/ScanTasksPage'),
  'ScanRecordsPage',
);
const AppDataPage = lazyNamed(() => import('../pages/app-management/AppDataPage'), 'AppDataPage');
const StatisticsPage = lazyNamed(
  () => import('../pages/statistics/StatisticsPage'),
  'StatisticsPage',
);
const ModelsPage = lazyNamed(() => import('../pages/models/ModelsPage'), 'ModelsPage');
const SettingsPage = lazyNamed(() => import('../pages/settings/SettingsPage'), 'SettingsPage');

export const router = createBrowserRouter([
  {
    path: '/',
    element: <AppShell />,
    children: [
      { index: true, element: routeElement(DashboardPage) },
      { path: 'sessions', element: routeElement(SessionsPage) },
      { path: 'messages', element: routeElement(MessagesPage) },
      { path: 'sources', element: routeElement(SourcesPage) },
      { path: 'scan-tasks', element: <Navigate to="/management/scan-records" replace /> },
      { path: 'management', element: <Navigate to="/management/scan-records" replace /> },
      { path: 'management/scan-records', element: routeElement(ScanRecordsPage) },
      { path: 'management/app-data', element: routeElement(AppDataPage) },
      { path: 'statistics', element: routeElement(StatisticsPage) },
      { path: 'models', element: routeElement(ModelsPage) },
      { path: 'settings', element: routeElement(SettingsPage) },
    ],
  },
]);

function lazyNamed<TModule extends Record<TExport, ComponentType>, TExport extends string>(
  loader: () => Promise<TModule>,
  exportName: TExport,
) {
  return lazy(async () => {
    const module = await loader();
    return { default: module[exportName] };
  });
}

function routeElement(Page: ComponentType) {
  return (
    <Suspense fallback={<RouteFallback />}>
      <Page />
    </Suspense>
  );
}
