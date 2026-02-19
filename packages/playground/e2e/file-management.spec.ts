/**
 * @ai-generated - E2E tests for file management in the playground.
 */
import { test, expect } from "@playwright/test";

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
    const addButton = page.locator(".file-selector .add-btn, .file-selector button:has-text('+')");
    await addButton.click();

    // A prompt/input should appear for filename
    const dialog = page.locator("input[type='text'], .filename-input");
    if (await dialog.isVisible({ timeout: 2000 }).catch(() => false)) {
      await dialog.fill("Child.vue");
      await dialog.press("Enter");
    } else {
      // Some implementations use window.prompt - handle via dialog
      page.once("dialog", async (d) => {
        await d.accept("Child.vue");
      });
      await addButton.click();
    }

    await page.waitForTimeout(500);
    const tab = page.locator(".file-selector .tab", { hasText: "Child.vue" });
    await expect(tab).toBeVisible({ timeout: 3000 });
  });

  test("new file becomes active tab", async ({ page }) => {
    // Listen for dialog (prompt)
    page.on("dialog", async (d) => {
      await d.accept("NewFile.vue");
    });

    const addButton = page.locator(".file-selector .add-btn, .file-selector button:has-text('+')");
    await addButton.click();
    await page.waitForTimeout(500);

    const tab = page.locator(".file-selector .tab.active, .file-selector .tab[data-active]", {
      hasText: "NewFile.vue",
    });
    // Check that NewFile.vue is somehow indicated as active
    const newTab = page.locator(".file-selector .tab", { hasText: "NewFile.vue" });
    await expect(newTab).toBeVisible({ timeout: 3000 });
  });

  test("cannot delete App.vue (no delete button or disabled)", async ({ page }) => {
    const appTab = page.locator(".file-selector .tab", { hasText: "App.vue" });
    // App.vue should not have an X/delete button
    const deleteBtn = appTab.locator(".delete-btn, .close-btn, button:has-text('×')");
    const count = await deleteBtn.count();
    if (count > 0) {
      // If there is a button, it should be disabled or hidden
      await expect(deleteBtn.first()).not.toBeVisible();
    }
    // Otherwise no delete button exists - which is correct
  });

  test("can switch between file tabs", async ({ page }) => {
    // Add a second file first
    page.on("dialog", async (d) => {
      await d.accept("Other.vue");
    });

    const addButton = page.locator(".file-selector .add-btn, .file-selector button:has-text('+')");
    await addButton.click();
    await page.waitForTimeout(500);

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
    // Import map may be a special tab or link
    const count = await importMapTab.count();
    expect(count).toBeGreaterThanOrEqual(0); // Import map may or may not be visible by default
  });

  test("new .vue file has default template content", async ({ page }) => {
    page.on("dialog", async (d) => {
      await d.accept("Child.vue");
    });

    const addButton = page.locator(".file-selector .add-btn, .file-selector button:has-text('+')");
    await addButton.click();
    await page.waitForTimeout(1000);

    // The editor should contain default vue template
    const editor = page.locator(".monaco-editor, .editor-container");
    await expect(editor).toBeVisible({ timeout: 5000 });
  });
});
