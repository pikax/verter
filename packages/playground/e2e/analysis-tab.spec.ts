/**
 * @ai-generated - E2E tests for the Analysis tab in the playground.
 * Verifies that clicking the Analysis tab renders analysis sections
 * without crashing (regression test for camelCase serialization bug).
 */
import { test, expect } from "@playwright/test";

test.describe("Analysis tab", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await page.waitForTimeout(3000);
  });

  test("clicking Analysis tab renders without errors", async ({ page }) => {
    // Collect any page errors during the test
    const errors: string[] = [];
    page.on("pageerror", (err) => errors.push(err.message));

    const analysisTab = page.locator(".output-tabs button", {
      hasText: "Analysis",
    });
    await expect(analysisTab).toBeVisible({ timeout: 5000 });
    await analysisTab.click();
    await page.waitForTimeout(1000);

    // The analysis panel should be visible (either with content or empty state)
    const panel = page.locator(".analysis-panel");
    await expect(panel).toBeVisible({ timeout: 5000 });

    // Must not have the "Cannot read properties of undefined" crash
    const criticalErrors = errors.filter(
      (e) => e.includes("Cannot read properties of undefined") || e.includes("reading 'length'"),
    );
    expect(criticalErrors).toEqual([]);
  });

  test("Analysis tab shows analysis content for default App.vue", async ({ page }) => {
    const analysisTab = page.locator(".output-tabs button", {
      hasText: "Analysis",
    });
    await analysisTab.click();
    await page.waitForTimeout(1000);

    const panel = page.locator(".analysis-panel");
    await expect(panel).toBeVisible({ timeout: 5000 });

    // The default App.vue has imports, bindings, macros, and styles —
    // so analysis-content (not empty-state) should render.
    const content = page.locator(".analysis-content");
    await expect(content).toBeVisible({ timeout: 5000 });
  });

  test("Analysis tab shows timing information", async ({ page }) => {
    const analysisTab = page.locator(".output-tabs button", {
      hasText: "Analysis",
    });
    await analysisTab.click();
    await page.waitForTimeout(1000);

    // The timing section should show parse duration
    const timingRow = page.locator(".timing-row");
    await expect(timingRow).toBeVisible({ timeout: 5000 });
  });

  test("Analysis tab shows imports section for default App.vue", async ({ page }) => {
    const analysisTab = page.locator(".output-tabs button", {
      hasText: "Analysis",
    });
    await analysisTab.click();
    await page.waitForTimeout(1000);

    // Default App.vue imports { ref } from 'vue', so Imports section should exist
    const importsSection = page.locator("details.analysis-section", {
      hasText: "Imports",
    });
    await expect(importsSection).toBeVisible({ timeout: 5000 });
  });

  test("Analysis tab shows bindings section for default App.vue", async ({ page }) => {
    const analysisTab = page.locator(".output-tabs button", {
      hasText: "Analysis",
    });
    await analysisTab.click();
    await page.waitForTimeout(1000);

    // Default App.vue has const count = ref(0), const message = ref('...'),
    // function increment — so Bindings section should exist
    const bindingsSection = page.locator("details.analysis-section", {
      hasText: "Bindings",
    });
    await expect(bindingsSection).toBeVisible({ timeout: 5000 });
  });

  test("Analysis tab shows styles section for default App.vue", async ({ page }) => {
    const analysisTab = page.locator(".output-tabs button", {
      hasText: "Analysis",
    });
    await analysisTab.click();
    await page.waitForTimeout(1000);

    // Default App.vue has <style scoped>, so Styles section should exist
    const stylesSection = page.locator("details.analysis-section", {
      hasText: "Styles",
    });
    await expect(stylesSection).toBeVisible({ timeout: 5000 });
  });
});
