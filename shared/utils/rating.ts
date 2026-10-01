/**
 * Compute the new rating when a star is clicked in a rating widget.
 * Clicking the star that matches the current rating clears it back to 0;
 * clicking any other star selects that star's value.
 */
export function nextRatingSelection(current: number, star: number): number {
	return current === star ? 0 : star
}
