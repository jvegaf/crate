import { defineConfig, devices } from '@playwright/test'

export default defineConfig({
	testDir: './apps/desktop/tests/e2e',
	timeout: 30 * 1000,
	expect: { timeout: 10000 },
	fullyParallel: false,
	forbidOnly: !!process.env.CI,
	retries: process.env.CI ? 1 : 0,
	workers: process.env.CI ? 1 : undefined,
	reporter: 'html',
	use: {
		baseURL: 'http://localhost:1420',
		trace: 'on-first-retry',
	},
	projects: [
		{
			name: 'chromium',
			use: { ...devices['Desktop Chrome'] },
		},
	],
	webServer: {
		command: 'cd apps/desktop && yarn dev:vite',
		port: 1420,
		reuseExistingServer: !process.env.CI,
		timeout: 60 * 1000,
	},
})
