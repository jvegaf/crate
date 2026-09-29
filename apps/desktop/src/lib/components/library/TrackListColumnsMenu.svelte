<script lang="ts">
	import type { ContextMenuItem } from '$shared/types'
	import {
		TRACKLIST_COLUMN_DEFINITIONS,
		defaultTracklistColumns,
		toggleTracklistColumn,
	} from '$shared/utils/tracklistColumns'
	import { translate } from '$shared/i18n'
	import { settingsStore, tracklistColumns } from '$lib/stores'
	import ContextMenu from '$lib/components/common/ContextMenu.svelte'

	type Props = {
		open: boolean
		x: number
		y: number
		onClose: () => void
		onClosed?: () => void
	}

	let { open, x, y, onClose, onClosed }: Props = $props()

	const menuItems = $derived.by<ContextMenuItem[]>(() => [
		...TRACKLIST_COLUMN_DEFINITIONS.map((definition) => {
			const labelKey =
				definition.labelKey ||
				(definition.id === 'color' ? 'smartPlaylist.fields.color' : 'modals.trackMetadata.artwork')
			const visible = $tracklistColumns.find(({ id }) => id === definition.id)?.visible ?? definition.defaultVisible

			return {
				id: definition.id,
				label: $translate(labelKey),
				selected: visible,
				keepOpen: true,
				disabled: definition.locked,
				tooltip: definition.locked ? $translate('library.columnMenu.alwaysVisible') : undefined,
				action: () => settingsStore.setTracklistColumns(toggleTracklistColumn($tracklistColumns, definition.id)),
			}
		}),
		{ id: 'columns-divider', label: '', divider: true },
		{
			id: 'reset-columns',
			label: $translate('library.columnMenu.reset'),
			action: () => settingsStore.setTracklistColumns(defaultTracklistColumns()),
		},
	])
</script>

<ContextMenu {open} {x} {y} items={menuItems} {onClose} {onClosed} />
