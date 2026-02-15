/**
 * @ai-generated - E2E tests for the Verter playground.
 * Tests the full pipeline: WASM compilation → preview rendering → runtime behavior.
 * Verifies parity with Vue's official compiler behavior.
 */
import { test, expect } from "@playwright/test";

// Helper: wait for the preview iframe to be ready and return its frame
async function getPreviewFrame(page: import("@playwright/test").Page) {
  // Wait for the iframe to appear
  const iframeLocator = page.locator("iframe.preview-iframe");
  await iframeLocator.waitFor({ state: "attached", timeout: 10000 });

  // Get the frame
  const frame = page.frame({ url: /^about:srcdoc/ }) ?? iframeLocator.contentFrame();
  expect(frame).not.toBeNull();
  return frame!;
}

// Helper: collect console errors from the preview iframe
async function collectIframeErrors(
  page: import("@playwright/test").Page,
  duration: number = 3000,
): Promise<string[]> {
  const errors: string[] = [];

  // Listen for console errors (forwarded from iframe via postMessage)
  page.on("console", (msg) => {
    if (msg.type() === "error") {
      errors.push(msg.text());
    }
  });

  // Also listen for page errors
  page.on("pageerror", (err) => {
    errors.push(err.message);
  });

  await page.waitForTimeout(duration);
  return errors;
}

test.describe("Playground default template", () => {
  test("should load without console errors", async ({ page }) => {
    // Collect errors from the start
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

    // Wait for compilation and preview to settle
    await page.waitForTimeout(4000);

    // Filter out non-critical errors (e.g., favicon 404)
    const criticalErrors = errors.filter(
      (e) =>
        !e.includes("favicon") &&
        !e.includes("404") &&
        !e.includes("DevTools") &&
        !e.includes("CORS") &&
        !e.includes("net::ERR_FAILED"),
    );

    expect(criticalErrors).toEqual([]);
  });

  test("should render the preview with the default component", async ({
    page,
  }) => {
    await page.goto("/");
    await page.waitForTimeout(4000);

    const frame = await getPreviewFrame(page);

    // The default template should render an h1 with "Hello from Verter!"
    const h1 = frame.locator("h1");
    await expect(h1).toBeVisible({ timeout: 5000 });
    await expect(h1).toHaveText("Hello from Verter!");

    // And a button with "Count: 0"
    const button = frame.locator("button");
    await expect(button).toBeVisible({ timeout: 5000 });
    await expect(button).toHaveText("Count: 0");
  });

  test("should support reactivity - clicking button increments count", async ({
    page,
  }) => {
    // Collect errors
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

    // Initial state
    const button = frame.locator("button");
    await expect(button).toHaveText("Count: 0", { timeout: 5000 });

    // Click the button
    await button.click();

    // Count should increment to 1
    await expect(button).toHaveText("Count: 1", { timeout: 3000 });

    // Click again
    await button.click();
    await expect(button).toHaveText("Count: 2", { timeout: 3000 });

    // No errors should have occurred during reactivity
    const criticalErrors = errors.filter(
      (e) =>
        !e.includes("favicon") &&
        !e.includes("404") &&
        !e.includes("DevTools") &&
        !e.includes("CORS") &&
        !e.includes("net::ERR_FAILED"),
    );
    expect(criticalErrors).toEqual([]);
  });

  test("should not have nextSibling errors during re-render", async ({
    page,
  }) => {
    // This specifically tests the bug: TypeError: Cannot read properties of null (reading 'nextSibling')
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

    // Click the button to trigger a re-render
    const button = frame.locator("button");
    await expect(button).toBeVisible({ timeout: 5000 });
    await button.click();
    await page.waitForTimeout(1000);

    // Check for the specific nextSibling error
    const nextSiblingErrors = errors.filter((e) =>
      e.includes("nextSibling"),
    );
    expect(nextSiblingErrors).toEqual([]);
  });
});
