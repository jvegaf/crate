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

	it('resolves legacy harmony m/d values to Camelot, case-insensitively', () => {
		// Stored values only: 11d = Camelot 11B, 1m = Camelot 1A (harmony convention,
		// NOT true OpenKey — no rotation applies).
		expect(keyToCamelot('11d')).toBe('11B')
		expect(keyToCamelot('1m')).toBe('1A')
		expect(keyToCamelot('11D')).toBe('11B')
		expect(keyToCamelot('1M')).toBe('1A')
		// Negative anchor: 4d is harmony-Camelot 4B, NOT true-OpenKey 11B.
		expect(keyToCamelot('4d')).toBe('4B')
	})

	it('resolves unicode accidentals written by external taggers', () => {
		expect(keyToCamelot('G♯ Minor')).toBe('1A')
		expect(keyToCamelot('G♭')).toBe('2B')
		expect(keyToCamelot('G♭ major')).toBe('2B')
	})

	it('returns null for anything it cannot resolve instead of guessing', () => {
		expect(keyToCamelot('Am')).toBe('8A')
		expect(keyToCamelot('8A')).toBe('8A')
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

	it('converts Camelot to true OpenKey for display', () => {
		expect(formatKey('8A', 'openkey')).toBe('1m')
		expect(formatKey('11B', 'openkey')).toBe('4d')
		expect(formatKey('1A', 'openkey')).toBe('6m')
		expect(formatKey('12B', 'openkey')).toBe('5d')
		expect(formatKey('8B', 'openkey')).toBe('1d')
	})

	it('resolves legacy harmony storage values before rotating to OpenKey', () => {
		// Stored 11d is Camelot 11B first; only then does it rotate to OpenKey 4d.
		expect(formatKey('11d', 'openkey')).toBe('4d')
	})

	/**
	 * All 24 rows of the published OpenKey/Camelot chart, transcribed verbatim as data.
	 * Expected OpenKey codes must come from this table, never be derived from the
	 * rotation formula — the table is the independent ground truth.
	 */
	const PUBLISHED_OPENKEY_TABLE: ReadonlyArray<readonly [camelot: string, openkey: string]> = [
		['1A', '6m'],
		['1B', '6d'],
		['2A', '7m'],
		['2B', '7d'],
		['3A', '8m'],
		['3B', '8d'],
		['4A', '9m'],
		['4B', '9d'],
		['5A', '10m'],
		['5B', '10d'],
		['6A', '11m'],
		['6B', '11d'],
		['7A', '12m'],
		['7B', '12d'],
		['8A', '1m'],
		['8B', '1d'],
		['9A', '2m'],
		['9B', '2d'],
		['10A', '3m'],
		['10B', '3d'],
		['11A', '4m'],
		['11B', '4d'],
		['12A', '5m'],
		['12B', '5d'],
	]

	it('matches the published OpenKey table for all 24 keys and the rotation is bijective', () => {
		for (const [camelot, openkey] of PUBLISHED_OPENKEY_TABLE) {
			expect(formatKey(camelot, 'openkey'), `${camelot} should display as ${openkey}`).toBe(openkey)

			// Invert openKeyNumber = ((camelotNumber + 4) % 12) + 1: camelotNumber = ((openKeyNumber + 6) % 12) + 1.
			const openKeyNumber = Number(openkey.slice(0, -1))
			const restored = `${((openKeyNumber + 6) % 12) + 1}${openkey.endsWith('m') ? 'A' : 'B'}`
			expect(restored, `${openkey} should restore to ${camelot}`).toBe(camelot)
		}
	})

	it('legacy-read shift: OpenKey display values read as stored m/d land on a different Camelot key', () => {
		// Executable form of the reason storage must never hold OpenKey: keyToCamelot
		// resolves m/d as the legacy harmony convention (no rotation), so an OpenKey
		// display value read back as stored data lands on a DIFFERENT Camelot key
		// (e.g. 8A → '1m' → '1A', not '8A'; 11B → '4d' → '4B', not '11B'). Storing
		// OpenKey would make data ambiguous and the bulk migration non-idempotent.
		const shifted: string[] = []
		for (const [camelot] of PUBLISHED_OPENKEY_TABLE) {
			const camelotNumber = Number(camelot.slice(0, -1))
			const expectedShift = `${((camelotNumber + 4) % 12) + 1}${camelot.endsWith('A') ? 'A' : 'B'}`
			const readBack = keyToCamelot(formatKey(camelot, 'openkey')) ?? ''
			expect(readBack, `${camelot} read as legacy harmony should shift to ${expectedShift}`).toBe(expectedShift)
			expect(readBack, `${camelot} must not read back as itself`).not.toBe(camelot)
			shifted.push(readBack)
		}

		// A permutation, not a collision: all 24 shifted keys are distinct.
		expect(new Set(shifted).size).toBe(24)
	})

	it('passes unknown values through untouched and renders null as a dash', () => {
		expect(formatKey(null)).toBe('-')
		expect(formatKey(null, 'openkey')).toBe('-')
		expect(formatKey('s-key')).toBe('s-key')
		expect(formatKey('s-key', 'standard')).toBe('s-key')
		expect(formatKey('s-key', 'openkey')).toBe('s-key')
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
