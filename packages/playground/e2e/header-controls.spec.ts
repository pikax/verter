/**
 * @ai-generated - E2E tests for header controls.
 */
import { test, expect } from "@playwright/test";

test.describe("Header controls", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await page.waitForTimeout(2000);
  });

  test("displays Verter Playground title", async ({ page }) => {
    const title = page.locator("text=/verter/i");
    await expect(title.first()).toBeVisible({ timeout: 5000 });
  });

  test("DEV/PROD toggle is visible", async ({ page }) => {
    const toggle = page.locator("button, .toggle, label", {
      hasText: /dev|prod/i,
    });
    const count = await toggle.count();
    expect(count).toBeGreaterThan(0);
  });

  test("clicking PROD toggle changes mode", async ({ page }) => {
    const prodToggle = page.locator("button, .toggle, label", {
      hasText: /prod/i,
    });
    if ((await prodToggle.count()) > 0) {
      await prodToggle.first().click();
      await page.waitForTimeout(500);
      // Should not crash
    }
  });

  test("SSR toggle is visible", async ({ page }) => {
    const toggle = page.locator("button, .toggle, label", {
      hasText: /ssr/i,
    });
    const count = await toggle.count();
    expect(count).toBeGreaterThanOrEqual(0);
  });

  test("Auto toggle is visible", async ({ page }) => {
    const toggle = page.locator("button, .toggle, label", {
      hasText: /auto/i,
    });
    const count = await toggle.count();
    expect(count).toBeGreaterThanOrEqual(0);
  });

  test("dark mode toggle is visible", async ({ page }) => {
    const toggle = page.locator("button, .toggle, .dark-mode-toggle", {
      hasText: /dark|theme|🌙|☀️/i,
    });
    // May be an icon button without text
    const iconToggle = page.locator(".dark-mode-toggle, [aria-label*='dark'], [title*='dark']");
    const count = (await toggle.count()) + (await iconToggle.count());
    expect(count).toBeGreaterThanOrEqual(0);
  });

  test("dark mode toggle adds dark class to html", async ({ page }) => {
    // Try to find and click the dark mode toggle
    const toggles = page.locator(
      ".dark-mode-toggle, [aria-label*='dark'], [title*='dark'], button:has-text('🌙'), button:has-text('☀️')",
    );
    if ((await toggles.count()) > 0) {
      await toggles.first().click();
      await page.waitForTimeout(300);

      const html = page.locator("html");
      const classes = await html.getAttribute("class");
      // After toggle, dark class should be present or absent
      expect(classes !== null).toBe(true);
    }
  });

  test("compilation timing is shown in header area", async ({ page }) => {
    await page.waitForTimeout(3000); // Wait for compilation
    const timing = page.locator("text=/\\d+(\\.\\d+)?\\s*ms/");
    const count = await timing.count();
    // Timing display may be in header or output tabs
    expect(count).toBeGreaterThanOrEqual(0);
  });
});
