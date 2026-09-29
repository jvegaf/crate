<script lang="ts">
	import type { SortConfig, TrackSortField, TracklistColumnId } from '$shared/types'
	import { getNextSortConfig } from '$shared/utils'
	import { DRAG_THRESHOLD, getDistance } from '$shared/utils/drag'
	import {
		moveTracklistColumn,
		tracklistGridTemplate,
		visibleTracklistColumns,
		type TracklistMovePlacement,
	} from '$shared/utils/tracklistColumns'
	import { translate } from '$shared/i18n'
	import Icon from '$lib/components/common/Icon.svelte'
	import TrackListColumnsMenu from './TrackListColumnsMenu.svelte'
	import { settingsStore, tracklistColumns } from '$lib/stores'
	import { fade } from 'svelte/transition'

	type Props = {
		sortConfig: SortConfig
		onSort?: (config: SortConfig) => void
	}

	type HeaderCellRect = {
		id: TracklistColumnId
		left: number
		right: number
		top: number
		bottom: number
	}

	let { sortConfig, onSort }: Props = $props()

	function handleSort(field: TrackSortField) {
		const newConfig = getNextSortConfig(sortConfig, field)
		onSort?.(newConfig)
	}

	const columns = $derived(visibleTracklistColumns($tracklistColumns))
	const gridTemplate = $derived(tracklistGridTemplate($tracklistColumns))
	let headerElement: HTMLDivElement | undefined = $state()
	let columnMenu = $state({ open: false, x: 0, y: 0 })
	let pointerStartPosition: { x: number; y: number } | null = $state(null)
	let activePointerId: number | null = $state(null)
	let pointerStartElement: HTMLElement | null = $state(null)
	let draggedColumnId: TracklistColumnId | null = $state(null)
	let dragStarted = $state(false)
	let dragCellRects: HeaderCellRect[] = $state([])
	let targetColumnId: TracklistColumnId | null = $state(null)
	let targetPlacement: TracklistMovePlacement | null = $state(null)
	let suppressNextSortClick = $state(false)
	let suppressClickTimer: ReturnType<typeof setTimeout> | undefined
	let measuredRectSignature = ''

	// Cached cell geometry is only valid for the column set it was measured from. Track that set so a
	// mid-drag change can never leave hit-testing or the insertion marker on stale coordinates.
	const columnSignature = $derived(columns.map(({ id }) => id).join(','))

	$effect(() => () => {
		if (suppressClickTimer) clearTimeout(suppressClickTimer)
		if (headerElement && activePointerId !== null) resetDragState()
	})

	// A pointer released while the window is blurred never delivers pointerup, which would leave the
	// drag armed and block every later drag.
	function handleWindowBlur() {
		if (activePointerId !== null) resetDragState()
	}

	function measureCellRects(root: HTMLElement): HeaderCellRect[] {
		return Array.from(root.querySelectorAll<HTMLElement>('[data-column-id]')).map((cell) => {
			const rect = cell.getBoundingClientRect()
			return {
				id: cell.dataset.columnId as TracklistColumnId,
				left: rect.left,
				right: rect.right,
				top: rect.top,
				bottom: rect.bottom,
			}
		})
	}

	function handlePointerDown(e: PointerEvent) {
		if (e.button !== 0 || !e.isPrimary || activePointerId !== null) return

		const columnId = (e.currentTarget as HTMLElement).dataset.columnId as TracklistColumnId | undefined
		if (!columnId) return

		pointerStartPosition = { x: e.clientX, y: e.clientY }
		activePointerId = e.pointerId
		pointerStartElement = e.currentTarget as HTMLElement
		draggedColumnId = columnId
		dragStarted = false
		dragCellRects = []
		targetColumnId = null
		targetPlacement = null
	}

	function handlePointerMove(e: PointerEvent) {
		if (activePointerId !== e.pointerId || !pointerStartPosition) return

		if (!dragStarted) {
			const distance = getDistance(pointerStartPosition.x, pointerStartPosition.y, e.clientX, e.clientY)
			if (distance < DRAG_THRESHOLD) return

			if (!headerElement || !pointerStartElement || !draggedColumnId) {
				resetDragState()
				return
			}

			measuredRectSignature = columnSignature
			dragCellRects = measureCellRects(headerElement)

			try {
				pointerStartElement.setPointerCapture(e.pointerId)
			} catch {
				resetDragState()
				return
			}

			dragStarted = true
		} else if (measuredRectSignature !== columnSignature && headerElement) {
			measuredRectSignature = columnSignature
			dragCellRects = measureCellRects(headerElement)
		}

		const hoveredCell = dragCellRects.find(
			({ left, right, top, bottom }) =>
				e.clientX >= left && e.clientX <= right && e.clientY >= top && e.clientY <= bottom
		)
		if (!hoveredCell || hoveredCell.id === draggedColumnId) {
			targetColumnId = null
			targetPlacement = null
			return
		}

		targetColumnId = hoveredCell.id
		targetPlacement = e.clientX < (hoveredCell.left + hoveredCell.right) / 2 ? 'before' : 'after'
	}

	function handlePointerUp(e: PointerEvent) {
		if (activePointerId !== e.pointerId) return

		const wasDrag = dragStarted
		const draggedId = draggedColumnId
		const targetId = targetColumnId
		const placement = targetPlacement
		resetDragState()
		if (!wasDrag) return

		// The browser dispatches the pointer-generated click in this input task; the timer clears an unused flag
		// before a later user click can arrive if releasing a drag produced no click.
		suppressNextSortClick = true
		if (suppressClickTimer) clearTimeout(suppressClickTimer)
		suppressClickTimer = setTimeout(() => {
			suppressNextSortClick = false
			suppressClickTimer = undefined
		}, 0)

		if (draggedId && targetId && placement) {
			settingsStore.setTracklistColumns(moveTracklistColumn($tracklistColumns, draggedId, targetId, placement))
		}
	}

	function handlePointerCancel(e: PointerEvent) {
		if (activePointerId === e.pointerId) resetDragState()
	}

	function handleLostPointerCapture(e: PointerEvent) {
		if (activePointerId === e.pointerId && dragStarted) resetDragState()
	}

	function resetDragState() {
		pointerStartPosition = null
		activePointerId = null
		pointerStartElement = null
		draggedColumnId = null
		dragStarted = false
		dragCellRects = []
		targetColumnId = null
		targetPlacement = null
	}

	function handleSortClickCapture(e: MouseEvent) {
		if (!suppressNextSortClick) return

		suppressNextSortClick = false
		if (suppressClickTimer) clearTimeout(suppressClickTimer)
		suppressClickTimer = undefined
		e.preventDefault()
		e.stopPropagation()
	}

	function handleContextMenu(e: MouseEvent) {
		e.preventDefault()
		e.stopPropagation()
		columnMenu = { open: true, x: e.clientX, y: e.clientY }
	}

	function stopMenuClickPropagation(e: MouseEvent) {
		e.stopPropagation()
	}
