<script lang="ts">
	import { Checkbox, Text } from '$lib/components/common'
	import { Button } from '$lib/components/common'
	import {
		settingsStore,
		taggerAutoApplyEnabled,
		taggerAutoApplyThreshold,
		taggerWeights,
	} from '$shared/stores/settings'
	import { translate } from '$shared/i18n'

	interface Weights {
		title: number
		artist: number
		duration: number
		genre: number
		label: number
		bpm: number
		key: number
	}

	const DEFAULT_WEIGHTS: Weights = {
		title: 0.4,
		artist: 0.25,
		duration: 0.1,
		genre: 0.1,
		label: 0.08,
		bpm: 0.04,
		key: 0.03,
	}

	let weights = $state<Weights>(parseWeights($taggerWeights))
	let weightsSumError = $state(false)

	function parseWeights(json: string | null): Weights {
		if (!json) return { ...DEFAULT_WEIGHTS }
		try {
			const parsed = JSON.parse(json)
			return {
				title: parsed.title ?? DEFAULT_WEIGHTS.title,
				artist: parsed.artist ?? DEFAULT_WEIGHTS.artist,
				duration: parsed.duration ?? DEFAULT_WEIGHTS.duration,
				genre: parsed.genre ?? DEFAULT_WEIGHTS.genre,
				label: parsed.label ?? DEFAULT_WEIGHTS.label,
				bpm: parsed.bpm ?? DEFAULT_WEIGHTS.bpm,
				key: parsed.key ?? DEFAULT_WEIGHTS.key,
			}
		} catch {
			return { ...DEFAULT_WEIGHTS }
		}
	}

	function checkSum(w: Weights): boolean {
		const sum = w.title + w.artist + w.duration + w.genre + w.label + w.bpm + w.key
		return Math.abs(sum - 1.0) < 0.001
	}

	function handleWeightChange(field: keyof Weights, value: string) {
		const num = parseFloat(value)
		if (isNaN(num)) return
		const newWeights = { ...weights, [field]: num }
		weights = newWeights
		weightsSumError = !checkSum(newWeights)
	}

	function handleSaveWeights() {
		if (!checkSum(weights)) return
		settingsStore.setTaggerWeights(JSON.stringify(weights))
	}

	function handleResetWeights() {
		weights = { ...DEFAULT_WEIGHTS }
		weightsSumError = false
		settingsStore.setTaggerWeights(null)
	}

	function handleAutoApplyEnabledChange(checked: boolean) {
		settingsStore.setTaggerAutoApplyEnabled(checked)
	}

	function handleThresholdChange(e: Event) {
		const input = e.target as HTMLInputElement
		const percent = parseFloat(input.value)
		if (isNaN(percent)) return
		settingsStore.setTaggerAutoApplyThreshold(percent / 100)
	}

	const thresholdPercent = $derived(Math.round($taggerAutoApplyThreshold * 100))

	const weightsSum = $derived(
		weights.title + weights.artist + weights.duration + weights.genre + weights.label + weights.bpm + weights.key
	)
	const weightsSumPercent = $derived(Math.round(weightsSum * 100))
</script>

<div class="space-y-8">
	<!-- Auto-Apply Section -->
	<section>
		<Text variant="header-3" class="mb-2">{$translate('settings.tagger.autoApply')}</Text>
		<Text variant="caption" as="p" class="mb-4">{$translate('settings.tagger.autoApplyDescription')}</Text>

		<div class="space-y-4">
			<Checkbox
				checked={$taggerAutoApplyEnabled}
				onchange={handleAutoApplyEnabledChange}
				label={$translate('settings.tagger.autoApplyEnabled')}
			/>
			<Text variant="caption" as="p" class="ml-6">
				{$translate('settings.tagger.autoApplyEnabledDescription')}
			</Text>

			{#if $taggerAutoApplyEnabled}
				<div class="ml-6 space-y-2">
					<div class="flex items-center justify-between">
						<Text variant="body-1">
							{$translate('settings.tagger.autoApplyThreshold')}
						</Text>
						<Text variant="body-1" class="font-mono text-text-secondary">
							{thresholdPercent}%
						</Text>
					</div>
					<input
						type="range"
						min="50"
						max="100"
						step="1"
						value={thresholdPercent}
						oninput={handleThresholdChange}
						class="w-full max-w-md accent-brand-primary"
					/>
					<Text variant="caption" as="p">
						{$translate('settings.tagger.autoApplyThresholdDescription')}
					</Text>
				</div>
			{/if}
		</div>
	</section>

	<!-- Scoring Weights Section -->
	<section>
		<Text variant="header-3" class="mb-2">{$translate('settings.tagger.weights')}</Text>
		<Text variant="caption" as="p" class="mb-4">{$translate('settings.tagger.weightsDescription')}</Text>

		<div class="space-y-3">
			{#each (['title', 'artist', 'duration', 'genre', 'label', 'bpm', 'key'] as const) as field (field)}
				<div class="flex items-center gap-4">
					<Text variant="body-1" class="w-20 shrink-0">
						{$translate(`settings.tagger.weight${field.charAt(0).toUpperCase()}${field.slice(1)}`)}
					</Text>
					<input
						type="range"
						min="0"
						max="100"
						step="1"
						value={Math.round(weights[field] * 100)}
						oninput={(e) => handleWeightChange(field, String(Number((e.target as HTMLInputElement).value) / 100))}
						class="flex-1 accent-brand-primary"
					/>
					<Text variant="body-1" class="w-12 text-right font-mono text-text-secondary">
						{Math.round(weights[field] * 100)}%
					</Text>
				</div>
			{/each}
		</div>

		<div class="mt-4 flex items-center gap-4">
			<Button variant="secondary" onclick={handleSaveWeights} disabled={weightsSumError}>
				{#if weightsSumError}
					{$translate('settings.tagger.weightsSumError', { values: { current: weightsSumPercent } })}
				{:else}
					{weightsSumPercent}%
				{/if}
			</Button>
			<Button variant="ghost" onclick={handleResetWeights}>
				{$translate('settings.tagger.resetWeights')}
			</Button>
		</div>
	</section>
</div>