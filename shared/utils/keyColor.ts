import { keyToCamelot } from './format'

/**
 * Key colours indexed by Camelot number, each entry the major (B) and minor (A) variant.
 *
 * Source: Mixxx `kMIKKeyColorPalette` (`src/util/color/predefinedcolorpalettes.cpp`), whose
 * comment anchors `C Major is 8B in Camelot Notation`. The published palette is indexed
 * chromatically, so it is reindexed here by Camelot number: this keeps a number and its
 * relative in one colour family, which is what the wheel communicates (same number =
 * harmonically compatible). The minor variant is the major colour darkened by 18%.
 */
const KEY_COLORS: Record<number, { major: string; minor: string }> = {
	1: { major: '#FD7EB3', minor: '#CF6793' },
	2: { major: '#20EF7F', minor: '#1AC468' },
	3: { major: '#D18BFD', minor: '#AB72CF' },
	4: { major: '#E0CA6D', minor: '#B8A659' },
	5: { major: '#4DD3F8', minor: '#3FADCB' },
	6: { major: '#FF8693', minor: '#D16E79' },
	7: { major: '#00EECB', minor: '#00C3A6' },
	8: { major: '#F17EDB', minor: '#C667B4' },
	9: { major: '#7FF448', minor: '#68C83B' },
	10: { major: '#9EB4FD', minor: '#8294CF' },
	11: { major: '#FDA078', minor: '#CF8362' },
	12: { major: '#01EAEC', minor: '#01C0C2' },
}

const CAMELOT_COLOR_PATTERN = /^(\d{1,2})([AB])$/

/**
 * Colour for a musical key, in whatever notation it is stored.
 *
 * Accepts everything `keyToCamelot` understands, so standard notation ("Am"), Camelot
 * ("8A") and tagger free text ("G Minor") all colour consistently. Returns `null` for a
 * key that cannot be resolved: the caller keeps its plain rendering rather than showing an
 * invented colour for an unknown key.
 */
export function keyColor(key: string | null | undefined): string | null {
	const camelot = keyToCamelot(key)
	if (!camelot) return null

	const match = camelot.match(CAMELOT_COLOR_PATTERN)
	if (!match) return null

	const color = KEY_COLORS[Number(match[1])]
	if (!color) return null

	return match[2] === 'A' ? color.minor : color.major
}
