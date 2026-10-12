<script lang="ts">
	import { translate } from '$shared/i18n'
	import { getArtworkUrl } from '$shared/utils'
	import { appDataDir } from '$lib/stores/app'
	import Icon from './Icon.svelte'

	type Size = 'xs' | 'sm' | 'md' | 'lg'

	type Props = {
		artworkPath: string | null
		artworkUrl?: string | null
		size?: Size
		class?: string
		onclick?: () => void
	}

	let { artworkPath, artworkUrl = null, size = 'md', class: className = '', onclick }: Props = $props()

	const sizeClasses: Record<Size, string> = {
		xs: 'h-6 w-6',
		sm: 'h-8 w-8',
		md: 'h-12 w-12',
		lg: 'aspect-square w-full max-w-[400px]',
	}

	const iconSizes: Record<Size, string> = {
		xs: 'h-4 w-4',
		sm: 'h-5 w-5',
		md: 'h-8 w-8',
		lg: 'h-36 w-36',
	}

	let localUrl = $derived(getArtworkUrl(artworkPath, $appDataDir))
	let localError = $state(false)
	let externalError = $state(false)

	/**
	 * True while the local cover is the one being rendered.
	 *
	 * The local cover is served over Tauri's `asset://` scheme. On Linux/WebKitGTK that custom
	 * scheme is cross-origin to the page, so a plain (no-CORS) `<img>` is rejected before the
	 * request is even made — the Rust handler never sees it and the vinyl placeholder shows for
	 * every track. Asking for the asset in CORS mode fixes it: the handler already answers with
	 * `Access-Control-Allow-Origin`. Remote covers keep the default no-CORS load, which must not
	 * require CORS headers from a third-party host.
	 */
	let usingAssetProtocol = $derived(Boolean(localUrl) && !localError)

	let displayUrl = $derived.by(() => {
		if (localUrl && !localError) return localUrl
		if (artworkUrl && !externalError) return artworkUrl
		return null
	})

	$effect(() => {
		// Reset error states when props change
		void artworkPath
		localError = false
	})

	$effect(() => {
		void artworkUrl
		externalError = false
	})

	function handleError() {
		if (localUrl && !localError) {
			localError = true
		} else {
			externalError = true
		}
	}

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Enter' || e.key === ' ') {
			e.preventDefault()
			onclick?.()
		}
	}
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div
	class="flex flex-shrink-0 items-center justify-center overflow-hidden rounded bg-surface-2 {sizeClasses[
		size
	]} {className}"
	role={onclick ? 'button' : undefined}
	tabindex={onclick ? 0 : undefined}
	{onclick}
	onkeydown={onclick ? handleKeydown : undefined}
>
	{#if displayUrl}
		<img
			src={displayUrl}
			alt={$translate('modals.albumArt')}
			class="h-full w-full object-cover"
			crossorigin={usingAssetProtocol ? 'anonymous' : undefined}
			onerror={handleError}
		/>
	{:else}
		<Icon name="vinyl" class="{iconSizes[size]} text-text-tertiary" />
	{/if}
</div>
