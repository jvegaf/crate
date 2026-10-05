<script lang="ts">
	import type { Track, TrackColor, Playlist, ContextMenuItem } from '$shared/types'
	import { TRACK_COLORS } from '$shared/types'
	import ContextMenu from '$lib/components/common/ContextMenu.svelte'
	import { missingTrackIds } from '$lib/stores'
	import { buildPlaylistMenuItems } from '$shared/stores/playlists'
	import { buildTagMenuItems, commonTagIds, tagsStore } from '$shared/stores/tags'
	import { getStoreName, joinMenuGroups } from '$shared/utils'
	import { extractBeatportTrackId } from '$shared/utils/beatport'
	import { openUrl } from '@tauri-apps/plugin-opener'
	import { translate } from '$shared/i18n'
	import { get } from 'svelte/store'

	type Props = {
		open: boolean
		x: number
		y: number
		selectedTracks: Track[]
		playlists: Playlist[]
		currentPlaylistId: string | null
		isAnalyzing?: boolean
		onClose: () => void
		onClosed?: () => void
		onRevealInExplorer: () => void
		onAddToPlaylist: (playlistId: string) => void
		onCreatePlaylistWithTracks?: (tracks: Track[]) => void
		onRemoveFromPlaylist: () => void
		onRemoveFromLibrary: () => void
		onRelocate?: (track: Track) => void
		onSetColor?: (color: TrackColor | null) => void
		onAnalyze?: () => void
		onToggleTag?: (tagId: string, assigned: boolean) => void
		onEditMetadata: (track: Track) => void
		onFindTags?: (tracks: Track[]) => void
		onShowBeatportRecommendations?: (track: Track) => void
	}

	let {
		open,
		x,
		y,
		selectedTracks,
		playlists,
		currentPlaylistId,
		isAnalyzing = false,
		onClose,
		onClosed,
		onRevealInExplorer,
		onAddToPlaylist,
		onCreatePlaylistWithTracks,
		onRemoveFromPlaylist,
		onRemoveFromLibrary,
		onRelocate,
		onSetColor,
		onAnalyze,
		onToggleTag,
		onEditMetadata,
		onFindTags,
		onShowBeatportRecommendations,
	}: Props = $props()

	// Platform-specific label for "View in Finder/Explorer"
	const revealLabel = $derived.by(() => {
		const ua = navigator.userAgent
		if (ua.includes('Mac')) return get(translate)('contextMenu.viewInFinder')
		if (ua.includes('Windows')) return get(translate)('contextMenu.viewInExplorer')
		return get(translate)('contextMenu.viewInFileManager')
	})

	const single = $derived(selectedTracks.length === 1)
	const hasMissingTrack = $derived(single && $missingTrackIds.has(selectedTracks[0].id))

	// A color only reads as current when the whole selection shares it.
	const currentColor = $derived.by(() => {
		if (selectedTracks.length === 0) return null
		const firstColor = selectedTracks[0].color
		return selectedTracks.every((t) => t.color === firstColor) ? firstColor : null
	})

	const assignedTagIds = $derived(commonTagIds(selectedTracks))
	// Recommendations only make sense for a single track with a Beatport track-page URL.
	const beatportTrackId = $derived(selectedTracks.length === 1 ? extractBeatportTrackId(selectedTracks[0].url) : null)
	const currentPlaylist = $derived(currentPlaylistId ? playlists.find((p) => p.id === currentPlaylistId) : null)

	// Groups follow the shared convention (.claude/docs/CONTEXT_MENUS.md):
	// act → organize → navigate & share → destructive, a divider between non-empty groups.
	const menuItems = $derived.by<ContextMenuItem[]>(() => {
		const groups: ContextMenuItem[][] = []

		const act: ContextMenuItem[] = []
		if (onAnalyze) {
			act.push({
				id: 'analyze',
				label: get(translate)('contextMenu.analyze'),
				icon: 'activity',
				disabled: isAnalyzing,
				action: onAnalyze,
			})
		}
		if (hasMissingTrack && onRelocate) {
			act.push({
				id: 'relocate',
				label: get(translate)('contextMenu.relocate'),
				icon: 'folder',
				disabled: isAnalyzing,
				action: () => onRelocate(selectedTracks[0]),
			})
		}
		// "Find track tags..." - ranked provider shortlist. One selection opens the
		// single-track modal; several open the batch layout with one row each.
		if (onFindTags) {
			act.push({
				id: 'findTags',
				label: get(translate)('contextMenu.findTrackTags'),
				icon: 'tag',
				action: () => onFindTags(selectedTracks),
			})
		}
		// Single-track metadata editor targets the track captured when this menu opened.
		if (single) {
			act.push({
				id: 'edit-metadata',
				label: get(translate)('contextMenu.editMetadata'),
				icon: 'edit',
				action: () => onEditMetadata(selectedTracks[0]),
			})
			// Beatport recommendations - single track whose URL is a Beatport track page.
			if (onShowBeatportRecommendations && beatportTrackId !== null) {
				const track = selectedTracks[0]
				act.push({
					id: 'beatport-recommendations',
					label: get(translate)('contextMenu.beatportRecommendations'),
					icon: 'music-note',
					action: () => onShowBeatportRecommendations(track),
				})
			}
		}
		groups.push(act)

		const organize: ContextMenuItem[] = []
		const playlistItems = buildPlaylistMenuItems(
			playlists.filter((p) => p.context === 'library'),
			(playlistId) => () => onAddToPlaylist(playlistId)
		)
		const playlistSubmenu: ContextMenuItem[] = []
		if (onCreatePlaylistWithTracks) {
			playlistSubmenu.push({
				id: 'new-playlist',
				label: get(translate)('contextMenu.newPlaylist'),
				icon: 'plus',
				action: () => onCreatePlaylistWithTracks(selectedTracks),
			})
		}
		if (playlistItems.length > 0) {
			if (playlistSubmenu.length > 0) {
				playlistSubmenu.push({ id: 'new-playlist-divider', label: '', divider: true })
			}
			playlistSubmenu.push(...playlistItems)
		}
		organize.push({
			id: 'add-to-playlist',
			label: get(translate)('contextMenu.addToPlaylist'),
			icon: 'list-plus',
			...(playlistSubmenu.length > 0 ? { submenu: playlistSubmenu } : { disabled: true }),
		})
		if (onToggleTag) {
			const tagItems = buildTagMenuItems($tagsStore.categories, assignedTagIds, onToggleTag)
			organize.push({
				id: 'tags',
				label: get(translate)('nav.tags'),
				icon: 'tag',
				...(tagItems.length > 0 ? { submenu: tagItems } : { disabled: true }),
			})
		}
		if (onSetColor) {
			const colorItems: ContextMenuItem[] = TRACK_COLORS.map((color) => ({
				id: `color-${color.id}`,
				label: get(translate)(`colors.${color.id}`),
				colorDot: color.hex,
				selected: currentColor === color.id,
				action: () => onSetColor(color.id),
			}))
			colorItems.push({ id: 'color-divider', label: '', divider: true })
			colorItems.push({
				id: 'remove-color',
				label: get(translate)('contextMenu.removeColor'),
				icon: 'minus-circle',
				variant: 'danger',
				action: () => onSetColor(null),
			})
			organize.push({
				id: 'set-color',
				label: get(translate)('contextMenu.setColor'),
				icon: 'palette',
				submenu: colorItems,
			})
		}
		groups.push(organize)

		const navigate: ContextMenuItem[] = []
		if (single) {
			navigate.push({
				id: 'reveal-in-explorer',
				label: revealLabel,
				icon: 'folder-open',
				action: onRevealInExplorer,
			})
			const storeUrl = selectedTracks[0].url?.trim() ?? ''
			if (storeUrl) {
				const store = getStoreName(storeUrl)
				navigate.push({
					id: 'view-in-store',
					label: store
						? get(translate)('contextMenu.viewOnStore', { values: { store } })
						: get(translate)('contextMenu.viewInStore'),
					icon: 'external-link',
					action: () => openUrl(storeUrl).catch(() => {}),
				})
			}
		}
		groups.push(navigate)

		const destructive: ContextMenuItem[] = []
		if (currentPlaylistId && !currentPlaylist?.is_smart) {
			destructive.push({
				id: 'remove-from-playlist',
				label: get(translate)('contextMenu.removeFromPlaylist'),
				icon: 'list-minus',
				variant: 'danger',
				disabled: isAnalyzing,
				action: onRemoveFromPlaylist,
			})
		}
		destructive.push({
			id: 'remove-from-library',
			label: get(translate)('contextMenu.removeFromLibrary'),
			icon: 'trash',
			variant: 'danger',
			disabled: isAnalyzing,
			action: onRemoveFromLibrary,
		})
		groups.push(destructive)

		return joinMenuGroups(groups)
	})
</script>

<ContextMenu {open} {x} {y} items={menuItems} {onClose} {onClosed} />
