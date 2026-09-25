<script lang="ts">
	import { toasts, toastStore } from '$shared/stores/toast'
	import Toast from './Toast.svelte'

	let el: HTMLDivElement | undefined = $state()

	// A modal `<dialog>` (Settings, for one) renders into the browser's top layer, which
	// paints above every z-index — a plain fixed container would sit under that dialog's
	// `::backdrop` and read as dimmed. This container is a `popover` of its own: also
	// top-layer, and inserted only once a toast exists, which is after any dialog that was
	// already open. Top-layer elements are ordered by insertion, so that ordering is what
	// keeps a toast above the backdrop of the modal that triggered it.
	$effect(() => {
		if (el && !el.matches(':popover-open')) el.showPopover()
	})
</script>

{#if $toasts.length > 0}
	<div
		bind:this={el}
		popover="manual"
		class="fixed top-auto right-4 bottom-4 left-auto z-50 m-0 flex h-fit w-fit flex-col gap-2
		overflow-visible border-0 bg-transparent p-0"
	>
		{#each $toasts as toast (toast.id)}
			<Toast {toast} onDismiss={() => toastStore.dismiss(toast.id)} />
		{/each}
	</div>
{/if}
