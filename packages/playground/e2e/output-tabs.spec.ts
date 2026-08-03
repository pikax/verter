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

  test("Files tab is visible", async ({ page }) => {
    const filesTab = page.locator(".output-tabs button", { hasText: "Files" });
    await expect(filesTab).toBeVisible({ timeout: 5000 });
  });

  test("Files tab exposes the style output", async ({ page }) => {
    await page.locator(".output-tabs button", { hasText: "Files" }).click();
    await expect(page.locator(".vfile-btn", { hasText: "style[0]" })).toBeVisible();
  });

  test("clicking the script file shows compiled JavaScript", async ({ page }) => {
    await page.locator(".output-tabs button", { hasText: "Files" }).click();
    await page.locator(".vfile-btn", { hasText: "script" }).click();

    const codeOutput = page.locator(".vfiles-code .monaco-editor");
    await expect(codeOutput).toBeVisible({ timeout: 5000 });
  });

  test("clicking the style file shows compiled CSS", async ({ page }) => {
    await page.locator(".output-tabs button", { hasText: "Files" }).click();
    await page.locator(".vfile-btn", { hasText: "style[0]" }).click();

    const codeOutput = page.locator(".vfiles-code .monaco-editor");
    await expect(codeOutput).toBeVisible({ timeout: 5000 });
  });

  test("switching back to Preview shows the iframe", async ({ page }) => {
    // Switch to Files first
    const filesTab = page.locator(".output-tabs button", { hasText: "Files" });
    await filesTab.click();
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

  test("Files tab shows compilation timing", async ({ page }) => {
    const filesTab = page.locator(".output-tabs button", { hasText: /Files.*ms/ });
    await expect(filesTab).toBeVisible({ timeout: 5000 });
  });
});
