import { describe, it, expect } from 'vitest'
import { getStoreName } from './storeUrl'

describe('getStoreName', () => {
	it('matches Beatport on www and bare host', () => {
		expect(getStoreName('https://www.beatport.com/track/x/123')).toBe('Beatport')
		expect(getStoreName('https://beatport.com/release/y')).toBe('Beatport')
	})

	it('matches Bandcamp on subdomains and bare host', () => {
		expect(getStoreName('https://someartist.bandcamp.com/track/thing')).toBe('Bandcamp')
		expect(getStoreName('https://bandcamp.com/foo')).toBe('Bandcamp')
	})

	it('matches Traxsource', () => {
		expect(getStoreName('https://www.traxsource.com/title/1')).toBe('Traxsource')
	})

	it('tolerates a missing scheme', () => {
		expect(getStoreName('www.beatport.com/track/1')).toBe('Beatport')
	})

	it('accepts a valid URL padded with whitespace', () => {
		expect(getStoreName('  https://www.beatport.com/track/x/123  ')).toBe('Beatport')
	})

	it('is case-insensitive on the host', () => {
		expect(getStoreName('HTTPS://WWW.BEATPORT.COM/Track')).toBe('Beatport')
	})

	it('ignores port, path, and query', () => {
		expect(getStoreName('https://bandcamp.com:443/x?y=1')).toBe('Bandcamp')
	})

	it('returns null for unknown hosts', () => {
		expect(getStoreName('https://soundcloud.com/foo')).toBeNull()
	})

	it('does not match lookalike suffixes', () => {
		expect(getStoreName('https://notbeatport.com/x')).toBeNull()
		expect(getStoreName('https://beatport.com.evil.io/x')).toBeNull()
	})

	it('returns null for blank, whitespace-only, null, or undefined input', () => {
		expect(getStoreName('')).toBeNull()
		expect(getStoreName('   ')).toBeNull()
		expect(getStoreName(null)).toBeNull()
		expect(getStoreName(undefined)).toBeNull()
	})

	it('returns null without throwing on unparseable URLs', () => {
		expect(getStoreName('https://')).toBeNull()
		expect(getStoreName('::not a url::')).toBeNull()
	})
})
