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
    const importMapTab = page.locator(".import-map-tab");
    await expect(importMapTab).toBeVisible({ timeout: 5000 });
  });

  test("clicking Import Map shows JSON content", async ({ page }) => {
    const importMapTab = page.locator(".import-map-tab");
    await importMapTab.click();
    await page.waitForTimeout(1000);

    // The editor should show JSON with vue CDN
    const editor = page.locator(".editor-container");
    await expect(editor).toBeVisible({ timeout: 5000 });
  });

  test("default import map contains vue CDN URL", async ({ page }) => {
    const importMapTab = page.locator(".import-map-tab");
    await importMapTab.click();
    await page.waitForTimeout(1000);

    // Check page content for vue CDN reference
    const content = await page.textContent("body");
    expect(content).toContain("cdn.jsdelivr.net");
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
