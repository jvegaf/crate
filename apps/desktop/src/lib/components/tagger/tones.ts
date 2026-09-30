/**
 * Shared visual tones for the tagger candidate layouts. Kept in one module so the
 * single-track modal and the batch modal cannot drift apart.
 */

export const CAPSULE_BASE_CLASS = 'rounded-full px-2 py-0.5 text-[11px] font-medium'

/**
 * Brand identity colours per provider. These are identities, not states, so they
 * stay on the raw palette instead of the success/info/warning tokens. Unknown
 * providers fall back to a neutral surface.
 */
export function providerCapsuleClass(provider: string): string {
	let tone: string
	switch (provider) {
		case 'beatport':
			tone = 'bg-green-500/15 text-green-500'
			break
		case 'traxsource':
			tone = 'bg-blue-500/15 text-blue-500'
			break
		case 'bandcamp':
			tone = 'bg-orange-500/15 text-orange-500'
			break
		default:
			tone = 'bg-surface-2 text-text-tertiary'
	}
	return `${CAPSULE_BASE_CLASS} ${tone}`
}

/** Match score is a state: use the semantic tokens so the best row reads at a glance. */
export function scoreClass(score: number): string {
	if (score >= 0.75) return 'bg-success/15 text-success'
	if (score >= 0.4) return 'bg-warning/15 text-warning'
	return 'bg-danger/15 text-danger'
}
