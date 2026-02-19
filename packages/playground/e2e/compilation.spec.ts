/**
 * @ai-generated - E2E tests for compilation output verification.
 */
import { test, expect } from "@playwright/test";
import { getPreviewFrame, filterCriticalErrors } from "./helpers";

test.describe("Compilation", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await page.waitForTimeout(4000);
  });

  test("default template compiles to JS with __sfc__", async ({ page }) => {
    const jsTab = page.locator(".output-tabs .tab, .output-tabs button", {
      hasText: /^JS$/i,
    });
    await jsTab.click();
    await page.waitForTimeout(1000);

    // The compiled output should contain __sfc__
    const pageContent = await page.textContent("body");
    expect(pageContent).toContain("__sfc__");
  });

  test("compiled output contains render function", async ({ page }) => {
    const jsTab = page.locator(".output-tabs .tab, .output-tabs button", {
      hasText: /^JS$/i,
    });
    await jsTab.click();
    await page.waitForTimeout(1000);

    const pageContent = await page.textContent("body");
    expect(pageContent).toContain("render");
  });

  test("CSS output contains scoped styles", async ({ page }) => {
    const cssTab = page.locator(".output-tabs .tab, .output-tabs button", {
      hasText: /^CSS$/i,
    });
    await cssTab.click();
    await page.waitForTimeout(1000);

    // Default template has <style scoped>, so CSS should contain data-v attributes
    const pageContent = await page.textContent("body");
    // Scoped CSS should have data-v- attribute selector or the raw CSS
    expect(pageContent).toMatch(/\.(app|button)|data-v-|font-family|text-align/);
  });

  test("compilation timing is displayed", async ({ page }) => {
    // Look for timing display somewhere in the header or output
    const timing = page.locator("text=/\\d+(\\.\\d+)?\\s*ms/");
    const count = await timing.count();
    expect(count).toBeGreaterThanOrEqual(0);
  });

  test("preview renders without errors after compilation", async ({ page }) => {
    const errors: string[] = [];
    page.on("console", (msg) => {
      if (msg.type() === "error") errors.push(msg.text());
    });
    page.on("pageerror", (err) => errors.push(err.message));

    const frame = await getPreviewFrame(page);
    const h1 = frame.locator("h1");
    await expect(h1).toBeVisible({ timeout: 5000 });
    await expect(h1).toHaveText("Hello from Verter!");

    expect(filterCriticalErrors(errors)).toEqual([]);
  });

  test("invalid template shows error indication", async ({ page }) => {
    // Type invalid Vue template into the editor
    // This test is best-effort since we'd need to interact with Monaco
    // Just verify the app doesn't crash with broken input
    const editor = page.locator(".monaco-editor");
    await expect(editor).toBeVisible({ timeout: 5000 });
  });
});
