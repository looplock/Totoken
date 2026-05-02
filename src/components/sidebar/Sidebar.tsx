import { useEffect, useState } from 'react';
import { ChevronDown, PanelLeftClose, PanelLeftOpen, type LucideIcon } from 'lucide-react';
import { NavLink, useLocation } from 'react-router-dom';
import { navItems } from './nav';
import { useI18n } from '../../i18n/useI18n';
import { IconButton } from '../icon-button/IconButton';
import brandLogo from '../../assets/logo/logo.png';
import './Sidebar.css';

export function Sidebar({
  collapsed,
  onToggleCollapsed,
}: {
  collapsed: boolean;
  onToggleCollapsed: () => void;
}) {
  const { t } = useI18n();
  const location = useLocation();
  const [expandedGroups, setExpandedGroups] = useState<Record<string, boolean>>({});
  const collapseLabel = collapsed ? t('sidebar.expand') : t('sidebar.collapse');

  useEffect(() => {
    setExpandedGroups((current) => {
      const next = { ...current };
      let changed = false;

      for (const item of navItems) {
        if (item.type !== 'group') {
          continue;
        }

        if (
          item.children.some((child) => isRouteActive(location.pathname, child.to)) &&
          !next[item.id]
        ) {
          next[item.id] = true;
          changed = true;
        }
      }

      return changed ? next : current;
    });
  }, [location.pathname]);

  return (
    <aside className={collapsed ? 'sidebar sidebar-collapsed' : 'sidebar'}>
      <div className="sidebar-brand">
        <div className="sidebar-brand-main">
          <img className="brand-mark" src={brandLogo} alt="" aria-hidden="true" />
          <div className="sidebar-brand-copy">
            <div className="brand-title">{t('brand.name')}</div>
            <div className="brand-subtitle">{t('brand.subtitle')}</div>
          </div>
          {collapsed ? (
            <IconButton
              className="sidebar-collapse-toggle sidebar-collapse-toggle-collapsed"
              onClick={onToggleCollapsed}
              label={collapseLabel}
            >
              <PanelLeftOpen size={17} />
            </IconButton>
          ) : null}
        </div>
        {!collapsed ? (
          <IconButton
            className="sidebar-collapse-toggle"
            onClick={onToggleCollapsed}
            label={collapseLabel}
          >
            <PanelLeftClose size={17} />
          </IconButton>
        ) : null}
      </div>

      <nav className="sidebar-nav">
        {navItems.map((item) => {
          if (item.type === 'link') {
            return (
              <SidebarLink
                key={item.to}
                label={t(item.labelKey)}
                to={item.to}
                icon={item.icon}
                collapsed={collapsed}
              />
            );
          }

          const Icon = item.icon;
          const label = t(item.labelKey);
          const hasActiveChild = item.children.some((child) =>
            isRouteActive(location.pathname, child.to),
          );
          const isExpanded = expandedGroups[item.id] ?? hasActiveChild;

          return (
            <div key={item.id} className="sidebar-group">
              <button
                type="button"
                className={
                  hasActiveChild
                    ? 'sidebar-group-trigger sidebar-group-trigger-active'
                    : 'sidebar-group-trigger'
                }
                aria-expanded={isExpanded}
                aria-label={label}
                title={label}
                onClick={() =>
                  setExpandedGroups((current) => ({
                    ...current,
                    [item.id]: !isExpanded,
                  }))
                }
              >
                <span className="sidebar-link-icon" aria-hidden="true">
                  <Icon size={16} strokeWidth={2} />
                </span>
                <span className="sidebar-group-label">{label}</span>
                <span
                  className={
                    isExpanded
                      ? 'sidebar-group-chevron sidebar-group-chevron-expanded'
                      : 'sidebar-group-chevron'
                  }
                  aria-hidden="true"
                >
                  <ChevronDown size={15} strokeWidth={2.1} />
                </span>
              </button>

              {isExpanded ? (
                <div className="sidebar-group-children">
                  {item.children.map((child) => (
                    <SidebarLink
                      key={child.to}
                      label={t(child.labelKey)}
                      to={child.to}
                      icon={child.icon}
                      compact
                      collapsed={collapsed}
                    />
                  ))}
                </div>
              ) : null}
            </div>
          );
        })}
      </nav>
    </aside>
  );
}

function SidebarLink({
  label,
  to,
  icon: Icon,
  compact,
  collapsed,
}: {
  label: string;
  to: string;
  icon: LucideIcon;
  compact?: boolean;
  collapsed: boolean;
}) {
  return (
    <NavLink
      to={to}
      end={to === '/'}
      title={label}
      aria-label={label}
      className={({ isActive }) => {
        const baseClass = compact ? 'sidebar-link sidebar-link-compact' : 'sidebar-link';
        return isActive ? `${baseClass} sidebar-link-active` : baseClass;
      }}
    >
      <span className="sidebar-link-icon" aria-hidden="true">
        <Icon size={16} strokeWidth={2} />
      </span>
      <span
        className={
          collapsed ? 'sidebar-link-label sidebar-link-label-hidden' : 'sidebar-link-label'
        }
      >
        {label}
      </span>
    </NavLink>
  );
}

function isRouteActive(pathname: string, to: string) {
  if (to === '/') {
    return pathname === '/';
  }

  return pathname === to || pathname.startsWith(`${to}/`);
}
