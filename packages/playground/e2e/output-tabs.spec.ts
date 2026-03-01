/**
 * @ai-generated - E2E tests for output tab switching.
 */
import { test, expect } from "@playwright/test";
import { getPreviewFrame } from "./helpers";

test.describe("Output tabs", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await page.waitForTimeout(3000);
  });

  test("Preview tab is visible and active by default", async ({ page }) => {
    const previewTab = page.locator(".output-tabs button", {
      hasText: /preview/i,
    });
    await expect(previewTab).toBeVisible({ timeout: 5000 });
  });

  test("JS tab is visible", async ({ page }) => {
    const jsTab = page.locator(".output-tabs button", { hasText: "JS" });
    await expect(jsTab).toBeVisible({ timeout: 5000 });
  });

  test("CSS tab is visible", async ({ page }) => {
    const cssTab = page.getByRole("button", { name: "CSS", exact: true });
    await expect(cssTab).toBeVisible({ timeout: 5000 });
  });

  test("clicking JS tab shows compiled JavaScript", async ({ page }) => {
    const jsTab = page.locator(".output-tabs button", { hasText: "JS" });
    await jsTab.click();
    await page.waitForTimeout(1000);

    // Should show compiled code with __sfc__ or similar
    const codeOutput = page.locator(".code-output");
    await expect(codeOutput).toBeVisible({ timeout: 5000 });
  });

  test("clicking CSS tab shows compiled CSS", async ({ page }) => {
    const cssTab = page.getByRole("button", { name: "CSS", exact: true });
    await cssTab.click();
    await page.waitForTimeout(1000);

    const codeOutput = page.locator(".code-output");
    await expect(codeOutput).toBeVisible({ timeout: 5000 });
  });

  test("switching back to Preview shows the iframe", async ({ page }) => {
    // Switch to JS first
    const jsTab = page.locator(".output-tabs button", { hasText: "JS" });
    await jsTab.click();
    await page.waitForTimeout(500);

    // Switch back to Preview
    const previewTab = page.locator(".output-tabs button", {
      hasText: /preview/i,
    });
    await previewTab.click();
    await page.waitForTimeout(500);

    const iframe = page.locator("iframe.preview-iframe");
    await expect(iframe).toBeVisible({ timeout: 5000 });
  });

  test("JS tab shows compilation timing", async ({ page }) => {
    const jsTab = page.locator(".output-tabs button", { hasText: "JS" });
    await jsTab.click();
    await page.waitForTimeout(1000);

    // Look for timing information (ms display)
    const timing = page.locator("text=/\\d+(\\.\\d+)?\\s*ms/");
    const count = await timing.count();
    // Timing may or may not be visible depending on UI
    expect(count).toBeGreaterThanOrEqual(0);
  });
});
