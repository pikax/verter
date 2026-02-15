import type { Page } from '@playwright/test'

/**
 * Collect console errors during test execution.
 * Returns an array that gets populated as errors occur.
 */
export function collectConsoleErrors(page: Page): string[] {
  const errors: string[] = []

  page.on('console', (msg) => {
    if (msg.type() === 'error') {
      errors.push(msg.text())
    }
  })

  page.on('pageerror', (err) => {
    errors.push(err.message)
  })

  return errors
}

/**
 * Wait for the Vue app to mount by checking for the app container.
 */
export async function waitForApp(page: Page): Promise<void> {
  await page.waitForSelector('#e2e-app', { timeout: 15000 })
  await page.waitForSelector('[data-testid="app-title"]', { timeout: 10000 })
}

/**
 * Get text content of an element by data-testid.
 */
export async function getTestIdText(page: Page, testId: string): Promise<string> {
  const el = page.getByTestId(testId)
  return (await el.textContent()) ?? ''
}

/**
 * Click an element by data-testid.
 */
export async function clickTestId(page: Page, testId: string): Promise<void> {
  await page.getByTestId(testId).click()
}

/**
 * Count elements matching a data-testid.
 */
export async function countTestId(page: Page, testId: string): Promise<number> {
  return page.getByTestId(testId).count()
}

/**
 * Filter known/acceptable console errors (e.g., HMR websocket in build mode).
 */
export function filterKnownErrors(errors: string[]): string[] {
  const ignored = [
    'WebSocket connection',
    '[vite]',
    'net::ERR_CONNECTION_REFUSED',
    // HMR-related noise in build mode
    'Failed to load resource',
  ]
  return errors.filter(
    (e) => !ignored.some((pattern) => e.includes(pattern)),
  )
}
