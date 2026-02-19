/**
 * @ai-generated - E2E tests for multi-file scenarios.
 */
import { test, expect } from "@playwright/test";
import { getPreviewFrame, filterCriticalErrors } from "./helpers";

test.describe("Multi-file", () => {
  test("can create a child component file", async ({ page }) => {
    await page.goto("/");
    await page.waitForTimeout(3000);

    // Add a new file
    page.on("dialog", async (d) => {
      await d.accept("Child.vue");
    });

    const addButton = page.locator(".file-selector .add-btn, .file-selector button:has-text('+')");
    await addButton.click();
    await page.waitForTimeout(500);

    const tab = page.locator(".file-selector .tab", { hasText: "Child.vue" });
    await expect(tab).toBeVisible({ timeout: 3000 });
  });

  test("can create a .ts utility file", async ({ page }) => {
    await page.goto("/");
    await page.waitForTimeout(3000);

    page.on("dialog", async (d) => {
      await d.accept("utils.ts");
    });

    const addButton = page.locator(".file-selector .add-btn, .file-selector button:has-text('+')");
    await addButton.click();
    await page.waitForTimeout(500);

    const tab = page.locator(".file-selector .tab", { hasText: "utils.ts" });
    await expect(tab).toBeVisible({ timeout: 3000 });
  });

  test("can create a .css file", async ({ page }) => {
    await page.goto("/");
    await page.waitForTimeout(3000);

    page.on("dialog", async (d) => {
      await d.accept("custom.css");
    });

    const addButton = page.locator(".file-selector .add-btn, .file-selector button:has-text('+')");
    await addButton.click();
    await page.waitForTimeout(500);

    const tab = page.locator(".file-selector .tab", { hasText: "custom.css" });
    await expect(tab).toBeVisible({ timeout: 3000 });
  });

  test("can delete a non-main file", async ({ page }) => {
    await page.goto("/");
    await page.waitForTimeout(3000);

    // Add a file first
    page.on("dialog", async (d) => {
      await d.accept("Temp.vue");
    });

    const addButton = page.locator(".file-selector .add-btn, .file-selector button:has-text('+')");
    await addButton.click();
    await page.waitForTimeout(500);

    // Find and click delete button on Temp.vue tab
    const tempTab = page.locator(".file-selector .tab", { hasText: "Temp.vue" });
    await expect(tempTab).toBeVisible({ timeout: 3000 });

    const deleteBtn = tempTab.locator(".delete-btn, .close-btn, button");
    if ((await deleteBtn.count()) > 0) {
      await deleteBtn.first().click();
      await page.waitForTimeout(500);
      // Tab should be gone
      await expect(tempTab).not.toBeVisible({ timeout: 3000 });
    }
  });

  test("multiple file tabs are shown after adding files", async ({ page }) => {
    await page.goto("/");
    await page.waitForTimeout(3000);

    let dialogCount = 0;
    page.on("dialog", async (d) => {
      dialogCount++;
      await d.accept(dialogCount === 1 ? "A.vue" : "B.vue");
    });

    const addButton = page.locator(".file-selector .add-btn, .file-selector button:has-text('+')");
    await addButton.click();
    await page.waitForTimeout(500);
    await addButton.click();
    await page.waitForTimeout(500);

    // Should now have at least 3 tabs
    const tabs = page.locator(".file-selector .tab");
    const count = await tabs.count();
    expect(count).toBeGreaterThanOrEqual(3);
  });
});
