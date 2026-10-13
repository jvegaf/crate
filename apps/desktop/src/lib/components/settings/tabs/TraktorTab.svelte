<script lang="ts">
	import { Button, Text, Spinner, ConfirmModal } from '$lib/components/common'
	import { traktorStore } from '$lib/stores/traktor'
	import { toastStore } from '$shared/stores/toast'
	import { translate } from '$shared/i18n'
	import { withNativeDialog } from '$shared/utils'
	import { open } from '@tauri-apps/plugin-dialog'
	import { get } from 'svelte/store'

	let nmlPath = $state<string | null>(null)
	let choosing = $state(false)
	let showConfirm = $state(false)

	let plan = $derived($traktorStore.plan)
	let outcome = $derived($traktorStore.outcome)
	let busy = $derived($traktorStore.previewing || $traktorStore.importing)

	let warnings = $derived.by(() => {
		const w = plan?.report.warnings
		return {
			withoutGrid: w?.tracks_without_grid.length ?? 0,
			withoutTempo: w?.tracks_without_tempo.length ?? 0,
			withoutKey: w?.tracks_without_key.length ?? 0,
			unresolvedKeys: w?.unresolved_keys.length ?? 0,
			collapsedCueDuplicates: w?.collapsed_cue_duplicates ?? 0,
			unsupportedCueTypes: w?.unsupported_cue_types.length ?? 0,
			unresolvedPlaylistReferences: w?.unresolved_playlist_references.length ?? 0,
		}
	})

	let warningTotal = $derived(
		warnings.withoutGrid +
			warnings.withoutTempo +
			warnings.withoutKey +
			warnings.unresolvedKeys +
			warnings.collapsedCueDuplicates +
			warnings.unsupportedCueTypes +
			warnings.unresolvedPlaylistReferences
	)

	let confirmMessage = $derived.by(() => {
		const totals = plan?.report.totals
		const playlists = plan?.report.playlists_by_type
		return $translate('settings.traktor.confirmBody', {
			values: {
				tracks: totals?.tracks ?? 0,
				playlists: (playlists?.playlists ?? 0) + (playlists?.smartlists ?? 0),
			},
		})
	})

	// Fresh start every time the tab is opened.
	$effect(() => {
		traktorStore.reset()
	})

	function surfaceError() {
		const message = get(traktorStore).error
		if (message) toastStore.error(message)
	}

	async function handleChoose() {
		if (choosing) return
		choosing = true
		try {
			const selected = await withNativeDialog(() =>
				open({
					filters: [{ name: 'Traktor Collection', extensions: ['nml'] }],
					multiple: false,
				})
			)
			const path = typeof selected === 'string' ? selected : null
			if (!path) return
			nmlPath = path
			await traktorStore.preview(path)
			surfaceError()
		} finally {
			choosing = false
		}
	}

	function handleImport() {
		showConfirm = true
	}

	async function confirmImport() {
		showConfirm = false
		// Guard against a double dispatch before the modal unmounts: `run` sets
		// `importing` synchronously, so a second confirm in the same frame bails out.
		if (!nmlPath || busy) return
		await traktorStore.run(nmlPath)
		if (get(traktorStore).outcome) {
			toastStore.success($translate('settings.traktor.importComplete'))
		} else {
			surfaceError()
		}
	}

	function reasonLabel(reason: string): string {
		switch (reason) {
			case 'file-not-found':
				return $translate('settings.traktor.reasonFileNotFound')
			case 'hash-failed':
				return $translate('settings.traktor.reasonHashFailed')
			case 'metadata-failed':
				return $translate('settings.traktor.reasonMetadataFailed')
			default:
				return reason
		}
	}
</script>

