<script lang="ts">
	import { get } from 'svelte/store'
	import { SvelteMap } from 'svelte/reactivity'
	import { openUrl } from '@tauri-apps/plugin-opener'
	import type { BeatportRecommendation } from '$shared/types'
	import { findBeatportSimilarTracks } from '$shared/api/beatport'
	import { swapBeatportImageSize } from '$shared/utils/beatport'
	import { toastStore } from '$shared/stores/toast'
	import { translate } from '$shared/i18n'
	import Modal from '$lib/components/common/Modal.svelte'
	import Text from '$lib/components/common/Text.svelte'
	import Icon from '$lib/components/common/Icon.svelte'
	import Button from '$lib/components/common/Button.svelte'
	import IconButton from '$lib/components/common/IconButton.svelte'
	import Tooltip from '$lib/components/common/Tooltip.svelte'

	type Props = {
		open: boolean
		/** Beatport numeric track id of the source track. */
		trackId: number
		onClose: () => void
	}

	let { open, trackId, onClose }: Props = $props()

	let recommendations = $state<BeatportRecommendation[]>([])
	let loading = $state(true)

	// Playback: at most one <audio> plays. The transport walks the rows that carry
	// a sample; playlist mode (Play all) keeps advancing as rows end, and stops at
	// the end of the list.
	let playingTrackId = $state<number | null>(null)
	let isPlaying = $state(false)
	let playlistMode = $state(false)
	const audioEls = new SvelteMap<number, HTMLAudioElement>()

	// The playlist is the subset of rows with a sample, in list order.
	const playable = $derived(recommendations.filter((r) => r.sample_url))
	const playingIndex = $derived(playable.findIndex((r) => r.track_id === playingTrackId))

	// Fetch on every open: the host mounts this component fresh per modal state, so
	// reopening re-queries Beatport. Results are deliberately never cached.
	$effect(() => {
		if (!open) return
		loading = true
		findBeatportSimilarTracks(trackId)
			.then((result) => {
				recommendations = result
				loading = false
			})
			.catch((error: unknown) => {
				// CrateError arrives as a plain string — mirror Harmony: notify, then close.
				toastStore.error(get(translate)('modals.beatportRecommendations.failed', { values: { error: String(error) } }))
				onClose()
			})
	})

	// Closing (or unmounting) pauses any live sample and resets playback state.
	$effect(() => {
		return () => {
			playlistMode = false
			isPlaying = false
			for (const el of audioEls.values()) {
				el.pause()
			}
		}
	})

	/** Registers a row's audio element so the transport can reach it by track id. */
	function registerAudio(node: HTMLAudioElement, id: number) {
		audioEls.set(id, node)
		return {
			destroy() {
				audioEls.delete(id)
			},
		}
	}

	function stopPlayback() {
		isPlaying = false
		playlistMode = false
	}

	/** Play the playable row at `index`; a failed play skips ahead (Harmony parity). */
	async function playAt(index: number): Promise<void> {
		if (index >= playable.length) {
			stopPlayback()
			return
		}
		const target = playable[index]
		const el = audioEls.get(target.track_id)
		if (!el) {
			await playAt(index + 1)
			return
		}
		for (const [otherId, other] of audioEls) {
			if (otherId !== target.track_id) other.pause()
		}
		playingTrackId = target.track_id
		try {
			await el.play()
			isPlaying = true
		} catch {
			isPlaying = false
			if (playlistMode) await playAt(index + 1)
		}
	}

	function togglePlayAll() {
		if (isPlaying) {
			if (playingTrackId !== null) audioEls.get(playingTrackId)?.pause()
			return
		}
		if (playable.length === 0) return
		playlistMode = true
		void playAt(playingIndex >= 0 ? playingIndex : 0)
	}

	function playPrev() {
		if (playingIndex <= 0) return
		void playAt(playingIndex - 1)
	}

	function playNext() {
		if (playingIndex < 0) return
		void playAt(playingIndex + 1)
	}

	/** The user pressed play on a row's native controls: enforce one-audio-at-a-time. */
	function handleNativePlay(id: number) {
		for (const [otherId, other] of audioEls) {
			if (otherId !== id) other.pause()
		}
		playingTrackId = id
		isPlaying = true
	}

	function handleNativePause(id: number) {
		if (id === playingTrackId) isPlaying = false
	}

	function handleEnded(id: number) {
		if (id !== playingTrackId) return
		isPlaying = false
		if (playlistMode) void playAt(playingIndex + 1)
	}

	function artistNames(rec: BeatportRecommendation): string {
		return rec.artists
			.map((a) => a.name)
			.filter((name): name is string => name !== null)
			.join(', ')
	}

	// `"2025-10-17T00:00:00"` has no timezone marker, so read the literal
	// YYYY-MM-DD prefix instead of parsing with new Date().
	function formatReleaseDate(value: string | null): string {
		const match = value?.match(/^(\d{4})-(\d{2})-(\d{2})/)
		if (!match) return '—'
		const [, year, month, day] = match
		return `${day}/${month}/${year}`
	}

	function openInBeatport(rec: BeatportRecommendation) {
		// The slug-agnostic /track/t/{id} redirect works for any track id.
		void openUrl(`https://www.beatport.com/track/t/${rec.track_id}`).catch(() => {})
	}

	const DASH = '—'
</script>

