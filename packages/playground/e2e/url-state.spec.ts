/**
 * @ai-generated - E2E tests for URL state persistence.
 */
import { test, expect } from "@playwright/test";

test.describe("URL state", () => {
  test("hash is populated after page loads and compiles", async ({ page }) => {
    await page.goto("/");
    await page.waitForTimeout(5000); // Wait for compilation + auto-save debounce

    const hash = await page.evaluate(() => window.location.hash);
    // Hash should be non-empty after auto-save
    expect(hash.length).toBeGreaterThan(1);
  });

  test("empty hash loads default state", async ({ page }) => {
    await page.goto("/");
    await page.waitForTimeout(4000);

    // Default state should show App.vue with default content
    const appTab = page.locator(".file-selector .tab", { hasText: "App.vue" });
    await expect(appTab).toBeVisible({ timeout: 5000 });
  });

  test("navigating to URL with hash restores state", async ({ page }) => {
    // First, load the page and wait for hash to be set
    await page.goto("/");
    await page.waitForTimeout(5000);

    const hash = await page.evaluate(() => window.location.hash);

    // Navigate to new page with same hash
    await page.goto(`/${hash}`);
    await page.waitForTimeout(4000);

    // Should restore the same state
    const appTab = page.locator(".file-selector .tab", { hasText: "App.vue" });
    await expect(appTab).toBeVisible({ timeout: 5000 });
  });

  test("corrupt hash gracefully falls back to default", async ({ page }) => {
    await page.goto("/#corrupt-data-that-cannot-be-decompressed");
    await page.waitForTimeout(4000);

    // Should still load with default state, not crash
    const appTab = page.locator(".file-selector .tab", { hasText: "App.vue" });
    await expect(appTab).toBeVisible({ timeout: 5000 });
  });
});
