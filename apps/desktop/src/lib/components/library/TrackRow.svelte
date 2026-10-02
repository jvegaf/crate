<script lang="ts">
	import { fade } from 'svelte/transition'
	import type { Track, TrackColor } from '$shared/types'
	import {
		formatDurationCompact,
		formatBpm,
		formatKey,
		formatBitrate,
		formatDate,
		getTrackDisplayName,
		getTrackDisplayArtist,
		nextRatingSelection,
	} from '$shared/utils'
	import { getTrackOriginFolder, visibleTracklistColumns, tracklistGridTemplate } from '$shared/utils/tracklistColumns'
	import { TagChip } from '$lib/components/tags'
	import Icon from '$lib/components/common/Icon.svelte'
	import { AlbumArt, AlbumArtModal, Spinner, Text, Tooltip } from '$lib/components/common'
	import {
		missingTrackIds,
		dragStore,
		isDraggingTag,
		keyNotationFormat,
		dateFormat,
		language,
		tracklistColumns,
	} from '$lib/stores'
	import { translate } from '$shared/i18n'
	import { DRAG_THRESHOLD, getDistance } from '$shared/utils/drag'
	import TrackColorCell from './TrackColorCell.svelte'

	type Props = {
		track: Track
		selected?: boolean
		playing?: boolean
		analyzing?: boolean
		dragTrackIds?: string[]
		categoryColors?: Map<string, string | null>
		categorySortOrders?: Map<string, number>
		onclick?: (e: MouseEvent) => void
		ondblclick?: (e: MouseEvent) => void
		oncontextmenu?: (e: MouseEvent) => void
		onColorChange?: (color: TrackColor | null) => void
		onRatingChange?: (rating: number) => void
		onCancelAnalysis?: () => void
	}

	let {
		track,
		selected = false,
		playing = false,
		analyzing = false,
		dragTrackIds = [],
		categoryColors,
		categorySortOrders,
		onclick,
		ondblclick,
		oncontextmenu,
		onColorChange,
		onRatingChange,
		onCancelAnalysis,
	}: Props = $props()

	let showArtworkModal = $state(false)
	let isHoveringColorCell = $state(false)
	let hoverRating = $state(0)
	let isTagDragHovered = $state(false)

	// Clear hover when tag drag ends
	$effect(() => {
		if (!$isDraggingTag) isTagDragHovered = false
	})

	// Track pointer state for drag detection
	let pointerStartPos: { x: number; y: number } | null = null
	let isDragStarted = false

	function handlePointerDown(e: PointerEvent) {
		// Only handle primary button (left click)
		if (e.button !== 0) return

		// Don't start drag on interactive elements
		const target = e.target as HTMLElement
		if (target.closest('button, [role="button"]')) return

		pointerStartPos = { x: e.clientX, y: e.clientY }
		isDragStarted = false
	}

	function handlePointerMove(e: PointerEvent) {
		if (!pointerStartPos) return

		const distance = getDistance(pointerStartPos.x, pointerStartPos.y, e.clientX, e.clientY)

		// Start drag if threshold exceeded
		if (!isDragStarted && distance >= DRAG_THRESHOLD) {
			isDragStarted = true

			// Determine which tracks to drag
			const trackIds = selected && dragTrackIds.length > 0 ? dragTrackIds : [track.id]

			// Start the drag via the store
			dragStore.startTrackDrag(trackIds, e.clientX, e.clientY)
		}
	}

	function handlePointerUp() {
		pointerStartPos = null
		isDragStarted = false
	}

	function handleArtworkClick() {
		if (track.artwork_path) {
			showArtworkModal = true
		}
	}

	const isMissing = $derived($missingTrackIds.has(track.id))
	const columns = $derived(visibleTracklistColumns($tracklistColumns))
	const gridTemplate = $derived(tracklistGridTemplate($tracklistColumns))
	const folder = $derived(getTrackOriginFolder(track.file_path))
	const ratingStars = [1, 2, 3, 4, 5]
</script>

