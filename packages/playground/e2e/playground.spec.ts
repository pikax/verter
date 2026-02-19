/**
 * @ai-generated - E2E tests for the Verter playground.
 * Tests the full pipeline: WASM compilation → preview rendering → runtime behavior.
 * Verifies parity with Vue's official compiler behavior.
 */
import { test, expect } from "@playwright/test";
import { getPreviewFrame, filterCriticalErrors } from "./helpers";

test.describe("Playground default template", () => {
  test("should load without console errors", async ({ page }) => {
    const errors: string[] = [];
    page.on("console", (msg) => {
      if (msg.type() === "error") {
        errors.push(msg.text());
      }
    });
    page.on("pageerror", (err) => {
      errors.push(err.message);
    });

    await page.goto("/");
    await page.waitForTimeout(4000);

    expect(filterCriticalErrors(errors)).toEqual([]);
  });

  test("should render the preview with the default component", async ({
    page,
  }) => {
    await page.goto("/");
    await page.waitForTimeout(4000);

    const frame = await getPreviewFrame(page);

    const h1 = frame.locator("h1");
    await expect(h1).toBeVisible({ timeout: 5000 });
    await expect(h1).toHaveText("Hello from Verter!");

    const button = frame.locator("button");
    await expect(button).toBeVisible({ timeout: 5000 });
    await expect(button).toHaveText("Count: 0");
  });

  test("should support reactivity - clicking button increments count", async ({
    page,
  }) => {
    const errors: string[] = [];
    page.on("console", (msg) => {
      if (msg.type() === "error") {
        errors.push(msg.text());
      }
    });
    page.on("pageerror", (err) => {
      errors.push(err.message);
    });

    await page.goto("/");
    await page.waitForTimeout(4000);

    const frame = await getPreviewFrame(page);

    const button = frame.locator("button");
    await expect(button).toHaveText("Count: 0", { timeout: 5000 });

    await button.click();
    await expect(button).toHaveText("Count: 1", { timeout: 3000 });

    await button.click();
    await expect(button).toHaveText("Count: 2", { timeout: 3000 });

    expect(filterCriticalErrors(errors)).toEqual([]);
  });

  test("should not have nextSibling errors during re-render", async ({
    page,
  }) => {
    const errors: string[] = [];
    page.on("console", (msg) => {
      if (msg.type() === "error") {
        errors.push(msg.text());
      }
    });
    page.on("pageerror", (err) => {
      errors.push(err.message);
    });

    await page.goto("/");
    await page.waitForTimeout(4000);

    const frame = await getPreviewFrame(page);

    const button = frame.locator("button");
    await expect(button).toBeVisible({ timeout: 5000 });
    await button.click();
    await page.waitForTimeout(1000);

    const nextSiblingErrors = errors.filter((e) =>
      e.includes("nextSibling"),
    );
    expect(nextSiblingErrors).toEqual([]);
  });
});
