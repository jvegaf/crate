import { describe, it, expect } from 'vitest'
import { extractBeatportTrackId, swapBeatportImageSize } from './beatport'

describe('extractBeatportTrackId', () => {
	it('extracts the id from a full track URL', () => {
		expect(extractBeatportTrackId('https://www.beatport.com/track/your-mind/123456')).toBe(123456)
	})

	it('accepts the bare host without www', () => {
		expect(extractBeatportTrackId('https://beatport.com/track/your-mind/654321')).toBe(654321)
	})

	it('tolerates a missing scheme', () => {
		expect(extractBeatportTrackId('www.beatport.com/track/your-mind/20528025')).toBe(20528025)
	})

	it('accepts a URL padded with whitespace', () => {
		expect(extractBeatportTrackId('  https://www.beatport.com/track/your-mind/123456  ')).toBe(123456)
	})

	it('returns null for a release URL', () => {
		expect(extractBeatportTrackId('https://www.beatport.com/release/acid/123456')).toBeNull()
	})

	it('returns null for non-Beatport hosts', () => {
		expect(extractBeatportTrackId('https://www.traxsource.com/title/1')).toBeNull()
		expect(extractBeatportTrackId('https://open.spotify.com/track/abc')).toBeNull()
	})

	it('does not match lookalike suffixes', () => {
		expect(extractBeatportTrackId('https://beatport.com.evil.io/track/x/123')).toBeNull()
	})

	it('returns null when the id segment is missing or not numeric', () => {
		expect(extractBeatportTrackId('https://www.beatport.com/track/your-mind')).toBeNull()
		expect(extractBeatportTrackId('https://www.beatport.com/track/123456/your-mind')).toBeNull()
	})

	it('returns null for blank, whitespace-only, null, or undefined input', () => {
		expect(extractBeatportTrackId('')).toBeNull()
		expect(extractBeatportTrackId('   ')).toBeNull()
		expect(extractBeatportTrackId(null)).toBeNull()
		expect(extractBeatportTrackId(undefined)).toBeNull()
	})
})

describe('swapBeatportImageSize', () => {
	it('swaps the resolved 800x800 segment for the requested size', () => {
		expect(swapBeatportImageSize('https://geo-media.beatport.com/image_size/800x800/abc.jpg', '80x80')).toBe(
			'https://geo-media.beatport.com/image_size/80x80/abc.jpg'
		)
	})

	it('leaves URLs without the resolved size untouched', () => {
		expect(swapBeatportImageSize('https://geo-media.beatport.com/image/abc.jpg', '80x80')).toBe(
			'https://geo-media.beatport.com/image/abc.jpg'
		)
	})

	it('returns null for missing input', () => {
		expect(swapBeatportImageSize(null, '80x80')).toBeNull()
		expect(swapBeatportImageSize(undefined, '80x80')).toBeNull()
	})
})
