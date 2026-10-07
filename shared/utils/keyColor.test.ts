import { describe, expect, it } from 'vitest'
import * as utils from './index'
import { keyColor } from './keyColor'

/**
 * The approved palette, verbatim. Source: Mixxx `kMIKKeyColorPalette`
 * (`src/util/color/predefinedcolorpalettes.cpp`), reindexed by Camelot number.
 * Locked here as data so an accidental edit to the palette fails loudly.
 */
const APPROVED: Record<string, string> = {
	'1B': '#FD7EB3',
	'1A': '#CF6793',
	'2B': '#20EF7F',
	'2A': '#1AC468',
	'3B': '#D18BFD',
	'3A': '#AB72CF',
	'4B': '#E0CA6D',
	'4A': '#B8A659',
	'5B': '#4DD3F8',
	'5A': '#3FADCB',
	'6B': '#FF8693',
	'6A': '#D16E79',
	'7B': '#00EECB',
	'7A': '#00C3A6',
	'8B': '#F17EDB',
	'8A': '#C667B4',
	'9B': '#7FF448',
	'9A': '#68C83B',
	'10B': '#9EB4FD',
	'10A': '#8294CF',
	'11B': '#FDA078',
	'11A': '#CF8362',
	'12B': '#01EAEC',
	'12A': '#01C0C2',
}

describe('keyColor', () => {
	it('is re-exported from the shared utils barrel', () => {
		// AGENTS.md §7: a util that is not re-exported from the barrel is invisible to the app.
		expect(utils.keyColor).toBe(keyColor)
	})

	it('returns the approved colour for every Camelot code', () => {
		for (const [camelot, expected] of Object.entries(APPROVED)) {
			expect(keyColor(camelot), `${camelot} should be ${expected}`).toBe(expected)
		}
	})

	it('shares one colour family per Camelot number and separates A from B', () => {
		// Same number = harmonically compatible, so the pair must read as a family.
		for (let n = 1; n <= 12; n++) {
			expect(keyColor(`${n}A`)).not.toBe(keyColor(`${n}B`))
		}
	})

	it('resolves standard notation to the same colour as its Camelot code', () => {
		// C major is 8B in Camelot notation, the anchor of the source palette.
		expect(keyColor('C')).toBe('#F17EDB')
		expect(keyColor('Am')).toBe('#C667B4')
		expect(keyColor('F#m')).toBe('#CF8362')
		expect(keyColor('Ebm')).toBe('#1AC468')
	})

	it('resolves tagger free text to the same colour as its Camelot code', () => {
		expect(keyColor('G Minor')).toBe('#D16E79')
		expect(keyColor('A minor')).toBe('#C667B4')
	})

	it('accepts a lowercase mode letter and surrounding whitespace', () => {
		expect(keyColor('8b')).toBe('#F17EDB')
		expect(keyColor(' 12a ')).toBe('#01C0C2')
	})

	it('resolves legacy harmony m/d storage values to the same colour as their Camelot code', () => {
		// Stored 11d is harmony-Camelot 11B, 1m is 1A (NOT true OpenKey).
		expect(keyColor('11d')).toBe(keyColor('11B'))
		expect(keyColor('11d')).toBe('#FDA078')
		expect(keyColor('1m')).toBe(keyColor('1A'))
		expect(keyColor('1m')).toBe('#CF6793')
	})

	it('returns null for anything it cannot resolve instead of inventing a colour', () => {
		expect(keyColor(null)).toBeNull()
		expect(keyColor(undefined)).toBeNull()
		expect(keyColor('')).toBeNull()
		expect(keyColor('s-key')).toBeNull()
		expect(keyColor('H')).toBeNull()
		expect(keyColor('13A')).toBeNull()
		expect(keyColor('0A')).toBeNull()
	})

	it('only ever returns a 6-digit hex colour', () => {
		for (const camelot of Object.keys(APPROVED)) {
			expect(keyColor(camelot)).toMatch(/^#[0-9A-F]{6}$/)
		}
	})
})
