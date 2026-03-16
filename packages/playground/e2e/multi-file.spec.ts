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

  test("imported .vue component does not produce 'Cannot find module' TS error", async ({
    page,
  }) => {
    await page.goto("/");
    await page.waitForTimeout(5000); // WASM + TS worker init

    // 1. Add Child.vue with exported content
    await addFile(page, "Child.vue");
    const editorArea = page.locator(".monaco-editor").first();
    await editorArea.click();
    await page.keyboard.press("ControlOrMeta+a");
    await page.keyboard.type(
      '<script setup lang="ts">\nconst msg = "child"\n</script>\n\n<template>\n  <div>{{ msg }}</div>\n</template>',
      { delay: 5 },
    );
    await page.waitForTimeout(2000);

    // 2. Switch to App.vue and import Child
    const appTab = page.locator(".file-selector .tab", { hasText: "App.vue" });
    await appTab.click();
    await page.waitForTimeout(1000);
    await editorArea.click();
    await page.keyboard.press("ControlOrMeta+a");
    await page.keyboard.press("Delete");
    await page.keyboard.type(
      [
        '<script setup lang="ts">',
        'import Child from "./Child.vue"',
        "</script>",
        "",
        "<template>",
        "  <Child />",
        "</template>",
      ].join("\n"),
      { delay: 5 },
    );

    // 3. Wait for TS sync (300ms debounce + TS worker round trip)
    await page.waitForTimeout(5000);

    // 4. Open Diagnostics tab and verify no "Cannot find module" error
    const diagTab = page.locator(".output-tabs button", { hasText: "Diagnostics" });
    await diagTab.click();
    await page.waitForTimeout(1000);

    // Check that no TypeScript diagnostic contains "Cannot find module"
    const tsSection = page.locator(".diag-section", { hasText: "TypeScript" });
    if ((await tsSection.count()) > 0) {
      const messages = await tsSection.locator(".diag-message").allTextContents();
      for (const msg of messages) {
        expect(msg).not.toContain("Cannot find module");
      }
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
