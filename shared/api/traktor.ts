import { invoke } from '@tauri-apps/api/core'
import type { TraktorImportOutcome, TraktorImportPlan } from '../types'

/**
 * Parse + map a Traktor `collection.nml` into a dry-run import plan (no DB writes)
 */
export async function previewTraktorImport(nmlPath: string): Promise<TraktorImportPlan> {
	return invoke<TraktorImportPlan>('preview_traktor_import', { nmlPath })
}

/**
 * Import a Traktor `collection.nml` into the library, returning what actually happened
 */
export async function importTraktorCollection(nmlPath: string): Promise<TraktorImportOutcome> {
	return invoke<TraktorImportOutcome>('import_traktor_collection', { nmlPath })
}
