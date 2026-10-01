export type StoreName = 'Beatport' | 'Bandcamp' | 'Traxsource'

// host suffix -> display name; matches the bare host and any subdomain
const STORE_HOSTS: ReadonlyArray<{ name: StoreName; hosts: string[] }> = [
	{ name: 'Beatport', hosts: ['beatport.com'] },
	{ name: 'Bandcamp', hosts: ['bandcamp.com'] },
	{ name: 'Traxsource', hosts: ['traxsource.com'] },
]

/** Hostname of a store URL, or null when it cannot be parsed. Tolerates a missing scheme. */
function extractHost(url: string): string | null {
	const trimmed = url.trim()
	if (!trimmed) return null
	// manually edited tags may omit the scheme (e.g. "www.beatport.com/track/…")
	const candidate = /^[a-z][a-z0-9+.-]*:/i.test(trimmed) ? trimmed : `https://${trimmed}`
	try {
		return new URL(candidate).hostname.toLowerCase()
	} catch {
		return null
	}
}

/** Known store a track URL belongs to, or null for unknown/unparseable URLs. */
export function getStoreName(url: string | null | undefined): StoreName | null {
	if (!url) return null
	const host = extractHost(url)
	if (!host) return null
	for (const store of STORE_HOSTS) {
		if (store.hosts.some((domain) => host === domain || host.endsWith(`.${domain}`))) {
			return store.name
		}
	}
	return null
}
