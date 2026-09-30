<script lang="ts">
	import { translate } from '$shared/i18n'
	import { taggerStore, type BatchRow } from '$shared/stores/tagger'
	import type { ScoredTagCandidate, Track } from '$shared/types'
	import { formatDuration } from '$shared/utils'
	import AlbumArt from '$lib/components/common/AlbumArt.svelte'
	import Button from '$lib/components/common/Button.svelte'
	import Icon from '$lib/components/common/Icon.svelte'
	import Modal from '$lib/components/common/Modal.svelte'
	import Spinner from '$lib/components/common/Spinner.svelte'
	import Text from '$lib/components/common/Text.svelte'
	import { providerCapsuleClass, scoreClass } from './tones'

	type Props = {
		open: boolean
		tracks: Track[]
		onClose: () => void
		onApplied: (updated: Track[]) => void
	}

	let { open, tracks, onClose, onApplied }: Props = $props()

	/** A candidate duration this close to the local one is treated as a match. */
	const DURATION_TOLERANCE_MS = 5000

	// Search once when the modal opens (or when the selection changes while open).
	// The cleanup clears the batch so a closed modal never keeps stale rows.
	$effect(() => {
		if (open) {
			void taggerStore.searchBatch(tracks)
		}
		return () => {
			taggerStore.resetBatch()
		}
	})

	const rows = $derived($taggerStore.batchRows)
	const selections = $derived($taggerStore.batchSelections)
	const progress = $derived($taggerStore.batchProgress)
	const loading = $derived($taggerStore.batchLoading)
	const applying = $derived($taggerStore.batchApplying)
	const autoApply = $derived($taggerStore.autoApply)
	const autoApplying = $derived(autoApply.processed < autoApply.total)

	// "Skipped" counts an explicit "not available" only; an undecided row is neither.
	const toApplyCount = $derived(rows.filter((row) => selections.get(row.track.id) != null).length)
	const skippedCount = $derived(rows.filter((row) => selections.get(row.track.id) === null).length)
	const confirmDisabled = $derived(toApplyCount === 0 || applying)

	/** Provider ids are proper nouns: capitalise locally, never translate. */
	function capitalize(value: string): string {
		return value.charAt(0).toUpperCase() + value.slice(1)
	}

	function fileName(filePath: string): string {
		const segments = filePath.split(/[\\/]/)
		return segments[segments.length - 1] ?? filePath
	}

	function isDurationMatch(localMs: number, candidateMs: number | null): boolean {
		if (candidateMs === null) return false
		return Math.abs(localMs - candidateMs) <= DURATION_TOLERANCE_MS
	}

	/** `undefined` means undecided; `null` means an explicit "not available". */
	function selectionFor(row: BatchRow): ScoredTagCandidate | null | undefined {
		return selections.get(row.track.id)
	}

	function handleClose() {
		taggerStore.resetBatch()
		onClose()
	}

	async function handleApply() {
		if (confirmDisabled) return
		const result = await taggerStore.applyBatch()
		// A partial failure is toasted by the store; the modal stays open only when
		// nothing could be written, so the user still sees the rows.
		if (result.updated.length > 0) onApplied(result.updated)
	}
</script>

