import { describe, it, expect } from 'vitest'
import { nextRatingSelection } from './rating'

describe('nextRatingSelection', () => {
	it('selects an unselected star', () => {
		expect(nextRatingSelection(0, 3)).toBe(3)
		expect(nextRatingSelection(2, 5)).toBe(5)
	})

	it('clears to 0 when clicking the currently rated star', () => {
		expect(nextRatingSelection(3, 3)).toBe(0)
		expect(nextRatingSelection(5, 5)).toBe(0)
		expect(nextRatingSelection(1, 1)).toBe(0)
	})

	it('lowers the rating when clicking a star below the current rating', () => {
		expect(nextRatingSelection(4, 2)).toBe(2)
		expect(nextRatingSelection(5, 1)).toBe(1)
	})

	it('raises the rating when clicking a star above the current rating', () => {
		expect(nextRatingSelection(2, 4)).toBe(4)
		expect(nextRatingSelection(1, 5)).toBe(5)
	})

	it('rates an unrated row (rating 0) on any star click', () => {
		expect(nextRatingSelection(0, 1)).toBe(1)
		expect(nextRatingSelection(0, 5)).toBe(5)
	})
})
