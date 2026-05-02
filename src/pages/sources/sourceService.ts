import { invokeCommand } from '../../lib/tauri';
import type { SourcePatch, SourceState } from './sourceData';

export async function fetchSources(): Promise<SourceState> {
  return invokeCommand<SourceState>('sources_list');
}

export async function updateSource(id: string, patch: SourcePatch): Promise<SourceState> {
  return invokeCommand<SourceState>('sources_update', { id, patch });
}
