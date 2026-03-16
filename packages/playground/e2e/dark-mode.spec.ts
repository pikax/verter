/**
 * @ai-generated - E2E tests for dark mode.
 */
import { test, expect } from "@playwright/test";

test.describe("Dark mode", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await page.waitForTimeout(2000);
  });

  test("toggles dark class on html element", async ({ page }) => {
    const html = page.locator("html");

    const toggles = page.locator(
      ".dark-mode-toggle, [aria-label*='dark' i], [title*='dark' i], [aria-label*='theme' i], button:has-text('🌙'), button:has-text('☀️')",
    );

    if ((await toggles.count()) > 0) {
      const hadDark = ((await html.getAttribute("class")) ?? "").includes("dark");
      await toggles.first().click();
      await page.waitForTimeout(300);

      const hasDarkNow = ((await html.getAttribute("class")) ?? "").includes("dark");
      expect(hasDarkNow).toBe(!hadDark);
    }
  });

  test("toggling twice returns to original state", async ({ page }) => {
    const html = page.locator("html");

    const toggles = page.locator(
      ".dark-mode-toggle, [aria-label*='dark' i], [title*='dark' i], [aria-label*='theme' i], button:has-text('🌙'), button:has-text('☀️')",
    );

    if ((await toggles.count()) > 0) {
      const originalClasses = (await html.getAttribute("class")) ?? "";
      await toggles.first().click();
      await page.waitForTimeout(200);
      await toggles.first().click();
      await page.waitForTimeout(200);

      const restoredClasses = (await html.getAttribute("class")) ?? "";
      expect(restoredClasses.includes("dark")).toBe(originalClasses.includes("dark"));
    }
  });

  test("respects prefers-color-scheme on load", async ({ page }) => {
    // Emulate dark color scheme preference
    await page.emulateMedia({ colorScheme: "dark" });
    await page.goto("/");
    await page.waitForTimeout(2000);

    const html = page.locator("html");
    const classes = (await html.getAttribute("class")) ?? "";
    // Should respect the preference (dark class present)
    expect(classes.includes("dark")).toBe(true);
  });

  test("respects prefers-color-scheme light on load", async ({ page }) => {
    await page.emulateMedia({ colorScheme: "light" });
    await page.goto("/");
    await page.waitForTimeout(2000);

    const html = page.locator("html");
    const classes = (await html.getAttribute("class")) ?? "";
    expect(classes.includes("dark")).toBe(false);
  });
});
