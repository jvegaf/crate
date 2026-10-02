<script lang="ts">
	import { get } from 'svelte/store'
	import { SvelteMap } from 'svelte/reactivity'
	import { openUrl } from '@tauri-apps/plugin-opener'
	import type { BeatportRecommendation } from '$shared/types'
	import { findBeatportSimilarTracks, registerBeatportSampleStreams } from '$shared/api/beatport'
	import { swapBeatportImageSize } from '$shared/utils/beatport'
	import { isInputFocused } from '$shared/utils'
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

	// Playback runs through ONE detached Audio (the previewPlayer pattern). An in-DOM
	// `<audio controls>` takes a slow WebKitGTK resource-selection path that stalls the
	// first sound 10–27 s even when the proxy serves the full body in <1 s; a detached
	// Audio on the same URL reached `playing` in ~0.5 s. Rows render compact custom
	// controls (play/pause, seek bar, time) instead of native controls.
	let playingTrackId = $state<number | null>(null)
	let isPlaying = $state(false)
	let playlistMode = $state(false)
	/** Row whose play() was requested but has not reached the playing event yet. */
	let pendingTrackId = $state<number | null>(null)
	let positionMs = $state(0)
	let durationMs = $state(0)

	// The playlist is the subset of rows with a sample, in list order.
	const playable = $derived(recommendations.filter((r) => r.sample_url))
	const playingIndex = $derived(playable.findIndex((r) => r.track_id === playingTrackId))

	// Proxy-backed sample sources, per track id. Rows absent from this map use their direct
	// `sample_url` — the pre-proxy behavior — so the feature never hard-depends on the proxy.
	const streamSrcByTrackId = new SvelteMap<number, string>()

	// Fetch on every open: the host mounts this component fresh per modal state, so
	// reopening re-queries Beatport. Results are deliberately never cached.
	$effect(() => {
		if (!open) return
		loading = true
		streamSrcByTrackId.clear()
		findBeatportSimilarTracks(trackId)
			.then(async (result) => {
				recommendations = result
				// Register sample URLs with the localhost stream proxy BEFORE rendering rows:
				// the webview's own remote https fetch hangs on WebKitGTK/libsoup on some
				// systems, and a <source> whose src is swapped after the media element was
				// created does not reliably re-run resource selection (a load() call would be
				// needed). Awaiting registration here means every <audio> is created with its
				// final src on first render. Registration is a local IPC hash insert — the
				// loading state barely notices it; any failure keeps rows on their direct URL.
				try {
					const rows = result.filter((r) => r.sample_url)
					const proxied = await registerBeatportSampleStreams(rows.map((r) => r.sample_url as string))
					rows.forEach((row, i) => {
						const src = proxied[i] ?? row.sample_url
						if (src) streamSrcByTrackId.set(row.track_id, src)
					})
				} catch (error) {
					// Proxy registration failed: every row keeps its direct URL (pre-proxy behavior).
					console.warn('beatport sample proxy registration failed', error)
				}
				loading = false
			})
			.catch((error: unknown) => {
				// CrateError arrives as a plain string — mirror Harmony: notify, then close.
				toastStore.error(get(translate)('modals.beatportRecommendations.failed', { values: { error: String(error) } }))
				onClose()
			})
	})

	// One detached Audio instance drives every row; its src is swapped on demand.
	let audioEl: HTMLAudioElement | null = null

	function ensureAudio(): HTMLAudioElement {
		if (audioEl) return audioEl
		const el = new Audio()
		el.addEventListener('play', () => {
			isPlaying = true
			pendingTrackId = null
		})
		el.addEventListener('pause', () => (isPlaying = false))
		el.addEventListener('timeupdate', () => (positionMs = Math.round(el.currentTime * 1000)))
		el.addEventListener(
			'durationchange',
			() => (durationMs = isFinite(el.duration) ? Math.round(el.duration * 1000) : 0)
		)
		el.addEventListener('ended', handleEnded)
		el.addEventListener('error', () => {
			pendingTrackId = null
			isPlaying = false
			toastStore.error(get(translate)('modals.beatportRecommendations.playbackFailed'))
		})
		audioEl = el
		return el
	}

	// Closing (or unmounting) pauses the live sample and resets playback state. The
	// element is dropped by unmounting itself — no src juggling that would fire a
	// spurious error event.
	$effect(() => {
		return () => {
			playlistMode = false
			isPlaying = false
			pendingTrackId = null
			playingTrackId = null
			positionMs = 0
			durationMs = 0
			audioEl?.pause()
		}
	})

	function stopPlayback() {
		isPlaying = false
		playlistMode = false
		pendingTrackId = null
	}

	/** Load the given row's sample onto the shared element and start it. */
	async function playRec(rec: BeatportRecommendation): Promise<boolean> {
		const src = streamSrcByTrackId.get(rec.track_id) ?? rec.sample_url
		if (!src) return false
		const el = ensureAudio()
		playingTrackId = rec.track_id
		pendingTrackId = rec.track_id
		positionMs = 0
		durationMs = 0
		el.src = src
		try {
			await el.play()
		} catch {
			// Rejected start (autoplay policy or network): leave the UI consistent.
			pendingTrackId = null
			isPlaying = false
		}
		return true
	}

	/** Play the playable row at `index`; a sourceless row skips ahead (Harmony parity). */
	async function playAt(index: number): Promise<void> {
		if (index >= playable.length) {
			stopPlayback()
			return
		}
		const started = await playRec(playable[index])
		if (!started && playlistMode) await playAt(index + 1)
	}

	/** Row button: toggles its own sample; starting another row switches source and turns playlist off. */
	function handleRowToggle(rec: BeatportRecommendation) {
		if (audioEl && playingTrackId === rec.track_id) {
			if (isPlaying || pendingTrackId === rec.track_id) audioEl.pause()
			else void audioEl.play().catch(() => (pendingTrackId = null))
			return
		}
		playlistMode = false
		void playRec(rec)
	}

	function togglePlayAll() {
		if (isPlaying) {
			audioEl?.pause()
			return
		}
		if (playable.length === 0) return
		playlistMode = true
		if (audioEl && playingTrackId !== null) {
			// Resume the current row and keep the playlist going from here.
			void audioEl.play().catch(() => {
				isPlaying = false
				pendingTrackId = null
			})
			return
		}
		void playAt(0)
	}

	function playPrev() {
		if (playingIndex <= 0) return
		void playAt(playingIndex - 1)
	}

	function playNext() {
		if (playingIndex < 0) return
		void playAt(playingIndex + 1)
	}

	function handleEnded() {
		isPlaying = false
		if (playlistMode) void playAt(playingIndex + 1)
	}

	function seekSample(event: MouseEvent, rec: BeatportRecommendation) {
		if (!audioEl || playingTrackId !== rec.track_id || durationMs <= 0) return
		const rect = (event.currentTarget as HTMLElement).getBoundingClientRect()
		const fraction = Math.min(1, Math.max(0, (event.clientX - rect.left) / rect.width))
		audioEl.currentTime = (fraction * durationMs) / 1000
	}

	function seekTo(ms: number) {
		if (!audioEl || durationMs <= 0) return
		const clamped = Math.min(durationMs, Math.max(0, ms))
		audioEl.currentTime = clamped / 1000
		positionMs = clamped
	}

	// While this modal is open it owns the plain arrow keys: Left/Right seek the current
	// sample by 10 s (same step as the global player shortcut), Down/Up switch rows —
	// Down starts from the first row when nothing is loaded yet. The global shortcuts are
	// already suppressed for any open ModalOrchestrator modal (useAppSetup → isModalOpen),
	// so there is no double-fire with the main player's seek/volume bindings.
	$effect(() => {
		if (!open) return
		function handleKeys(e: KeyboardEvent) {
			if (e.metaKey || e.ctrlKey || e.altKey || e.shiftKey || isInputFocused()) return
			switch (e.key) {
				case 'ArrowLeft':
					if (playingTrackId !== null) {
						e.preventDefault()
						seekTo(positionMs - 10_000)
					}
					break
				case 'ArrowRight':
					if (playingTrackId !== null) {
						e.preventDefault()
						seekTo(positionMs + 10_000)
					}
					break
				case 'ArrowDown':
					e.preventDefault()
					void playAt(playingIndex < 0 ? 0 : playingIndex + 1)
					break
				case 'ArrowUp':
					if (playingIndex > 0) {
						e.preventDefault()
						void playAt(playingIndex - 1)
					}
					break
			}
		}
		window.addEventListener('keydown', handleKeys)
		return () => window.removeEventListener('keydown', handleKeys)
	})

	function fmtTime(ms: number): string {
		const total = Math.max(0, Math.round(ms / 1000))
		return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, '0')}`
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

							<!-- Compact sample player: custom controls over the shared detached
							     Audio (native `<audio controls>` in the DOM stalls ~10–27 s in
							     WebKitGTK resource selection; see the playback note in the script). -->
							{#if streamSrcByTrackId.get(rec.track_id) ?? rec.sample_url}
								{@const rowPlaying = playingTrackId === rec.track_id}
								<div class="flex min-w-[300px] grow basis-[320px] items-center gap-3">
									<IconButton
										icon={rowPlaying && (isPlaying || pendingTrackId === rec.track_id) ? 'pause' : 'play'}
										fill
										size="xl"
										onclick={() => handleRowToggle(rec)}
									/>
									<button
										type="button"
										class="h-1.5 min-w-0 grow cursor-pointer overflow-hidden rounded-full bg-surface-1 disabled:cursor-default"
										disabled={!rowPlaying}
										aria-label={$translate('modals.beatportRecommendations.seekPreview')}
										onclick={(event) => seekSample(event, rec)}
									>
										<span
											class="block h-full rounded-full bg-brand-primary"
											style="width: {rowPlaying && durationMs > 0
												? Math.min(100, (positionMs / durationMs) * 100)
												: 0}%"
										></span>
									</button>
									<span class="w-[92px] shrink-0 text-right text-xs text-text-secondary">
										{rowPlaying ? `${fmtTime(positionMs)} / ${fmtTime(durationMs)}` : fmtTime(rec.track_length_ms ?? 0)}
									</span>
								</div>
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