</script>

<svelte:window
	onpointermove={handlePointerMove}
	onpointerup={handlePointerUp}
	onpointercancel={handlePointerCancel}
	onblur={handleWindowBlur}
/>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
	bind:this={headerElement}
	class="sticky top-0 z-10 grid justify-items-start gap-2 border-b border-stroke bg-surface-1/50 px-3 py-2 text-xs font-medium tracking-wider text-text-tertiary uppercase backdrop-blur-sm"
	style="grid-template-columns: {gridTemplate}"
	onclickcapture={handleSortClickCapture}
	oncontextmenu={handleContextMenu}
>
	{#each columns as column (column.id)}
		<div
			class="relative w-full min-w-0 touch-none select-none {dragStarted ? 'cursor-grabbing' : 'cursor-grab'}"
			data-column-id={column.id}
			onpointerdown={handlePointerDown}
			onlostpointercapture={handleLostPointerCapture}
		>
			{#if column.sortable}
				<button
					type="button"
					class="w-full text-left transition-colors hover:text-text-secondary"
					onclick={() => handleSort(column.id as TrackSortField)}
				>
					{column.labelKey ? $translate(column.labelKey) : ''}
					{#if sortConfig.field === column.id}
						<span class="inline-block" transition:fade={{ duration: 50 }}>
							<Icon
								name="chevron-down"
								class="ml-1 inline-block h-3 w-3 align-middle transition-transform {sortConfig.direction === 'asc'
									? 'rotate-180'
									: ''}"
							/>
						</span>
					{/if}
				</button>
			{:else if column.labelKey}
				<div class="w-full text-left">
					{$translate(column.labelKey)}
				</div>
			{:else}
				<div></div>
			{/if}
			{#if dragStarted && targetColumnId === column.id && targetPlacement}
				<div
					class="pointer-events-none absolute inset-y-0 z-10 w-0.5 bg-brand-primary {targetPlacement === 'before'
						? 'left-0'
						: 'right-0'}"
				></div>
			{/if}
		</div>
	{/each}
</div>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div onclick={stopMenuClickPropagation}>
	<TrackListColumnsMenu
		open={columnMenu.open}
		x={columnMenu.x}
		y={columnMenu.y}
		onClose={() => (columnMenu.open = false)}
	/>
</div>
