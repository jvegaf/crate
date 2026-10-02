// Beatport track page: https://www.beatport.com/track/{slug}/{numeric-id}
// The scheme is optional because manually edited WOAR tags may omit it. Only a
// `track` page carries a usable recommendations id; release/label URLs do not.
const BEATPORT_TRACK_URL = /beatport\.com\/track\/[^/]+\/(\d+)/

/**
 * Beatport numeric track id parsed out of a stored track URL, or null when the
 * URL is missing, not a Beatport track page, or has no numeric id segment.
 */
export function extractBeatportTrackId(url: string | null | undefined): number | null {
	if (!url) return null
	const match = url.trim().match(BEATPORT_TRACK_URL)
	if (!match) return null
	const id = Number(match[1])
	return Number.isSafeInteger(id) && id > 0 ? id : null
}

/**
 * Recommendation artwork/waveform URLs arrive pre-resolved to `800x800`
 * (see the Rust recommendations service). geo-media.beatport.com serves
 * arbitrary sizes, so thumbs swap that segment — never a `{w}x{h}` placeholder.
 */
export function swapBeatportImageSize(url: string | null | undefined, size: string): string | null {
	if (!url) return null
	return url.replace('800x800', size)
}
