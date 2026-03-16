/**
 * @ai-generated - E2E tests for the Vue version selector in the playground.
 */
import { test, expect } from "@playwright/test";

test.describe("Vue Version Selector", () => {
  test("displays the Vue version selector in the header", async ({ page }) => {
    await page.goto("/");
    await page.waitForLoadState("networkidle");

    // Vue version button should be visible with a version number
    const vueBtn = page.locator(".vue-version-btn");
    await expect(vueBtn).toBeVisible({ timeout: 5000 });
    const text = await vueBtn.textContent();
    expect(text).toMatch(/3\.\d+\.\d+/);
  });

  test("opens a dropdown with Vue versions on click", async ({ page }) => {
    await page.goto("/");
    await page.waitForLoadState("networkidle");

    const vueBtn = page.locator(".vue-version-btn");
    await vueBtn.click();

    const dropdown = page.locator(".vue-version-select .dropdown");
    await expect(dropdown).toBeVisible({ timeout: 5000 });

    // Should have at least one version item
    const items = dropdown.locator(".dropdown-item");
    expect(await items.count()).toBeGreaterThan(0);
  });

  test("closes the dropdown when clicking outside", async ({ page }) => {
    await page.goto("/");
    await page.waitForLoadState("networkidle");

    const vueBtn = page.locator(".vue-version-btn");
    await vueBtn.click();

    const dropdown = page.locator(".vue-version-select .dropdown");
    await expect(dropdown).toBeVisible();

    // Click outside
    await page.click("body", { position: { x: 10, y: 10 } });
    await expect(dropdown).not.toBeVisible();
  });

  test("highlights the currently active version", async ({ page }) => {
    await page.goto("/");
    await page.waitForLoadState("networkidle");

    const vueBtn = page.locator(".vue-version-btn");
    await vueBtn.click();

    const activeItem = page.locator(".vue-version-select .dropdown-item.active");
    await expect(activeItem).toBeVisible({ timeout: 5000 });

    // Active version should match the button text
    const btnText = await vueBtn.textContent();
    const activeText = await activeItem.textContent();
    const versionFromBtn = btnText?.match(/(\d+\.\d+\.\d+)/)?.[1];
    const versionFromActive = activeText?.match(/(\d+\.\d+\.\d+)/)?.[1];
    expect(versionFromBtn).toBe(versionFromActive);
  });
});
