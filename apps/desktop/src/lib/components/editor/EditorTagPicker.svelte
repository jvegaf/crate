<script lang="ts">
	import { allTags, tagsStore } from '$shared/stores/tags'
	import { translate } from '$shared/i18n'
	import { toastStore } from '$shared/stores/toast'
	import type { Tag } from '$shared/types'
	import Button from '$lib/components/common/Button.svelte'
	import Checkbox from '$lib/components/common/Checkbox.svelte'

	type Props = {
		selectedTagIds: string[]
		disabled?: boolean
		onAssign: (tagId: string) => void | Promise<void>
		onUnassign: (tagId: string) => void | Promise<void>
		onClose: () => void
	}

	let { selectedTagIds, disabled = false, onAssign, onUnassign, onClose }: Props = $props()

	let search = $state('')
	let creating = $state(false)
	let panel: HTMLDivElement | undefined = $state()
	let requestedLoad = false

	const categories = $derived($tagsStore.categories)
	const normalizedSearch = $derived(search.trim().toLocaleLowerCase())
	const matchingCategories = $derived.by(() =>
		categories
			.map((category) => ({
				...category,
				tags: category.tags.filter((tag) => tag.name.toLocaleLowerCase().includes(normalizedSearch)),
			}))
			.filter((category) => category.tags.length > 0 || normalizedSearch.length > 0)
	)
	const exactMatch = $derived(
		normalizedSearch.length > 0 && $allTags.some((tag) => tag.name.toLocaleLowerCase() === normalizedSearch)
	)

	$effect(() => {
		if (!requestedLoad && categories.length === 0 && !$tagsStore.loading) {
			requestedLoad = true
			void tagsStore.load()
		}
	})

	$effect(() => {
		function handlePointerDown(event: PointerEvent) {
			if (!(event.target instanceof Node) || !panel?.parentElement?.contains(event.target)) onClose()
		}
		function handleKeyDown(event: KeyboardEvent) {
			if (event.key === 'Escape') {
				event.preventDefault()
				onClose()
			}
		}

		window.addEventListener('pointerdown', handlePointerDown)
		window.addEventListener('keydown', handleKeyDown)
		return () => {
			window.removeEventListener('pointerdown', handlePointerDown)
			window.removeEventListener('keydown', handleKeyDown)
		}
	})

	function toggleTag(tagId: string, checked: boolean) {
		if (disabled || creating) return
		if (checked) void onAssign(tagId)
		else void onUnassign(tagId)
	}

	async function createAndAssign(categoryId: string) {
		const name = search.trim()
		if (disabled || creating || !categoryId || !name || exactMatch) return

		creating = true
		try {
			const createdTag = await tagsStore.createTag(categoryId, name)
			const tag: Tag | undefined =
				createdTag ??
				$allTags.find(
					(candidate) =>
						candidate.category_id === categoryId && candidate.name.toLocaleLowerCase() === name.toLocaleLowerCase()
				)
			if (!tag) {
				toastStore.error($tagsStore.error ?? 'Failed to create tag')
				return
			}

			await onAssign(tag.id)
		} catch (error) {
			toastStore.error(error instanceof Error ? error.message : String(error))
		} finally {
			creating = false
		}
	}
</script>

<div
	bind:this={panel}
	role="dialog"
	aria-label={$translate('editor.addTags')}
	class="absolute top-full right-0 z-50 mt-2 max-h-72 w-64 overflow-y-auto rounded-lg border border-stroke bg-surface-2 p-2 shadow-lg"
>
	<input
		bind:value={search}
		type="search"
		placeholder={$translate('editor.searchTags')}
		aria-label={$translate('editor.searchTags')}
		disabled={disabled || creating}
		class="mb-2 w-full rounded-md border border-stroke bg-surface-0 px-2.5 py-2 text-sm text-text-primary outline-none placeholder:text-text-tertiary focus:border-brand-primary disabled:opacity-50"
	/>

	<div class="space-y-2">
		{#each matchingCategories as category (category.id)}
			<section>
				<p class="mb-1 px-1 text-xs font-medium text-text-tertiary">{category.name}</p>
				<div class="space-y-1">
					{#each category.tags as tag (tag.id)}
						<Checkbox
							label={tag.name}
							checked={selectedTagIds.includes(tag.id)}
							disabled={disabled || creating}
							onchange={(checked) => toggleTag(tag.id, checked)}
						/>
					{/each}
					{#if normalizedSearch && !exactMatch}
						<Button
							variant="ghost"
							size="sm"
							class="w-full justify-start"
							disabled={disabled || creating}
							onclick={() => createAndAssign(category.id)}
						>
							{$translate('editor.createTag', { values: { name: search.trim() } })}
						</Button>
					{/if}
				</div>
			</section>
		{/each}
	</div>
</div>
