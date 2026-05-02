export type AppDataOverview = {
  storage: {
    bootstrapDir: string;
    dataDir: string;
    dbPath: string;
    usingDefaultDir: boolean;
    restartRequired: boolean;
  };
  summary: {
    totalSizeBytes: number;
    fileCount: number;
    directoryCount: number;
    configCount: number;
    cacheSizeBytes: number;
    backupCount: number;
    lastBackupAt: string | null;
  };
  items: AppDataItem[];
  defaultSelectedPath: string | null;
};

export type AppDataItem = {
  relativePath: string;
  name: string;
  fullPath: string;
  itemType: 'file' | 'directory' | string;
  category: string;
  health: string;
  sizeBytes: number;
  modifiedAt: string | null;
  children: AppDataItem[];
};

export type AppDataItemDetail = {
  relativePath: string;
  name: string;
  fullPath: string;
  itemType: 'file' | 'directory' | string;
  category: string;
  health: string;
  sizeBytes: number;
  modifiedAt: string | null;
  entryCount: number | null;
  preview: string | null;
  previewLanguage: string | null;
  sqlite: {
    tableCount: number;
    indexCount: number;
    pageCount: number;
    freelistCount: number;
    pageSizeBytes: number;
    integrity: string;
  } | null;
  recommendedActions: AppDataMaintenanceAction[];
};

export type AppDataMaintenanceAction =
  | 'create_backup'
  | 'vacuum_database'
  | 'rebuild_indexes'
  | 'clear_caches';

export type AppDataActionOutcome = {
  overview: AppDataOverview;
  reclaimedBytes: number | null;
};
