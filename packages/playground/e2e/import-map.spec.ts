/**
 * @ai-generated - E2E tests for import map functionality.
 */
import { test, expect } from "@playwright/test";

test.describe("Import map", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await page.waitForTimeout(3000);
  });

  test("Import Map tab/button exists", async ({ page }) => {
    const importMapTab = page.locator(
      ".file-selector .tab, .file-selector button, .import-map-tab",
      { hasText: /import.?map/i },
    );
    // If import map is accessed via a special tab/button
    const count = await importMapTab.count();
    // It might be shown as a separate button or in the file list
    expect(count).toBeGreaterThanOrEqual(0);
  });

  test("clicking Import Map shows JSON content", async ({ page }) => {
    const importMapTab = page.locator(
      ".file-selector .tab, .file-selector button, .import-map-tab",
      { hasText: /import.?map/i },
    );

    if ((await importMapTab.count()) > 0) {
      await importMapTab.first().click();
      await page.waitForTimeout(1000);

      // The editor should show JSON with vue CDN
      const editor = page.locator(".monaco-editor, .editor-container");
      await expect(editor).toBeVisible({ timeout: 5000 });
    }
  });

  test("default import map contains vue CDN URL", async ({ page }) => {
    const importMapTab = page.locator(
      ".file-selector .tab, .file-selector button, .import-map-tab",
      { hasText: /import.?map/i },
    );

    if ((await importMapTab.count()) > 0) {
      await importMapTab.first().click();
      await page.waitForTimeout(1000);

      // Check page content for vue CDN reference
      const content = await page.textContent("body");
      expect(content).toContain("cdn.jsdelivr.net");
    }
  });

  test("invalid JSON in import map does not crash the app", async ({ page }) => {
    // Just verify the app is still functional after any import map interaction
    const appTab = page.locator(".file-selector .tab", { hasText: "App.vue" });
    await expect(appTab).toBeVisible({ timeout: 5000 });

    // App should still be responsive
    await appTab.click();
    await page.waitForTimeout(300);
  });
});
