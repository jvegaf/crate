<script lang="ts">
	import { MIXED_PLACEHOLDER } from '$shared/utils'
	import Text from '$lib/components/common/Text.svelte'

	type Props = {
		label: string
		value: string | number | null
		mixed?: boolean
		type?: 'text' | 'number'
		step?: string
		placeholder?: string
		disabled?: boolean
		onchange?: (value: string | number | null) => void
		onsubmit?: () => void
		onblur?: () => void
	}

	let {
		label,
		value = $bindable(),
		mixed = false,
		type = 'text',
		step = undefined,
		placeholder = '',
		disabled = false,
		onchange,
		onsubmit,
		onblur,
	}: Props = $props()

	// Local state for the input - shows placeholder when mixed
	let inputValue = $state('')

	// Sync inputValue with value prop
	$effect(() => {
		if (mixed) {
			inputValue = ''
		} else {
			const next = value === null || value === undefined ? '' : String(value)
			// Keep an in-progress decimal like "123." or "12.5" intact when the parsed number is unchanged
			const partialDecimal =
				type === 'number' && next !== '' && inputValue !== '' && Number(next) === Number(inputValue)
			if (next !== inputValue && !partialDecimal) {
				inputValue = next
			}
		}
	})

	function handleInput(e: Event) {
		const target = e.target as HTMLInputElement
		inputValue = target.value

		if (type === 'number') {
			const numValue = target.value === '' ? null : Number(target.value)
			value = numValue
			onchange?.(numValue)
		} else {
			const strValue = target.value === '' ? null : target.value
			value = strValue
			onchange?.(strValue)
		}
	}

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Enter') {
			e.preventDefault()
			onsubmit?.()
		}
	}
</script>

<label class="block space-y-1">
	<Text as="span" size="xs" weight="medium" color="secondary" class="block">{label}</Text>
	<input
		{type}
		{step}
		value={inputValue}
		placeholder={mixed ? MIXED_PLACEHOLDER : placeholder}
		{disabled}
		oninput={handleInput}
		onkeydown={handleKeydown}
		onblur={() => onblur?.()}
		class="w-full rounded-md border border-stroke bg-surface-2 px-3 py-1.5 text-sm text-text-primary placeholder-text-tertiary focus:border-transparent focus:ring-2 focus:ring-brand-primary focus:outline-none disabled:cursor-not-allowed disabled:opacity-50"
	/>
</label>
