import { useEffect, useState } from 'react';
import { Outlet } from 'react-router-dom';
import { NotificationCenter } from '../components/notifications/NotificationCenter';
import { Sidebar } from '../components/sidebar/Sidebar';
import { SourceAppsProvider } from '../lib/useEnabledSourceApps';
import './AppShell.css';

const SIDEBAR_COLLAPSED_STORAGE_KEY = 'totoken.sidebar.collapsed';

export function AppShell() {
  const [isSidebarCollapsed, setIsSidebarCollapsed] = useState(() => {
    if (typeof window === 'undefined') {
      return false;
    }

    return window.localStorage.getItem(SIDEBAR_COLLAPSED_STORAGE_KEY) === 'true';
  });

  useEffect(() => {
    window.localStorage.setItem(
      SIDEBAR_COLLAPSED_STORAGE_KEY,
      isSidebarCollapsed ? 'true' : 'false',
    );
  }, [isSidebarCollapsed]);

  return (
    <SourceAppsProvider>
      <div className={isSidebarCollapsed ? 'app-shell app-shell-sidebar-collapsed' : 'app-shell'}>
        <Sidebar
          collapsed={isSidebarCollapsed}
          onToggleCollapsed={() => setIsSidebarCollapsed((current) => !current)}
        />
        <NotificationCenter />
        <main className="app-main">
          <section className="app-content">
            <Outlet />
          </section>
        </main>
      </div>
    </SourceAppsProvider>
  );
}