<div
	role="row"
	tabindex="0"
	data-track-row
	data-track-id={track.id}
	class="relative grid cursor-pointer items-center gap-2 border-b border-stroke-subtle px-3 py-1 text-sm transition-colors select-none {selected
		? 'bg-brand-muted'
		: 'hover:bg-surface-2/50'} {playing ? 'text-brand-primary' : 'text-text-secondary'} {isMissing
		? 'bg-red-500/5'
		: ''} {isTagDragHovered ? 'bg-brand-primary/10 ring-1 ring-brand-primary ring-inset' : ''}"
	style="grid-template-columns: {gridTemplate}"
	{onclick}
	{ondblclick}
	{oncontextmenu}
	onpointerdown={handlePointerDown}
	onpointermove={handlePointerMove}
	onpointerup={handlePointerUp}
	onpointercancel={handlePointerUp}
	onpointerenter={() => $isDraggingTag && (isTagDragHovered = true)}
	onpointerleave={() => (isTagDragHovered = false)}
	onkeydown={(e) => e.key === 'Enter' && ondblclick?.(e as unknown as MouseEvent)}
>
	<!-- Missing file indicator -->
	{#if isMissing}
		<div class="pointer-events-none absolute inset-0 border-l-2 border-red-500/50"></div>
	{/if}
	{#each columns as column (column.id)}
		{#if column.id === 'color'}
			<!-- Color -->
			<div
				role="presentation"
				class="relative flex h-full items-center justify-center"
				onmouseenter={() => (isHoveringColorCell = true)}
				onmouseleave={() => (isHoveringColorCell = false)}
			>
				{#if analyzing}
					{#if isHoveringColorCell && onCancelAnalysis}
						<Tooltip text={$translate('contextMenu.stopAnalysis')} position="right">
							<div transition:fade={{ duration: 150 }}>
								<button
									type="button"
									class="flex h-5 w-5 cursor-pointer items-center justify-center rounded transition-colors hover:bg-red-500/20"
									onclick={(e) => {
										e.stopPropagation()
										onCancelAnalysis?.()
									}}
								>
									<Icon name="x" class="h-3 w-3 text-red-500" />
								</button>
							</div>
						</Tooltip>
					{:else}
						<div class="absolute inset-0 flex items-center justify-center" transition:fade={{ duration: 150 }}>
							<Spinner class="h-3 w-3" />
						</div>
					{/if}
				{:else}
					<div transition:fade={{ duration: 150 }}>
						<TrackColorCell color={track.color} onselect={onColorChange} />
					</div>
				{/if}
			</div>
		{:else if column.id === 'artwork'}
			<!-- Artwork -->
			<div class="flex justify-center">
				<AlbumArt
					artworkPath={track.artwork_path}
					size="xs"
					onclick={handleArtworkClick}
					class={track.artwork_path ? 'cursor-zoom-in' : ''}
				/>
			</div>
		{:else if column.id === 'title'}
			<!-- Title -->
			<div class="flex items-center truncate font-medium {playing ? 'text-brand-primary' : 'text-text-primary'}">
				{#if isMissing}
					<span class="mr-1.5 flex-shrink-0" title="File not found">
						<Icon name="warning" class="h-3.5 w-3.5 text-red-500" />
					</span>
					<!--{:else if playing}-->
					<!--	<span class="mr-1 inline-block w-4 flex-shrink-0">-->
					<!--		<Icon name="play" class="h-3 w-3 animate-pulse" fill />-->
					<!--	</span>-->
				{/if}
				<span class="truncate">{getTrackDisplayName(track)}</span>
			</div>
		{:else if column.id === 'artist'}
			<!-- Artist -->
			<div class="truncate text-text-secondary">
				{getTrackDisplayArtist(track)}
			</div>
		{:else if column.id === 'album'}
			<div class="truncate text-text-secondary">
				{track.album ?? ''}
			</div>
		{:else if column.id === 'genre'}
			<div class="truncate text-text-secondary">
				{track.genre ?? ''}
			</div>
		{:else if column.id === 'label'}
			<div class="truncate text-text-secondary">
				{track.label ?? ''}
			</div>
		{:else if column.id === 'origin'}
			<div class="truncate text-text-secondary" title={folder}>
				{folder}
			</div>
		{:else if column.id === 'bpm'}
			<!-- BPM -->
			<div class="text-text-secondary tabular-nums">
				{formatBpm(track.bpm)}
			</div>
		{:else if column.id === 'key'}
			<!-- Key -->
			<div class="flex items-center text-text-secondary">
				{formatKey(track.key, $keyNotationFormat)}
			</div>
		{:else if column.id === 'duration_ms'}
			<!-- Duration -->
			<div class="text-text-secondary tabular-nums">
				{formatDurationCompact(track.duration_ms)}
			</div>
		{:else if column.id === 'bitrate'}
			<div class="text-text-secondary tabular-nums">
				{formatBitrate(track.bitrate)}
			</div>
		{:else if column.id === 'year'}
			<div class="text-text-secondary tabular-nums">
				{track.year ?? ''}
			</div>
		{:else if column.id === 'date_added'}
			<div class="text-text-secondary tabular-nums">
				{formatDate(track.date_added, $dateFormat, $language)}
			</div>
		{:else if column.id === 'tags'}
			<!-- Tags -->
			<div class="flex h-6 items-center gap-1 overflow-hidden">
				{#each track.tags
					.toSorted((a, b) => {
						const orderA = categorySortOrders?.get(a.category_id) ?? 0
						const orderB = categorySortOrders?.get(b.category_id) ?? 0
						if (orderA !== orderB) return orderA - orderB
						return a.name.localeCompare(b.name)
					})
					.slice(0, 3) as tag (tag.id)}
					<TagChip {tag} size="sm" color={categoryColors?.get(tag.category_id)} />
				{/each}
				{#if track.tags.length > 3}
					<Text variant="caption">+{track.tags.length - 3}</Text>
				{/if}
			</div>
		{:else if column.id === 'rating'}
			<!-- Rating -->
			<div
				role="presentation"
				class="flex items-center justify-center gap-0.5 text-base leading-none"
				onpointerleave={() => (hoverRating = 0)}
			>
				{#each ratingStars as star (star)}
					<button
						type="button"
						aria-label={track.rating === star ? 'Clear rating' : `Set rating to ${star} stars`}
						class="cursor-pointer leading-none {(hoverRating || track.rating) >= star
							? 'text-warning'
							: 'text-text-tertiary/50'}"
						onpointerenter={() => (hoverRating = star)}
						onclick={(e) => {
							e.stopPropagation()
							onRatingChange?.(nextRatingSelection(track.rating, star))
						}}
						ondblclick={(e) => e.stopPropagation()}
						onkeydown={(e) => e.stopPropagation()}
					>
						★
					</button>
				{/each}
			</div>
		{/if}
	{/each}
	</div>

	<!-- Artwork -->
	<div class="flex justify-center">
		<AlbumArt
			artworkPath={track.artwork_path}
			size="xs"
			onclick={handleArtworkClick}
			class={track.artwork_path ? 'cursor-zoom-in' : ''}
		/>
	</div>

	<!-- Title -->
	<div class="flex items-center truncate font-medium {playing ? 'text-brand-primary' : 'text-text-primary'}">
		{#if isMissing}
			<span class="mr-1.5 flex-shrink-0" title={$translate('library.fileNotFound')}>
				<Icon name="warning" class="h-3.5 w-3.5 text-red-500" />
			</span>
			<!--{:else if playing}-->
			<!--	<span class="mr-1 inline-block w-4 flex-shrink-0">-->
			<!--		<Icon name="play" class="h-3 w-3 animate-pulse" fill />-->
			<!--	</span>-->
		{/if}
		<span class="truncate">{getTrackDisplayName(track)}</span>
	</div>

	<!-- Artist -->
	<div class="truncate text-text-secondary">
		{getTrackDisplayArtist(track)}
	</div>

	<!-- BPM -->
	<div class="text-text-secondary tabular-nums">
		{formatBpm(track.bpm)}
	</div>

	<!-- Key -->
	<div class="flex items-center text-text-secondary">
		{formatKey(track.key, $keyNotationFormat)}
	</div>

	<!-- Duration -->
	<div class="text-text-secondary tabular-nums">
		{formatDurationCompact(track.duration_ms)}
	</div>

	<!-- Tags -->
	<div class="flex h-6 items-center gap-1 overflow-hidden">
		{#each track.tags
			.toSorted((a, b) => {
				const orderA = categorySortOrders?.get(a.category_id) ?? 0
				const orderB = categorySortOrders?.get(b.category_id) ?? 0
				if (orderA !== orderB) return orderA - orderB
				return a.name.localeCompare(b.name)
			})
			.slice(0, 3) as tag (tag.id)}
			<TagChip {tag} size="sm" color={categoryColors?.get(tag.category_id)} />
		{/each}
		{#if track.tags.length > 3}
			<Text variant="caption">+{track.tags.length - 3}</Text>
		{/if}
	</div>
</div>

{#if showArtworkModal}
	<AlbumArtModal
		open={showArtworkModal}
		artworkPath={track.artwork_path}
		trackTitle={getTrackDisplayName(track)}
		onClose={() => (showArtworkModal = false)}
	/>
{/if}