{#snippet stat(label: string, value: number)}
	<div>
		<Text variant="caption" as="p">{label}</Text>
		<Text variant="body-2" as="p" tabular>{value}</Text>
	</div>
{/snippet}

{#snippet warningRow(label: string, count: number)}
	<div class="flex items-center justify-between gap-3">
		<Text variant="caption" as="p">{label}</Text>
		<Text variant="caption" as="p" tabular color={count > 0 ? 'warning' : undefined}>{count}</Text>
	</div>
{/snippet}

<div class="space-y-8">
	<section>
		<Text variant="header-3" class="mb-2">{$translate('settings.traktor.title')}</Text>
		<Text variant="caption" as="p" class="mb-4">{$translate('settings.traktor.description')}</Text>

		<div class="flex items-center justify-between gap-4 rounded-lg border border-stroke bg-surface-1 px-4 py-3">
			<div class="min-w-0 flex-1">
				{#if nmlPath}
					<Text variant="caption" as="p" truncate title={nmlPath}>{nmlPath}</Text>
				{:else}
					<Text variant="caption" as="p" class="text-text-tertiary">
						{$translate('settings.traktor.noFile')}
					</Text>
				{/if}
			</div>
			<div class="flex flex-shrink-0 items-center gap-2">
				<Button variant="secondary" size="sm" onclick={handleChoose} disabled={choosing || busy}>
					{$translate(nmlPath ? 'settings.traktor.changeFile' : 'settings.traktor.choose')}
				</Button>
			</div>
		</div>

		{#if $traktorStore.previewing}
			<div class="mt-3 flex items-center gap-2">
				<Spinner class="h-4 w-4" />
				<Text variant="body-2" as="p">{$translate('settings.traktor.previewing')}</Text>
			</div>
		{/if}

		{#if $traktorStore.importing}
			<div class="mt-3 flex items-center gap-2">
				<Spinner class="h-4 w-4" />
				<Text variant="body-2" as="p">{$translate('settings.traktor.importing')}</Text>
			</div>
		{/if}

		{#if $traktorStore.error}
			<Text variant="caption" as="p" color="danger" class="mt-2">
				{$translate('settings.traktor.error', { values: { error: $traktorStore.error } })}
			</Text>
		{/if}
	</section>

	{#if plan && !outcome}
		<section>
			<Text variant="header-3" class="mb-3">{$translate('settings.traktor.previewTitle')}</Text>

			<div class="grid grid-cols-2 gap-x-6 gap-y-3 sm:grid-cols-4">
				{@render stat($translate('settings.traktor.tracks'), plan.report.totals.tracks)}
				{@render stat($translate('settings.traktor.cues'), plan.report.totals.cues)}
				{@render stat($translate('settings.traktor.memoryCues'), plan.report.totals.memory_cues)}
				{@render stat($translate('settings.traktor.hotCues'), plan.report.totals.hot_cues)}
				{@render stat($translate('settings.traktor.loops'), plan.report.totals.loops)}
				{@render stat($translate('settings.traktor.grids'), plan.report.totals.grids)}
				{@render stat($translate('settings.traktor.folders'), plan.report.playlists_by_type.folders)}
				{@render stat($translate('settings.traktor.playlists'), plan.report.playlists_by_type.playlists)}
				{@render stat($translate('settings.traktor.smartlists'), plan.report.playlists_by_type.smartlists)}
			</div>

			<div class="mt-4 rounded-lg border border-stroke bg-surface-1 p-4">
				<Text variant="header-4" as="p" class="mb-2">{$translate('settings.traktor.warningsTitle')}</Text>
				{#if warningTotal === 0}
					<Text variant="caption" as="p">{$translate('settings.traktor.warningsNone')}</Text>
				{:else}
					<div class="grid grid-cols-1 gap-x-6 gap-y-2 sm:grid-cols-2">
						{@render warningRow($translate('settings.traktor.withoutGrid'), warnings.withoutGrid)}
						{@render warningRow($translate('settings.traktor.withoutTempo'), warnings.withoutTempo)}
						{@render warningRow($translate('settings.traktor.withoutKey'), warnings.withoutKey)}
						{@render warningRow($translate('settings.traktor.unresolvedKeys'), warnings.unresolvedKeys)}
						{@render warningRow($translate('settings.traktor.collapsedCueDuplicates'), warnings.collapsedCueDuplicates)}
						{@render warningRow($translate('settings.traktor.unsupportedCueTypes'), warnings.unsupportedCueTypes)}
						{@render warningRow(
							$translate('settings.traktor.unresolvedPlaylistReferences'),
							warnings.unresolvedPlaylistReferences
						)}
					</div>
				{/if}
			</div>

			<div class="mt-4">
				<Button variant="primary" onclick={handleImport} disabled={busy}>
					{$translate('settings.traktor.import')}
				</Button>
			</div>
		</section>
	{/if}

	{#if outcome}
		<section>
			<Text variant="header-3" class="mb-3">{$translate('settings.traktor.outcomeTitle')}</Text>

			<div class="grid grid-cols-2 gap-x-6 gap-y-3 sm:grid-cols-4">
				{@render stat($translate('settings.traktor.matched'), outcome.matched)}
				{@render stat($translate('settings.traktor.ingested'), outcome.ingested)}
				{@render stat($translate('settings.traktor.cuesWritten'), outcome.cues_written)}
				{@render stat($translate('settings.traktor.gridsWritten'), outcome.grids_written)}
				{@render stat($translate('settings.traktor.keysWritten'), outcome.keys_written)}
				{@render stat($translate('settings.traktor.playlistsImported'), outcome.playlists_imported)}
				{@render stat($translate('settings.traktor.playlistTracksWritten'), outcome.playlist_tracks_written)}
				{@render stat($translate('settings.traktor.unresolvedEntries'), outcome.unresolved_playlist_entries)}
			</div>

			{#if outcome.skipped_internal_playlists.length > 0}
				<div class="mt-4">
					<Text variant="caption" as="p" class="mb-1">{$translate('settings.traktor.skippedInternal')}</Text>
					<div class="flex flex-wrap gap-1">
						{#each outcome.skipped_internal_playlists as name (name)}
							<Text variant="caption" as="span" class="rounded bg-surface-2 px-2 py-0.5">{name}</Text>
						{/each}
					</div>
				</div>
			{/if}

			{#if outcome.unmatched.length > 0}
				<div class="mt-4">
					<Text variant="caption" as="p" class="mb-1">{$translate('settings.traktor.unmatchedTitle')}</Text>
					<div class="max-h-40 overflow-auto rounded-lg border border-stroke bg-surface-1">
						{#each outcome.unmatched as entry (entry.file_path + entry.reason)}
							<div class="border-b border-stroke px-3 py-2 last:border-b-0">
								<Text variant="caption" as="p" truncate title={entry.file_path}>
									{entry.title ?? entry.file_path}
								</Text>
								<Text variant="caption" as="p" color="warning">{reasonLabel(entry.reason)}</Text>
							</div>
						{/each}
					</div>
				</div>
			{/if}
		</section>
	{/if}
</div>

<ConfirmModal
	open={showConfirm}
	title={$translate('settings.traktor.confirmTitle')}
	message={confirmMessage}
	confirmLabel={$translate('settings.traktor.import')}
	onConfirm={confirmImport}
	onCancel={() => (showConfirm = false)}
/>
