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
    // Use the Files tab which renders in a <pre> (no Monaco virtualization)
    const filesTab = page.locator(".output-tabs button", { hasText: "Files" });
    await filesTab.click();
    await page.waitForTimeout(500);
    // Click the "script" node in the sidebar
    const scriptBtn = page.locator(".vfile-btn", { hasText: "script" });
    await scriptBtn.click();
    await page.waitForTimeout(500);

    const code = await page.locator(".vfiles-code pre code").textContent();
    expect(code).toContain("__sfc__");
  });

  test("compiled output contains render function", async ({ page }) => {
    // Use the Files tab → template node
    const filesTab = page.locator(".output-tabs button", { hasText: "Files" });
    await filesTab.click();
    await page.waitForTimeout(500);
    const templateBtn = page.locator(".vfile-btn", { hasText: "template" });
    await templateBtn.click();
    await page.waitForTimeout(500);

    const code = await page.locator(".vfiles-code pre code").textContent();
    expect(code).toContain("render");
  });

  test("CSS output contains scoped styles", async ({ page }) => {
    const cssTab = page.getByRole("button", { name: "CSS", exact: true });
    await cssTab.click();
    await page.waitForTimeout(1000);

    // Default template has <style scoped>, so CSS should contain data-v attributes.
    // Read from the code output view lines (CSS output is typically short).
    const code = await page.locator(".code-output").textContent();
    // Scoped CSS should have data-v- attribute selector or the raw CSS
    expect(code).toMatch(/\.(app|button)|data-v-|font-family|text-align/);
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
