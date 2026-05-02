import type { SourceApp } from '../../lib/sourceApps';

export type SourceScanPath = {
  path: string;
  exists: boolean;
};

export type SourceRecord = {
  id: string;
  app: SourceApp;
  rootPath: string;
  scanPaths: SourceScanPath[];
  enabled: boolean;
  rootPathExists: boolean;
  scanSupported: boolean;
};

export type SourceState = {
  items: SourceRecord[];
};

export type SourcePatch = {
  enabled?: boolean;
};