<Modal {open} size="3xl" flush {onClose}>
	<div class="flex h-[85vh] flex-col">
		<!-- Header: title left, transport centered, close right -->
		<header class="flex shrink-0 items-center gap-4 border-b border-stroke-subtle px-4 py-3">
			<div class="min-w-0 flex-1">
				<Text variant="header-1">{$translate('modals.beatportRecommendations.title')}</Text>
			</div>
			{#if !loading && playable.length > 0}
				<div class="flex shrink-0 items-center gap-1">
					<Tooltip text={$translate('modals.beatportRecommendations.previousTrack')} position="bottom">
						<IconButton icon="skip-back" fill size="md" disabled={playingIndex <= 0} onclick={playPrev} />
					</Tooltip>
					<Tooltip
						text={isPlaying
							? $translate('modals.beatportRecommendations.pausePlaylist')
							: $translate('modals.beatportRecommendations.playAllSamples')}
						position="bottom"
					>
						<IconButton
							icon={isPlaying ? 'pause' : 'play'}
							fill
							size="lg"
							class="text-brand-primary"
							onclick={togglePlayAll}
						/>
					</Tooltip>
					<Tooltip text={$translate('modals.beatportRecommendations.nextTrack')} position="bottom">
						<IconButton
							icon="skip-forward"
							fill
							size="md"
							disabled={playingIndex < 0 || playingIndex >= playable.length - 1}
							onclick={playNext}
						/>
					</Tooltip>
				</div>
			{/if}
			<div class="flex flex-1 justify-end">
				<Tooltip text={$translate('common.close')} position="bottom">
					<IconButton icon="x" onclick={onClose} />
				</Tooltip>
			</div>
		</header>

		<!-- Body: loading / empty / recommendation rows -->
		<div class="min-h-0 flex-1 overflow-y-auto px-4 py-4">
			{#if loading}
				<div class="flex h-full flex-col items-center justify-center gap-3">
					<Icon name="loader" class="h-8 w-8 animate-spin text-text-tertiary" />
					<Text color="secondary">{$translate('modals.beatportRecommendations.finding')}</Text>
				</div>
			{:else if recommendations.length === 0}
				<div class="flex h-full flex-col items-center justify-center gap-3">
					<Icon name="music-note" class="h-12 w-12 text-text-tertiary/50" />
					<Text color="tertiary">{$translate('modals.beatportRecommendations.empty')}</Text>
				</div>
			{:else}
				<div class="flex flex-col gap-3">
					{#each recommendations as rec (rec.track_id)}
						<article
							class="flex flex-wrap items-center gap-4 rounded-lg border border-stroke p-3 {rec.track_id ===
							playingTrackId
								? 'bg-brand-primary-10'
								: 'bg-surface-2 hover:brightness-110'}"
						>
							<!-- Artwork: 800x800 pre-resolved URL swapped to 80x80 for the 40px thumb -->
							{#if swapBeatportImageSize(rec.release?.image_url, '80x80')}
								<img
									src={swapBeatportImageSize(rec.release?.image_url, '80x80')}
									alt={rec.release?.name ?? rec.track_name}
									loading="lazy"
									class="h-10 w-10 shrink-0 rounded-sm object-cover"
								/>
							{:else}
								<div
									class="flex h-10 w-10 shrink-0 items-center justify-center rounded-sm bg-surface-1 text-text-tertiary/50"
								>
									<Icon name="music-note" class="h-5 w-5" />
								</div>
							{/if}

							<!-- Title + mix, artists -->
							<div class="flex min-w-[200px] grow basis-[350px] flex-col gap-0.5 overflow-hidden">
								<span class="truncate text-sm font-semibold text-text-primary">
									{rec.track_name}{#if rec.mix_name && rec.mix_name !== 'Original Mix'}
										<span class="font-normal text-text-secondary"> ({rec.mix_name})</span>
									{/if}
								</span>
								<span class="truncate text-xs text-text-secondary">{artistNames(rec)}</span>
							</div>

							<!-- Sample player, only when a preview exists -->
							{#if rec.sample_url}
								<audio
									class="h-8 min-w-[300px] grow basis-[320px]"
									controls
									preload="none"
									use:registerAudio={rec.track_id}
									onplay={() => handleNativePlay(rec.track_id)}
									onpause={() => handleNativePause(rec.track_id)}
									onended={() => handleEnded(rec.track_id)}
								>
									<source src={rec.sample_url} type="audio/mpeg" />
								</audio>
							{/if}

							{@render statColumn(
								$translate('modals.beatportRecommendations.genre'),
								rec.genre?.name ?? DASH,
								'w-[120px]'
							)}
							{@render statColumn(
								$translate('modals.beatportRecommendations.label'),
								rec.label?.name ?? DASH,
								'w-[180px]'
							)}
							{@render statColumn(
								$translate('modals.beatportRecommendations.bpm'),
								rec.bpm != null ? String(rec.bpm) : DASH,
								'w-[120px]'
							)}
							{@render statColumn(
								$translate('modals.beatportRecommendations.releaseDate'),
								formatReleaseDate(rec.release?.release_date ?? null),
								'w-[120px]'
							)}

							<Button variant="ghost" size="sm" onclick={() => openInBeatport(rec)}>
								<Icon name="external-link" class="h-3.5 w-3.5" />
								{$translate('modals.beatportRecommendations.openInBeatport')}
							</Button>
						</article>
					{/each}
				</div>
			{/if}
		</div>
	</div>
</Modal>

{#snippet statColumn(label: string, value: string, width: string)}
	<div class="flex {width} shrink-0 flex-col items-center gap-0.5">
		<Text variant="header-4">{label}</Text>
		<span class="max-w-full truncate text-center text-sm text-text-primary">{value}</span>
	</div>
{/snippet}
