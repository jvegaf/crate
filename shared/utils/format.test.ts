import { describe, it, expect } from 'vitest'
import { formatDuration, formatDurationCompact, formatBpm, formatBitrate, formatKey, keyToCamelot } from './format'

describe('keyToCamelot', () => {
	it('resolves standard notation to Camelot', () => {
		expect(keyToCamelot('Am')).toBe('8A')
		expect(keyToCamelot('C')).toBe('8B')
		expect(keyToCamelot('F#m')).toBe('11A')
		expect(keyToCamelot('Ebm')).toBe('2A')
		expect(keyToCamelot('Gbm')).toBe('11A')
		expect(keyToCamelot('Ab')).toBe('4B')
	})

	it('accepts Camelot input as-is and normalizes the mode letter to uppercase', () => {
		expect(keyToCamelot('8A')).toBe('8A')
		expect(keyToCamelot('8a')).toBe('8A')
		expect(keyToCamelot('12b')).toBe('12B')
	})

	it('resolves tagger free-text forms that the plain lookup map misses', () => {
		expect(keyToCamelot('G Minor')).toBe('6A')
		expect(keyToCamelot('G minor')).toBe('6A')
		expect(keyToCamelot('G min')).toBe('6A')
		expect(keyToCamelot('A minor')).toBe('8A')
		expect(keyToCamelot('F# min')).toBe('11A')
		expect(keyToCamelot('G major')).toBe('9B')
		expect(keyToCamelot('Gmaj')).toBe('9B')
	})

	it('returns null for anything it cannot resolve instead of guessing', () => {
		expect(keyToCamelot(null)).toBeNull()
		expect(keyToCamelot('')).toBeNull()
		expect(keyToCamelot('   ')).toBeNull()
		expect(keyToCamelot('s-key')).toBeNull()
		expect(keyToCamelot('H')).toBeNull()
		expect(keyToCamelot('13A')).toBeNull()
		expect(keyToCamelot('0A')).toBeNull()
	})
})

describe('formatKey', () => {
	it('converts standard notation to Camelot by default', () => {
		expect(formatKey('Am')).toBe('8A')
		expect(formatKey('C')).toBe('8B')
	})

	it('converts Camelot back to canonical standard notation', () => {
		expect(formatKey('8A', 'standard')).toBe('Am')
		expect(formatKey('8B', 'standard')).toBe('C')
	})

	it('passes unknown values through untouched and renders null as a dash', () => {
		expect(formatKey(null)).toBe('-')
		expect(formatKey('s-key')).toBe('s-key')
		expect(formatKey('s-key', 'standard')).toBe('s-key')
	})
})

describe('formatDuration', () => {
	it('formats 0ms as 0:00', () => {
		expect(formatDuration(0)).toBe('0:00')
	})

	it('formats under-60s correctly', () => {
		expect(formatDuration(45_000)).toBe('0:45')
		expect(formatDuration(59_999)).toBe('0:59')
	})

	it('formats minutes with leading zeros', () => {
		expect(formatDuration(120_000)).toBe('2:00')
		expect(formatDuration(90_000)).toBe('1:30')
	})

	it('formats hours when duration >= 3600s', () => {
		expect(formatDuration(3_600_000)).toBe('1:00:00')
		expect(formatDuration(7_260_000)).toBe('2:01:00')
	})

	it('handles partial seconds (floors)', () => {
		expect(formatDuration(65_500)).toBe('1:05')
	})

	it('large values work', () => {
		expect(formatDuration(8_640_000)).toBe('2:24:00')
	})
})

describe('formatDurationCompact', () => {
	it('drops hours even for very long durations', () => {
		expect(formatDurationCompact(3_661_000)).toBe('61:01')
	})

	it('works like regular formatDuration for short durations', () => {
		expect(formatDurationCompact(90_000)).toBe('1:30')
		expect(formatDurationCompact(0)).toBe('0:00')
	})

	it('always pads seconds', () => {
		expect(formatDurationCompact(10_100)).toBe('0:10')
		expect(formatDurationCompact(60_050)).toBe('1:00')
	})
})

describe('formatBpm', () => {
	it('returns dash for null BPM', () => {
		expect(formatBpm(null)).toBe('-')
	})

	it('formats integer BPM with one decimal', () => {
		expect(formatBpm(120)).toBe('120.0')
	})

	it('preserves decimal places up to one digit', () => {
		expect(formatBpm(123.456)).toBe('123.5')
	})

	it('handles zero BPM', () => {
		expect(formatBpm(0)).toBe('0.0')
	})
})

describe('formatBitrate', () => {
	it('formats stored kilobits per second without unit conversion', () => {
		expect(formatBitrate(320)).toBe('320 kbps')
		expect(formatBitrate(128)).toBe('128 kbps')
		expect(formatBitrate(1411)).toBe('1411 kbps')
	})

	it('rounds fractional stored kilobits per second', () => {
		expect(formatBitrate(319.6)).toBe('320 kbps')
	})

	it('returns a dash for null and non-positive kbps placeholders', () => {
		expect(formatBitrate(null)).toBe('-')
		expect(formatBitrate(0)).toBe('-')
		expect(formatBitrate(-5)).toBe('-')
	})
})
