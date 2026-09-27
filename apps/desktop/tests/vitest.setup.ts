import '@testing-library/jest-dom/vitest'

// eslint-disable-next-line no-var
var cleanup: Array<() => void> = []

beforeEach(() => {
	cleanup.forEach((fn) => fn())
	cleanup = []
})

export function registerCleanup(fn: () => void): void {
	cleanup.push(fn)
}
