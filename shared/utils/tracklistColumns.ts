import type { TracklistColumnId, TracklistColumnPref } from '../types'

export interface TracklistColumnDefinition {
	id: TracklistColumnId
	labelKey: string
	width: string
	sortable: boolean
	locked?: boolean
	defaultVisible: boolean
}

export const TRACKLIST_COLUMN_DEFINITIONS: TracklistColumnDefinition[] = [
	{ id: 'color', labelKey: '', width: '24px', sortable: true, defaultVisible: true },
	{ id: 'artwork', labelKey: '', width: '40px', sortable: false, defaultVisible: true },
	{
		id: 'title',
		labelKey: 'library.columns.title',
		width: '1fr',
		sortable: true,
		locked: true,
		defaultVisible: true,
	},
	{ id: 'artist', labelKey: 'library.columns.artist', width: '1fr', sortable: true, defaultVisible: true },
	{ id: 'album', labelKey: 'library.columns.album', width: '1fr', sortable: true, defaultVisible: false },
	{ id: 'genre', labelKey: 'library.columns.genre', width: '1fr', sortable: true, defaultVisible: false },
	{ id: 'label', labelKey: 'library.columns.label', width: '1fr', sortable: true, defaultVisible: false },
	{ id: 'origin', labelKey: 'library.columns.origin', width: '1fr', sortable: true, defaultVisible: false },
	{ id: 'bpm', labelKey: 'library.columns.bpm', width: '80px', sortable: true, defaultVisible: true },
	{ id: 'key', labelKey: 'library.columns.key', width: '60px', sortable: true, defaultVisible: true },
	{ id: 'duration_ms', labelKey: 'library.columns.time', width: '80px', sortable: true, defaultVisible: true },
	{ id: 'bitrate', labelKey: 'library.columns.bitrate', width: '90px', sortable: true, defaultVisible: false },
	{ id: 'year', labelKey: 'library.columns.year', width: '56px', sortable: true, defaultVisible: false },
	{ id: 'date_added', labelKey: 'library.columns.dateAdded', width: '110px', sortable: true, defaultVisible: false },
	{ id: 'provider', labelKey: 'library.columns.provider', width: '100px', sortable: true, defaultVisible: true },
	{ id: 'tags', labelKey: 'library.columns.tags', width: '1fr', sortable: false, defaultVisible: true },
	{ id: 'rating', labelKey: 'library.columns.rating', width: '72px', sortable: true, defaultVisible: true},
]

const definitionsById = new Map(TRACKLIST_COLUMN_DEFINITIONS.map((definition) => [definition.id, definition]))
const canonicalPosition = new Map(TRACKLIST_COLUMN_DEFINITIONS.map(({ id }, index) => [id, index]))

export function defaultTracklistColumns(): TracklistColumnPref[] {
	return TRACKLIST_COLUMN_DEFINITIONS.map(({ id, defaultVisible }) => ({ id, visible: defaultVisible }))
}

export function normalizeTracklistColumns(prefs: TracklistColumnPref[] | null | undefined): TracklistColumnPref[] {
	const normalized: TracklistColumnPref[] = []
	const seen = new Set<TracklistColumnId>()

	for (const pref of prefs ?? []) {
		if (!definitionsById.has(pref.id) || seen.has(pref.id)) continue
		normalized.push({ id: pref.id, visible: pref.visible })
		seen.add(pref.id)
	}

	for (const { id, defaultVisible } of TRACKLIST_COLUMN_DEFINITIONS) {
		if (seen.has(id)) continue
		const position = canonicalPosition.get(id)!
		let predecessor: { id: TracklistColumnId; position: number } | undefined
		let successor: { id: TracklistColumnId; position: number } | undefined

		for (const pref of normalized) {
			const prefPosition = canonicalPosition.get(pref.id) ?? 0
			if (prefPosition < position && (!predecessor || prefPosition > predecessor.position)) {
				predecessor = { id: pref.id, position: prefPosition }
			}
			if (prefPosition > position && (!successor || prefPosition < successor.position)) {
				successor = { id: pref.id, position: prefPosition }
			}
		}

		const predecessorIndex = predecessor ? normalized.findIndex((pref) => pref.id === predecessor.id) : -1
		const successorIndex = successor ? normalized.findIndex((pref) => pref.id === successor.id) : -1
		let insertionIndex = 0

		if (predecessor && successor && predecessorIndex !== -1 && successorIndex !== -1) {
			if (predecessorIndex < successorIndex) {
				insertionIndex = predecessorIndex + 1
			} else {
				const predecessorDistance = position - predecessor.position
				const successorDistance = successor.position - position
				insertionIndex = predecessorDistance <= successorDistance ? predecessorIndex + 1 : successorIndex
			}
		} else if (predecessorIndex !== -1) {
			insertionIndex = predecessorIndex + 1
		} else if (successorIndex !== -1) {
			insertionIndex = successorIndex
		}

		normalized.splice(insertionIndex, 0, { id, visible: defaultVisible })
		seen.add(id)
	}

	const title = normalized.find((pref) => pref.id === 'title')
	if (title) title.visible = true

	return normalized
}

export function visibleTracklistColumns(prefs: TracklistColumnPref[] | null | undefined): TracklistColumnDefinition[] {
	const normalized = normalizeTracklistColumns(prefs)
	return normalized.filter(({ visible }) => visible).map(({ id }) => definitionsById.get(id)!)
}

export function tracklistGridTemplate(prefs: TracklistColumnPref[] | null | undefined): string {
	return visibleTracklistColumns(prefs)
		.map(({ width }) => width)
		.join(' ')
}

export type TracklistMovePlacement = 'before' | 'after'

/**
 * Moves a column while preserving the relative order of all other entries, including hidden columns.
 * The header caller decides explicit placement against visible order from the pointer's half of the hovered
 * target cell; hidden entries keep their relative order and never move on their own. The derived placement
 * exists only to preserve the meaning of older call sites that omit the placement argument.
 */
export function moveTracklistColumn(
	prefs: TracklistColumnPref[],
	draggedId: TracklistColumnId,
	targetId: TracklistColumnId,
	placement?: TracklistMovePlacement
): TracklistColumnPref[] {
	if (draggedId === targetId) return prefs

	const draggedIndex = prefs.findIndex(({ id }) => id === draggedId)
	const targetIndex = prefs.findIndex(({ id }) => id === targetId)
	if (draggedIndex === -1 || targetIndex === -1 || prefs.filter(({ visible }) => visible).length <= 1) return prefs

	const resolvedPlacement = placement ?? (draggedIndex < targetIndex ? 'after' : 'before')
	const reordered = [...prefs]
	const [dragged] = reordered.splice(draggedIndex, 1)
	const targetIndexAfterRemoval = reordered.findIndex(({ id }) => id === targetId)
	const insertionIndex = targetIndexAfterRemoval + (resolvedPlacement === 'after' ? 1 : 0)
	reordered.splice(insertionIndex, 0, dragged)
	return reordered
}

export function toggleTracklistColumn(prefs: TracklistColumnPref[], id: TracklistColumnId): TracklistColumnPref[] {
	const definition = definitionsById.get(id)
	if (!definition || definition.locked) return prefs

	return prefs.map((pref) => (pref.id === id ? { ...pref, visible: !pref.visible } : pref))
}

export function getTrackOriginFolder(filePath: string): string {
	const hasTrailingSeparator = /[/\\]$/.test(filePath)
	const parts = filePath.split(/[/\\]/).filter(Boolean)
	if (parts.length < 2) return ''
	return parts[hasTrailingSeparator ? parts.length - 1 : parts.length - 2]
}
