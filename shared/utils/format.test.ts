import { describe, it, expect } from 'vitest'
import { formatDuration, formatDurationCompact, formatBpm } from './format'

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
