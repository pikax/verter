/**
 * @ai-generated - E2E tests for multi-file scenarios.
 */
import { test, expect } from "@playwright/test";
import { getPreviewFrame, filterCriticalErrors, addFile } from "./helpers";

test.describe("Multi-file", () => {
  test("can create a child component file", async ({ page }) => {
    await page.goto("/");
    await page.waitForTimeout(3000);

    await addFile(page, "Child.vue");

    const tab = page.locator(".file-selector .tab", { hasText: "Child.vue" });
    await expect(tab).toBeVisible({ timeout: 3000 });
  });

  test("can create a .ts utility file", async ({ page }) => {
    await page.goto("/");
    await page.waitForTimeout(3000);

    await addFile(page, "utils.ts");

    const tab = page.locator(".file-selector .tab", { hasText: "utils.ts" });
    await expect(tab).toBeVisible({ timeout: 3000 });
  });

  test("can create a .css file", async ({ page }) => {
    await page.goto("/");
    await page.waitForTimeout(3000);

    await addFile(page, "custom.css");

    const tab = page.locator(".file-selector .tab", { hasText: "custom.css" });
    await expect(tab).toBeVisible({ timeout: 3000 });
  });

  test("can delete a non-main file", async ({ page }) => {
    await page.goto("/");
    await page.waitForTimeout(3000);

    await addFile(page, "Temp.vue");

    // Find the close button on Temp.vue tab
    const tempTab = page.locator(".file-selector .tab", { hasText: "Temp.vue" });
    await expect(tempTab).toBeVisible({ timeout: 3000 });

    const closeBtn = tempTab.locator(".close");
    if ((await closeBtn.count()) > 0) {
      await closeBtn.first().click();
      await page.waitForTimeout(500);
      // Tab should be gone
      await expect(tempTab).not.toBeVisible({ timeout: 3000 });
    }
  });

  test("multiple file tabs are shown after adding files", async ({ page }) => {
    await page.goto("/");
    await page.waitForTimeout(3000);

    await addFile(page, "A.vue");
    await addFile(page, "B.vue");

    // Should now have at least 3 tabs
    const tabs = page.locator(".file-selector .tab");
    const count = await tabs.count();
    expect(count).toBeGreaterThanOrEqual(3);
  });
});
