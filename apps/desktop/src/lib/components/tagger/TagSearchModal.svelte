<script lang="ts">
	import { translate } from '$shared/i18n'
	import { taggerStore } from '$shared/stores/tagger'
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
		track: Track
		onClose: () => void
		onApplied: (track: Track) => void
	}

	let { open, track, onClose, onApplied }: Props = $props()

	let applying = $state(false)

	/** Provider ids are proper nouns: capitalise locally, never translate. */
	function capitalize(value: string): string {
		return value.charAt(0).toUpperCase() + value.slice(1)
	}

	// Search once when the modal opens (or when the track changes while open).
	// Keyed on `open`/`track`, so it does not re-run on every render.
	$effect(() => {
		if (open) {
			void taggerStore.search(track)
		}
	})

	const hasCandidates = $derived($taggerStore.candidates.length > 0)

	function handleClose() {
		taggerStore.reset()
		onClose()
	}

	function handleSelect(candidate: ScoredTagCandidate) {
		taggerStore.select(candidate)
		// Bandcamp only returns label, release date and duration through `extend`,
		// so enrich immediately and let the row show the detail while it loads.
		void taggerStore.extendSelected()
	}

	async function handleApply() {
		if (applying || $taggerStore.selected === null) return
		applying = true
		try {
			const updated = await taggerStore.apply(track)
			if (updated) onApplied(updated)
		} finally {
			applying = false
		}
	}
</script>

<Modal {open} title={$translate('tagger.title')} size="xl" onClose={handleClose}>
	<div class="space-y-4">
		<!-- Track context -->
		<div class="rounded-md bg-surface-2 p-3">
			<Text variant="body-2" truncate>{track.title || $translate('common.untitled')}</Text>
			<Text color="secondary" truncate>{track.artist || $translate('common.unknownArtist')}</Text>
		</div>

		{#if $taggerStore.loading}
			<div class="flex items-center gap-2 text-sm text-text-secondary">
				<Spinner class="h-4 w-4" />
				<span>{$translate('tagger.searching')}</span>
			</div>
		{:else}
			<p class="text-sm text-text-secondary">{$translate('tagger.selectHint')}</p>

			<!-- Partial failures must never be silent -->
			{#if $taggerStore.errors.length > 0}
				<div role="alert" class="space-y-2 rounded-md border border-warning/20 bg-warning/10 p-3">
					<div class="flex items-center gap-2 text-warning">
						<Icon name="warning" class="h-4 w-4 shrink-0" />
						<Text as="span" weight="medium" color="warning">{$translate('tagger.providerErrors')}</Text>
					</div>
					<ul class="list-inside list-disc space-y-0.5 text-sm text-text-secondary">
						{#each $taggerStore.errors as providerError (providerError.provider)}
							<li>{providerError.provider}: {providerError.error}</li>
						{/each}
					</ul>
				</div>
			{/if}

			{#if hasCandidates}
				<div class="overflow-hidden rounded-md border border-stroke bg-surface-0">
					{#each $taggerStore.candidates as candidate, index (index)}
						{@const isSelected = $taggerStore.selected === candidate}
						{@const isExtending = isSelected && $taggerStore.extending}
						{@const shown = isSelected && $taggerStore.extended !== null ? $taggerStore.extended : candidate}
						<button
							type="button"
							aria-pressed={isSelected}
							onclick={() => handleSelect(candidate)}
							class="flex w-full items-center gap-3 border-b border-stroke-subtle px-3 py-2.5 text-left transition-colors last:border-b-0 {isSelected
								? 'bg-brand-muted'
								: 'hover:bg-surface-2/50'}"
						>
							<AlbumArt artworkPath={null} artworkUrl={shown.artwork_url} size="xs" />
							<div class="flex min-w-0 flex-1 flex-col gap-1">
								<div class="flex items-center justify-between gap-2">
									<div class="flex min-w-0 items-center gap-2">
										<span class={providerCapsuleClass(candidate.provider)}>{capitalize(candidate.provider)}</span>
										{#if shown.version}
											<span class="rounded-full bg-purple-500/15 px-2 py-0.5 text-[11px] font-medium text-purple-500">
												{shown.version}
											</span>
										{/if}
									</div>
									<span
										class="shrink-0 rounded px-1.5 py-0.5 text-xs font-medium {scoreClass(candidate.similarity_score)}"
										title={$translate('tagger.score')}
									>
										{Math.round(candidate.similarity_score * 100)}%
									</span>
								</div>
								<div class="flex min-w-0 items-baseline gap-2">
									<Text as="span" weight="medium" truncate>{shown.title || $translate('common.untitled')}</Text>
								</div>
								<Text as="span" variant="caption" truncate>
									{shown.artists.length > 0 ? shown.artists.join(', ') : $translate('common.unknownArtist')}
								</Text>
								<div class="flex flex-wrap gap-x-3 gap-y-0.5 text-xs text-text-tertiary">
									{#if shown.album}<span class="truncate">{shown.album}</span>{/if}
									{#if shown.label}<span class="truncate">{shown.label}</span>{/if}
									{#if shown.bpm !== null}
										<span>{$translate('modals.trackMetadata.fields.bpm')} {shown.bpm}</span>
									{/if}
									{#if shown.key}<span>{shown.key}</span>{/if}
									{#if shown.duration_ms !== null}<span>{formatDuration(shown.duration_ms)}</span>{/if}
								</div>
								{#if isExtending}
									<span class="flex items-center gap-1.5 text-xs text-text-tertiary">
										<Spinner class="h-3 w-3" />
										{$translate('tagger.enriching')}
									</span>
								{/if}
							</div>
						</button>
					{/each}
				</div>
			{:else}
				<div class="rounded-md border border-stroke bg-surface-0 p-6 text-center">
					<Text color="secondary">{$translate('tagger.noResults')}</Text>
				</div>
			{/if}
		{/if}
	</div>

	{#snippet footer()}
		<Button variant="ghost" onclick={handleClose} disabled={applying}>{$translate('common.cancel')}</Button>
		<Button variant="primary" onclick={handleApply} disabled={$taggerStore.selected === null || applying}>
			{applying ? $translate('tagger.applying') : $translate('tagger.apply')}
		</Button>
	{/snippet}
</Modal>
