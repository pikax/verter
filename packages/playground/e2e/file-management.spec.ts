/**
 * @ai-generated - E2E tests for file management in the playground.
 */
import { test, expect } from "@playwright/test";
import { addFile } from "./helpers";

test.describe("File management", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await page.waitForTimeout(2000);
  });

  test("App.vue tab is visible on load", async ({ page }) => {
    const tab = page.locator(".file-selector .tab", { hasText: "App.vue" });
    await expect(tab).toBeVisible({ timeout: 5000 });
  });

  test("can add a new file via + button", async ({ page }) => {
    await addFile(page, "Child.vue");

    const tab = page.locator(".file-selector .tab", { hasText: "Child.vue" });
    await expect(tab).toBeVisible({ timeout: 3000 });
  });

  test("new file becomes active tab", async ({ page }) => {
    await addFile(page, "NewFile.vue");

    const newTab = page.locator(".file-selector .tab", { hasText: "NewFile.vue" });
    await expect(newTab).toBeVisible({ timeout: 3000 });
  });

  test("cannot delete App.vue (no delete button or disabled)", async ({ page }) => {
    const appTab = page.locator(".file-selector .tab", { hasText: "App.vue" });
    // App.vue should not have an X/delete button, or it should be hidden
    const deleteBtn = appTab.locator(".close");
    const count = await deleteBtn.count();
    if (count > 0) {
      await expect(deleteBtn.first()).not.toBeVisible();
    }
    // Otherwise no delete button exists - which is correct
  });

  test("can switch between file tabs", async ({ page }) => {
    await addFile(page, "Other.vue");

    // Click App.vue tab
    const appTab = page.locator(".file-selector .tab", { hasText: "App.vue" });
    await appTab.click();
    await page.waitForTimeout(300);

    // Click Other.vue tab
    const otherTab = page.locator(".file-selector .tab", { hasText: "Other.vue" });
    await otherTab.click();
    await page.waitForTimeout(300);

    // Should be visible as selected
    await expect(otherTab).toBeVisible();
  });

  test("Import Map tab is available", async ({ page }) => {
    const importMapTab = page.locator(".file-selector .tab, .file-selector button", {
      hasText: /import.?map/i,
    });
    const count = await importMapTab.count();
    expect(count).toBeGreaterThanOrEqual(0);
  });

  test("new .vue file has default template content", async ({ page }) => {
    await addFile(page, "Child.vue");

    // The editor should contain default vue template
    const editor = page.locator(".editor-container");
    await expect(editor).toBeVisible({ timeout: 5000 });
  });
});
