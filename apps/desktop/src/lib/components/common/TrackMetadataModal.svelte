<script lang="ts" module>
	export type CrateArtworkChange = { type: 'unchanged' } | { type: 'set'; filePath: string } | { type: 'clear' }
</script>

<script lang="ts">
	import { open } from '@tauri-apps/plugin-dialog'
	import { readFile } from '@tauri-apps/plugin-fs'
	import { get } from 'svelte/store'
	import { translate } from '$shared/i18n'
	import type { EmbeddedArtwork, Tag, TagCategory, Track, TrackMetadataPatch } from '$shared/types'
	import { withNativeDialog } from '$shared/utils'
	import AlbumArt from './AlbumArt.svelte'
	import Button from './Button.svelte'
	import Modal from './Modal.svelte'

	type SaveResult = { errors: string[] }
	type EditableField =
		| 'title'
		| 'artist'
		| 'album'
		| 'year'
		| 'genre'
		| 'label'
		| 'catalog_number'
		| 'bpm'
		| 'key'
		| 'rating'

	type Props = {
		track: Track
		tagCategories: TagCategory[]
		onClose: () => void
		onSave: (
			track: Track,
			patch: TrackMetadataPatch,
			tagIds: string[],
			crateArtwork: CrateArtworkChange
		) => Promise<SaveResult>
	}

	let { track, tagCategories, onClose, onSave }: Props = $props()

	const nullableFields: EditableField[] = [
		'title',
		'artist',
		'album',
		'year',
		'genre',
		'label',
		'catalog_number',
		'bpm',
		'key',
	]
	const fieldLabels: Record<EditableField, string> = {
		title: 'title',
		artist: 'artist',
		album: 'album',
		year: 'year',
		genre: 'genre',
		label: 'label',
		catalog_number: 'catalogNumber',
		bpm: 'bpm',
		key: 'key',
		rating: 'rating',
	}
	const imageMimes: Record<string, string> = {
		bmp: 'image/bmp',
		gif: 'image/gif',
		jpeg: 'image/jpeg',
		jpg: 'image/jpeg',
		png: 'image/png',
		webp: 'image/webp',
	}

	let draft = $state<Partial<Record<EditableField, string>>>({})
	let clearedFields = $state<EditableField[]>([])
	let selectedTagIds = $state<string[] | null>(null)
	let crateArtwork = $state<CrateArtworkChange>({ type: 'unchanged' })
	let embeddedArtwork = $state<EmbeddedArtwork | null>(null)
	let embeddedArtworkCleared = $state(false)
	let saving = $state(false)
	let validationError = $state<string | null>(null)
	let saveErrors = $state<string[]>([])

	const allTags = $derived.by(() => {
		const knownIds = new Set(tagCategories.flatMap((category) => category.tags.map((tag) => tag.id)))
		const existingUnlisted = track.tags.filter((tag) => !knownIds.has(tag.id))
		return [...tagCategories.flatMap((category) => category.tags), ...existingUnlisted]
	})
	const hasChanges = $derived.by(() => {
		const patch = buildPatch()
		return (
			Object.keys(patch).length > 0 ||
			!sameIds(
				selectedTagIds ?? track.tags.map((tag) => tag.id),
				track.tags.map((tag) => tag.id)
			) ||
			crateArtwork.type !== 'unchanged' ||
			embeddedArtwork !== null ||
			embeddedArtworkCleared
		)
	})

	function sameIds(left: string[], right: string[]): boolean {
		return left.length === right.length && left.every((id) => right.includes(id))
	}

	function setField(field: EditableField, value: string) {
		draft = { ...draft, [field]: value }
		clearedFields = clearedFields.filter((cleared) => cleared !== field)
	}

	function clearField(field: EditableField) {
		if (field === 'rating') {
			draft = { ...draft, rating: '0' }
			return
		}
		clearedFields = clearedFields.includes(field) ? clearedFields : [...clearedFields, field]
	}

	function fieldValue(field: EditableField): string {
		if (clearedFields.includes(field)) return ''
		const draftValue = draft[field]
		if (draftValue !== undefined) return draftValue
		const original = track[field]
		return original === null ? '' : String(original)
	}

	function buildPatch(): TrackMetadataPatch {
		const patch: TrackMetadataPatch = {}
		for (const field of nullableFields) {
			if (clearedFields.includes(field)) {
				Object.assign(patch, { [field]: null })
				continue
			}
			const draftValue = draft[field]
			if (draftValue === undefined) continue
			const original = track[field]
			const originalValue = original === null ? '' : String(original)
			if (draftValue === originalValue) continue
			if (field === 'year' || field === 'bpm') {
				if (draftValue.trim() === '') continue
				const value = Number(draftValue)
				if (Number.isFinite(value)) Object.assign(patch, { [field]: value })
			} else {
				Object.assign(patch, { [field]: draftValue })
			}
		}
		const ratingValue = draft.rating
		const rating = Number(ratingValue ?? track.rating)
		if (
			ratingValue !== undefined &&
			Number.isInteger(rating) &&
			rating >= 0 &&
			rating <= 5 &&
			rating !== track.rating
		) {
			patch.rating = rating
		}
		return patch
	}

	function validatePatch(): string[] {
		const errors: string[] = []
		for (const field of ['year', 'bpm'] as const) {
			const draftValue = draft[field]
			if (clearedFields.includes(field) || draftValue === undefined || draftValue.trim() === '') continue
			const value = Number(draftValue)
			if (!Number.isFinite(value) || (field === 'year' && (!Number.isInteger(value) || value < 0))) {
				errors.push(get(translate)(`modals.trackMetadata.invalid.${field}`))
			}
		}
		if (
			draft.rating !== undefined &&
			(!Number.isInteger(Number(draft.rating)) || Number(draft.rating) < 0 || Number(draft.rating) > 5)
		) {
			errors.push(get(translate)('modals.trackMetadata.invalid.rating'))
		}
		return errors
	}

	function toggleTag(tag: Tag, checked: boolean) {
		const currentIds = selectedTagIds ?? track.tags.map((item) => item.id)
		selectedTagIds = checked
			? currentIds.includes(tag.id)
				? currentIds
				: [...currentIds, tag.id]
			: currentIds.filter((id) => id !== tag.id)
	}

	async function chooseImage(): Promise<{ path: string; artwork: EmbeddedArtwork } | null> {
		const selected = await withNativeDialog(() =>
			open({
				multiple: false,
				title: get(translate)('modals.trackMetadata.chooseArtwork'),
				filters: [{ name: 'Image files', extensions: Object.keys(imageMimes) }],
			})
		)
		if (typeof selected !== 'string') return null

		const extension =
			selected
				.split(/[./\\]/)
				.pop()
				?.toLowerCase() ?? ''
		const mimeType = imageMimes[extension]
		if (!mimeType) throw new Error(get(translate)('modals.trackMetadata.invalidImage'))

		const bytes = await readFile(selected)
		if (bytes.length === 0) throw new Error(get(translate)('modals.trackMetadata.invalidImage'))
		const buffer = new ArrayBuffer(bytes.byteLength)
		new Uint8Array(buffer).set(bytes)
		const bitmap = await createImageBitmap(new Blob([buffer], { type: mimeType }))
		bitmap.close()

		return { path: selected, artwork: { mime_type: mimeType, data: Array.from(bytes) } }
	}

	async function handleSelectCrateArtwork() {
		validationError = null
		try {
			const selected = await chooseImage()
			if (selected) crateArtwork = { type: 'set', filePath: selected.path }
		} catch (error) {
			validationError = error instanceof Error ? error.message : String(error)
		}
	}

	async function handleSelectEmbeddedArtwork() {
		validationError = null
		try {
			const selected = await chooseImage()
			if (selected) {
				embeddedArtwork = selected.artwork
				embeddedArtworkCleared = false
			}
		} catch (error) {
			validationError = error instanceof Error ? error.message : String(error)
		}
	}

	async function handleSave() {
		if (saving) return
		const patch = buildPatch()
		const validationErrors = validatePatch()
		if (validationErrors.length > 0) {
			validationError = validationErrors.join(' ')
			return
		}
		if (embeddedArtwork) patch.embedded_artwork = embeddedArtwork
		else if (embeddedArtworkCleared) patch.embedded_artwork = null
		if (Object.keys(patch).length === 0 && !hasChanges) return

		saving = true
		validationError = null
		saveErrors = []
		try {
			const result = await onSave(track, patch, selectedTagIds ?? track.tags.map((tag) => tag.id), crateArtwork)
			saveErrors = result.errors
			if (result.errors.length === 0) onClose()
		} catch (error) {
			saveErrors = [error instanceof Error ? error.message : String(error)]
		} finally {
			saving = false
		}
	}

	function formatDuration(milliseconds: number): string {
		const seconds = Math.floor(milliseconds / 1000)
		return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`
	}

	function formatDate(value: string | null): string {
		if (!value) return get(translate)('common.unknown')
		const date = new Date(value)
		return Number.isNaN(date.getTime()) ? value : date.toLocaleString()
	}

	const readOnlyProperties = $derived([
		{ key: 'duration', value: formatDuration(track.duration_ms) },
		{ key: 'format', value: track.format },
		{ key: 'bitrate', value: track.bitrate === null ? $translate('common.unknown') : `${track.bitrate} kbps` },
		{
			key: 'sampleRate',
			value: track.sample_rate === null ? $translate('common.unknown') : `${track.sample_rate} Hz`,
		},
		{ key: 'playCount', value: String(track.play_count) },
		{ key: 'dateAdded', value: formatDate(track.date_added) },
		{ key: 'dateModified', value: formatDate(track.date_modified) },
		{ key: 'lastPlayed', value: formatDate(track.last_played) },
		{ key: 'filePath', value: track.file_path },
		{ key: 'analysisSource', value: track.analysis_source ?? $translate('common.unknown') },
		{ key: 'color', value: track.color ?? $translate('common.unknown') },
		{ key: 'rekordboxId', value: track.rekordbox_id ?? $translate('common.unknown') },
	])
</script>

<Modal open={true} title={$translate('modals.trackMetadata.title')} size="xl" {onClose}>
	<div class="space-y-6">
		{#if validationError}
			<p role="alert" class="rounded-md bg-danger/10 p-3 text-sm text-danger">{validationError}</p>
		{/if}
		{#if saveErrors.length > 0}
			<div role="alert" class="rounded-md border border-warning bg-surface-2 p-3 text-sm text-text-primary">
				<p class="font-medium">{$translate('modals.trackMetadata.partialSave')}</p>
				<ul class="mt-1 list-inside list-disc">
					{#each saveErrors as error (error)}
						<li>{error}</li>
					{/each}
				</ul>
				<p class="mt-2 text-text-secondary">{$translate('modals.trackMetadata.partialSaveHint')}</p>
			</div>
		{/if}

		<section class="space-y-3">
			<h3 class="text-sm font-semibold text-text-primary">{$translate('modals.trackMetadata.details')}</h3>
			<div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
				{#each nullableFields as field (field)}
					<label class="space-y-1 text-sm text-text-secondary">
						<span>{$translate(`modals.trackMetadata.fields.${fieldLabels[field]}`)}</span>
						<div class="flex gap-2">
							<input
								class="min-w-0 flex-1 rounded-md border border-stroke bg-surface-0 px-3 py-2 text-text-primary outline-none focus:border-brand-primary"
								type={field === 'year' || field === 'bpm' ? 'number' : 'text'}
								step={field === 'bpm' ? 'any' : '1'}
								value={fieldValue(field)}
								oninput={(event) => setField(field, event.currentTarget.value)}
							/>
							{#if nullableFields.includes(field)}
								<Button variant="secondary" size="sm" disabled={saving} onclick={() => clearField(field)}>
									{$translate('modals.trackMetadata.clearField')}
								</Button>
							{/if}
						</div>
					</label>
				{/each}
				<label class="space-y-1 text-sm text-text-secondary">
					<span>{$translate('modals.trackMetadata.fields.rating')}</span>
					<div class="flex gap-2">
						<input
							class="min-w-0 flex-1 rounded-md border border-stroke bg-surface-0 px-3 py-2 text-text-primary outline-none focus:border-brand-primary"
							type="number"
							min="0"
							max="5"
							step="1"
							value={fieldValue('rating')}
							oninput={(event) => setField('rating', event.currentTarget.value)}
						/>
						<Button variant="secondary" size="sm" disabled={saving} onclick={() => clearField('rating')}>
							{$translate('modals.trackMetadata.clearField')}
						</Button>
					</div>
				</label>
			</div>
		</section>

		<section class="grid grid-cols-1 gap-5 border-t border-stroke pt-5 md:grid-cols-2">
			<div class="space-y-3">
				<h3 class="text-sm font-semibold text-text-primary">{$translate('modals.trackMetadata.crateTags')}</h3>
				{#if allTags.length === 0}
					<p class="text-sm text-text-secondary">{$translate('modals.trackMetadata.noTags')}</p>
				{:else}
					<div class="max-h-40 space-y-2 overflow-y-auto rounded-md border border-stroke bg-surface-0 p-3">
						{#each allTags as tag (tag.id)}
							<label class="flex items-center gap-2 text-sm text-text-primary">
								<input
									type="checkbox"
									checked={(selectedTagIds ?? track.tags.map((item) => item.id)).includes(tag.id)}
									disabled={saving}
									onchange={(event) => toggleTag(tag, event.currentTarget.checked)}
								/>
								<span>{tag.name}</span>
								<span class="text-xs text-text-tertiary">
									{tagCategories.find((category) => category.id === tag.category_id)?.name ?? ''}
								</span>
							</label>
						{/each}
					</div>
				{/if}
				<p class="text-xs text-text-secondary">{$translate('modals.trackMetadata.crateTagsHint')}</p>
			</div>

			<div class="space-y-3">
				<h3 class="text-sm font-semibold text-text-primary">{$translate('modals.trackMetadata.crateArtwork')}</h3>
				<div class="flex items-center gap-3">
					<AlbumArt artworkPath={track.artwork_path} size="sm" />
					<div class="min-w-0 flex-1 text-sm text-text-secondary">
						<p>{$translate('modals.trackMetadata.crateArtworkHint')}</p>
						{#if crateArtwork.type === 'set'}
							<p class="truncate text-xs">{crateArtwork.filePath}</p>
						{:else if crateArtwork.type === 'clear'}
							<p class="text-xs">{$translate('modals.trackMetadata.artworkWillClear')}</p>
						{/if}
					</div>
				</div>
				<div class="flex flex-wrap gap-2">
					<Button variant="secondary" size="sm" disabled={saving} onclick={handleSelectCrateArtwork}>
						{$translate('modals.trackMetadata.chooseArtwork')}
					</Button>
					<Button
						variant="secondary"
						size="sm"
						disabled={saving || (track.artwork_path === null && crateArtwork.type !== 'set')}
						onclick={() => (crateArtwork = { type: 'clear' })}
					>
						{$translate('modals.trackMetadata.clearArtwork')}
					</Button>
					{#if crateArtwork.type !== 'unchanged'}
						<Button
							variant="secondary"
							size="sm"
							disabled={saving}
							onclick={() => (crateArtwork = { type: 'unchanged' })}
						>
							{$translate('modals.trackMetadata.undoArtworkChange')}
						</Button>
					{/if}
				</div>
			</div>
		</section>

		<section class="space-y-3 border-t border-stroke pt-5">
			<h3 class="text-sm font-semibold text-text-primary">{$translate('modals.trackMetadata.embeddedArtwork')}</h3>
			<p class="text-sm text-text-secondary">{$translate('modals.trackMetadata.embeddedArtworkHint')}</p>
			<div class="flex flex-wrap items-center gap-2">
				<Button variant="secondary" size="sm" disabled={saving} onclick={handleSelectEmbeddedArtwork}>
					{$translate('modals.trackMetadata.chooseArtwork')}
				</Button>
				<Button
					variant="secondary"
					size="sm"
					disabled={saving}
					onclick={() => {
						embeddedArtwork = null
						embeddedArtworkCleared = true
					}}
				>
					{$translate('modals.trackMetadata.clearArtwork')}
				</Button>
				{#if embeddedArtwork}
					<span class="text-xs text-text-secondary">{$translate('modals.trackMetadata.artworkReady')}</span>
				{:else if embeddedArtworkCleared}
					<span class="text-xs text-text-secondary">{$translate('modals.trackMetadata.artworkWillClear')}</span>
				{/if}
				{#if embeddedArtwork !== null || embeddedArtworkCleared}
					<Button
						variant="secondary"
						size="sm"
						disabled={saving}
						onclick={() => {
							embeddedArtwork = null
							embeddedArtworkCleared = false
						}}
					>
						{$translate('modals.trackMetadata.undoArtworkChange')}
					</Button>
				{/if}
			</div>
		</section>

		<section class="space-y-3 border-t border-stroke pt-5">
			<h3 class="text-sm font-semibold text-text-primary">{$translate('modals.trackMetadata.technicalDetails')}</h3>
			<dl class="grid grid-cols-1 gap-x-4 gap-y-2 text-sm sm:grid-cols-2">
				{#each readOnlyProperties as item (item.key)}
					<div class="min-w-0">
						<dt class="text-text-secondary">{$translate(`modals.trackMetadata.readOnly.${item.key}`)}</dt>
						<dd class="truncate text-text-primary" title={item.value}>{item.value}</dd>
					</div>
				{/each}
			</dl>
		</section>
	</div>

	{#snippet footer()}
		<Button variant="secondary" disabled={saving} onclick={onClose}>{$translate('common.cancel')}</Button>
		<Button variant="primary" disabled={saving || !hasChanges} onclick={handleSave}>
			{saving ? $translate('common.loading') : $translate('common.save')}
		</Button>
	{/snippet}
</Modal>
