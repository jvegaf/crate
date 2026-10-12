import { defineConfig } from 'vitest/config'
import tsconfigPaths from 'vite-tsconfig-paths'
import { svelte } from '@sveltejs/vite-plugin-svelte'

export default defineConfig({
	plugins: [svelte(), tsconfigPaths()],
	// Component tests need Svelte's *client* entry: without this condition Vitest resolves the
	// server build and every `mount(...)` throws lifecycle_function_unavailable.
	resolve: {
		conditions: ['browser'],
	},
	test: {
		globals: true,
		environment: 'jsdom',
		include: ['apps/desktop/src/**/*.{test,spec}.{ts,svelte}', 'shared/**/*.test.ts'],
		setupFiles: ['./apps/desktop/tests/vitest.setup.ts'],
	},
})
