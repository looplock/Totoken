import { invokeCommand } from '../../lib/tauri';
import type {
  ModelCatalogListQuery,
  ModelCatalogListResponse,
  ModelCatalogSyncRun,
} from './modelTypes';

export async function fetchModelCatalog(
  query?: ModelCatalogListQuery,
): Promise<ModelCatalogListResponse> {
  return invokeCommand<ModelCatalogListResponse>('list_models', {
    query,
  });
}

export async function refreshModelCatalog(): Promise<ModelCatalogSyncRun> {
  return invokeCommand<ModelCatalogSyncRun>('refresh_model_catalog');
}
