import { render } from '@testing-library/svelte'
import { readable } from 'svelte/store'
import { describe, expect, it, vi } from 'vitest'

// jsdom has no Tauri IPC, so `convertFileSrc` is stubbed with the URL shape Tauri produces on
// Linux/macOS: `asset://localhost/<percent-encoded absolute path>`.
vi.mock('@tauri-apps/api/core', () => ({
	convertFileSrc: (path: string) => `asset://localhost/${encodeURIComponent(path)}`,
}))

vi.mock('$lib/stores/app', () => ({
	appDataDir: readable('/data/app'),
}))

import AlbumArt from './AlbumArt.svelte'

describe('AlbumArt', () => {
	/**
	 * Regression: on Linux/WebKitGTK an `<img>` pointing at the `asset://` custom scheme is
	 * rejected as a plain (no-CORS) subresource — the request never even reaches the Rust
	 * handler — so every cover fell back to the vinyl placeholder. The handler already answers
	 * with `Access-Control-Allow-Origin`, so requesting the local asset in CORS mode makes it
	 * load. Tauri's own docs describe the same requirement for `convertFileSrc` on Linux.
	 */
	it('loads the local asset through CORS so WebKitGTK can decode it', () => {
		const { container } = render(AlbumArt, { props: { artworkPath: 'artwork/track-abc.webp' } })

		const img = container.querySelector('img')
		expect(img).not.toBeNull()
		expect(img!.getAttribute('src')).toContain('asset://localhost/')
		expect(img!.getAttribute('crossorigin')).toBe('anonymous')
	})

	/**
	 * A remote cover URL is served by a third party. Forcing CORS there would make the image
	 * fail whenever that server omits `Access-Control-Allow-Origin`, so the attribute must only
	 * accompany the local asset URL.
	 */
	it('does not force CORS on a remote artwork URL', () => {
		const { container } = render(AlbumArt, {
			props: { artworkPath: null, artworkUrl: 'https://cdn.example/cover.jpg' },
		})

		const img = container.querySelector('img')
		expect(img).not.toBeNull()
		expect(img!.getAttribute('src')).toBe('https://cdn.example/cover.jpg')
		expect(img!.hasAttribute('crossorigin')).toBe(false)
	})

	/** Without artwork at all the vinyl placeholder icon renders instead of an `<img>`. */
	it('falls back to the placeholder when there is no artwork', () => {
		const { container } = render(AlbumArt, { props: { artworkPath: null } })

		expect(container.querySelector('img')).toBeNull()
	})
})
