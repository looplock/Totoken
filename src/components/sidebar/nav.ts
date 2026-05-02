import {
  BarChart3,
  Briefcase,
  Database,
  FolderKanban,
  Gauge,
  HardDrive,
  MessageSquareText,
  Settings,
  TableProperties,
  Workflow,
  type LucideIcon,
} from 'lucide-react';

export type NavLinkItem = {
  type: 'link';
  labelKey: string;
  to: string;
  icon: LucideIcon;
};

export type NavGroup = {
  type: 'group';
  id: string;
  labelKey: string;
  icon: LucideIcon;
  children: NavLinkItem[];
};

export type NavItem = NavLinkItem | NavGroup;

export const navItems: NavItem[] = [
  { type: 'link', labelKey: 'nav.dashboard', to: '/', icon: Gauge },
  {
    type: 'group',
    id: 'sessions',
    labelKey: 'nav.sessions',
    icon: FolderKanban,
    children: [
      {
        type: 'link',
        labelKey: 'nav.sessions',
        to: '/sessions',
        icon: FolderKanban,
      },
      {
        type: 'link',
        labelKey: 'nav.messages',
        to: '/messages',
        icon: MessageSquareText,
      },
    ],
  },
  { type: 'link', labelKey: 'nav.sources', to: '/sources', icon: Database },
  { type: 'link', labelKey: 'nav.statistics', to: '/statistics', icon: BarChart3 },
  { type: 'link', labelKey: 'nav.models', to: '/models', icon: TableProperties },
  {
    type: 'group',
    id: 'app-management',
    labelKey: 'nav.appManagement',
    icon: Briefcase,
    children: [
      {
        type: 'link',
        labelKey: 'nav.scanRecords',
        to: '/management/scan-records',
        icon: Workflow,
      },
      {
        type: 'link',
        labelKey: 'nav.appData',
        to: '/management/app-data',
        icon: HardDrive,
      },
    ],
  },
  { type: 'link', labelKey: 'nav.settings', to: '/settings', icon: Settings },
];
