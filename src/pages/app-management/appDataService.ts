import { invokeCommand } from '../../lib/tauri';
import type {
  AppDataActionOutcome,
  AppDataItemDetail,
  AppDataMaintenanceAction,
  AppDataOverview,
} from './appDataTypes';

export async function fetchAppDataOverview(): Promise<AppDataOverview> {
  return invokeCommand<AppDataOverview>('app_data_get_overview');
}

export async function fetchAppDataItemDetail(
  relativePath?: string | null,
): Promise<AppDataItemDetail> {
  return invokeCommand<AppDataItemDetail>('app_data_get_item_detail', {
    relativePath: relativePath ?? null,
  });
}

export async function runAppDataAction(
  action: AppDataMaintenanceAction,
): Promise<AppDataActionOutcome> {
  return invokeCommand<AppDataActionOutcome>('app_data_run_action', { action });
}