<Modal {open} title={$translate('tagger.batch.title')} size="xl" onClose={handleClose}>
	<div class="space-y-4">
		<p class="text-sm text-text-secondary">
			{$translate('tagger.batch.subtitle', { values: { count: tracks.length } })}
		</p>

		{#if loading}
			<div role="status" class="flex items-center gap-2 text-sm text-text-secondary">
				<Spinner class="h-4 w-4" />
				<span>
					{#if progress.currentTitle}
						{$translate('tagger.batch.searchingCurrent', {
							values: { title: progress.currentTitle, processed: progress.processed, total: progress.total },
						})}
					{:else}
						{$translate('tagger.batch.searching', {
							values: { processed: progress.processed, total: progress.total },
						})}
					{/if}
				</span>
			</div>
		{:else}
			<div class="space-y-4">
				{#if autoApply.total > 0}
					{#if autoApplying}
						<div role="status" class="flex items-center gap-2 text-sm text-text-secondary">
							<Spinner class="h-4 w-4" />
							<span>
								{$translate('tagger.batch.autoApplying', {
									values: { processed: autoApply.processed, total: autoApply.total },
								})}
							</span>
						</div>
					{:else}
						<div role="status" class="flex items-center gap-2 text-sm text-text-secondary">
							<Icon name="check" class="h-4 w-4 shrink-0 text-success" />
							<span>
								{$translate('tagger.batch.autoApplied', { values: { count: autoApply.updated.length } })}
							</span>
						</div>
						{#if autoApply.failed.length > 0}
							<div
								role="alert"
								class="flex items-center gap-2 rounded-md border border-warning/20 bg-warning/10 p-2 text-warning"
							>
								<Icon name="warning" class="h-4 w-4 shrink-0" />
								<Text as="span" variant="caption" color="warning">
									{$translate('tagger.batch.autoApplyFailed', { values: { count: autoApply.failed.length } })}
								</Text>
							</div>
						{/if}
					{/if}
				{/if}

				{#if rows.length === 0}
					<p class="rounded-md border border-stroke bg-surface-0 p-4 text-sm text-text-secondary">
						{$translate('tagger.batch.allAutoApplied', { values: { count: autoApply.updated.length } })}
					</p>
				{/if}

				{#each rows as row (row.track.id)}
					{@const selected = selectionFor(row)}
					<div class="rounded-md border border-stroke bg-surface-0">
						<!-- Local track header -->
						<div class="flex items-start justify-between gap-3 border-b border-stroke-subtle px-3 py-2.5">
							<div class="flex min-w-0 flex-col gap-0.5">
								<Text variant="body-2" truncate>{row.track.title || $translate('common.untitled')}</Text>
								<Text variant="caption" truncate>
									{row.track.artist || $translate('common.unknownArtist')}
								</Text>
								<div class="flex flex-wrap items-center gap-x-3 gap-y-0.5 text-xs text-text-tertiary">
									<span>{formatDuration(row.track.duration_ms)}</span>
									<span class="truncate" title={row.track.file_path}>{fileName(row.track.file_path)}</span>
								</div>
							</div>
							{#if row.error}
								<span
									class="flex shrink-0 items-center gap-1 rounded-full bg-danger/15 px-2 py-0.5 text-[11px] font-medium text-danger"
								>
									<Icon name="alert-circle" class="h-3 w-3" />
									{$translate('tagger.batch.error')}
								</span>
							{:else if row.candidates.length === 0}
								<span
									class="flex shrink-0 items-center gap-1 rounded-full bg-surface-2 px-2 py-0.5 text-[11px] font-medium text-text-tertiary"
								>
									<Icon name="alert-circle" class="h-3 w-3" />
									{$translate('tagger.batch.noResults')}
								</span>
							{/if}
						</div>

						{#if row.error}
							<p class="px-3 py-2 text-sm text-danger">{row.error}</p>
						{:else}
							{#if row.errors.length > 0}
								<div role="alert" class="mx-3 mt-3 space-y-1 rounded-md border border-warning/20 bg-warning/10 p-2">
									<div class="flex items-center gap-2 text-warning">
										<Icon name="warning" class="h-4 w-4 shrink-0" />
										<Text as="span" weight="medium" color="warning">
											{$translate('tagger.batch.providerErrors')}
										</Text>
									</div>
									<ul class="list-inside list-disc space-y-0.5 text-xs text-text-secondary">
										{#each row.errors as providerError (providerError.provider)}
											<li>{providerError.provider}: {providerError.error}</li>
										{/each}
									</ul>
								</div>
							{/if}

							<!-- Candidate cards, horizontally scrollable -->
							<div class="flex gap-3 overflow-x-auto px-3 py-3">
								{#each row.candidates as candidate, index (index)}
									{@const cardSelected = selected === candidate}
									{@const durationMatch = isDurationMatch(row.track.duration_ms, candidate.duration_ms)}
									<button
										type="button"
										aria-pressed={cardSelected}
										onclick={() => taggerStore.selectFor(row.track.id, candidate)}
										class="relative flex w-52 shrink-0 flex-col gap-2 rounded-md border p-3 text-left transition-colors {cardSelected
											? 'border-brand-primary bg-brand-muted'
											: 'border-stroke-subtle bg-surface-1 hover:bg-surface-2/50'}"
									>
										{#if cardSelected}
											<span
												class="absolute top-2 right-2 flex h-4 w-4 items-center justify-center rounded-full bg-brand-primary text-white"
											>
												<Icon name="check" class="h-3 w-3" />
											</span>
										{/if}

										<div class="flex items-start gap-2">
											<AlbumArt artworkPath={null} artworkUrl={candidate.artwork_url} size="xs" />
											<div class="flex min-w-0 flex-col items-start gap-1">
												<div class="flex flex-wrap items-center gap-1">
													<span class={providerCapsuleClass(candidate.provider)}>
														{capitalize(candidate.provider)}
													</span>
													<span
														class="shrink-0 rounded px-1.5 py-0.5 text-xs font-medium {scoreClass(
															candidate.similarity_score
														)}"
														title={$translate('tagger.score')}
													>
														{Math.round(candidate.similarity_score * 100)}%
													</span>
												</div>
												{#if candidate.version}
													<span
														class="rounded-full bg-purple-500/15 px-2 py-0.5 text-[11px] font-medium text-purple-500"
													>
														{candidate.version}
													</span>
												{/if}
											</div>
										</div>

										<Text as="span" weight="medium" truncate>
											{candidate.title || $translate('common.untitled')}
										</Text>
										<Text as="span" variant="caption" truncate>
											{candidate.artists.length > 0 ? candidate.artists.join(', ') : $translate('common.unknownArtist')}
										</Text>

										<div class="flex flex-wrap gap-x-3 gap-y-0.5 text-xs text-text-tertiary">
											{#if candidate.duration_ms !== null}
												<span class="inline-flex items-center gap-1">
													{formatDuration(candidate.duration_ms)}
													{#if durationMatch}
														<Icon name="check" class="h-3 w-3 text-success" />
													{/if}
												</span>
											{/if}
											{#if candidate.bpm !== null}
												<span>{$translate('modals.trackMetadata.fields.bpm')} {candidate.bpm}</span>
											{/if}
											{#if candidate.key}
												<span>{$translate('modals.trackMetadata.fields.key')}: {candidate.key}</span>
											{/if}
											{#if candidate.genre}
												<span class="truncate">
													{$translate('modals.trackMetadata.fields.genre')}: {candidate.genre}
												</span>
											{/if}
											{#if candidate.release_date}
												<span>{candidate.release_date.slice(0, 10)}</span>
											{/if}
											{#if candidate.label}
												<span class="truncate">
													{$translate('modals.trackMetadata.fields.label')}: {candidate.label}
												</span>
											{/if}
										</div>
									</button>
								{/each}

								<!-- Explicit skip: distinguishable from "not decided yet" -->
								<button
									type="button"
									aria-pressed={selected === null}
									onclick={() => taggerStore.selectFor(row.track.id, null)}
									class="relative flex w-28 shrink-0 flex-col items-center justify-center gap-1 rounded-md border p-3 text-center transition-colors {selected ===
									null
										? 'border-brand-primary bg-brand-muted'
										: 'border-stroke-subtle bg-surface-1 hover:bg-surface-2/50'}"
								>
									{#if selected === null}
										<span
											class="absolute top-2 right-2 flex h-4 w-4 items-center justify-center rounded-full bg-brand-primary text-white"
										>
											<Icon name="check" class="h-3 w-3" />
										</span>
									{/if}
									<Icon name="x" class="h-5 w-5 text-text-tertiary" />
									<span class="text-xs font-medium text-text-secondary">
										{$translate('tagger.batch.notAvailable')}
									</span>
								</button>
							</div>
						{/if}
					</div>
				{/each}
			</div>
		{/if}
	</div>

	{#snippet footer()}
		<span class="mr-auto text-xs text-text-tertiary">
			{$translate('tagger.batch.stats', { values: { toApply: toApplyCount, skipped: skippedCount } })}
		</span>
		<Button variant="ghost" onclick={handleClose} disabled={applying}>
			{$translate('common.cancel')}
		</Button>
		<Button variant="primary" onclick={handleApply} disabled={confirmDisabled}>
			{applying
				? $translate('tagger.batch.applying')
				: $translate('tagger.batch.confirm', { values: { count: toApplyCount } })}
		</Button>
	{/snippet}
</Modal>
