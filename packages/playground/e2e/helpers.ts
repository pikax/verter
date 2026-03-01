/**
 * Shared E2E test helpers for the Verter playground.
 */
import { expect, type Page } from "@playwright/test";

/** Wait for the preview iframe to be ready and return its frame */
export async function getPreviewFrame(page: Page) {
  const iframeLocator = page.locator("iframe.preview-iframe");
  await iframeLocator.waitFor({ state: "attached", timeout: 10000 });

  const frame = page.frame({ url: /^about:srcdoc/ }) ?? iframeLocator.contentFrame();
  expect(frame).not.toBeNull();
  return frame!;
}

/** Collect console errors from the preview iframe for a given duration */
export async function collectIframeErrors(
  page: Page,
  duration: number = 3000,
): Promise<string[]> {
  const errors: string[] = [];

  page.on("console", (msg) => {
    if (msg.type() === "error") {
      errors.push(msg.text());
    }
  });

  page.on("pageerror", (err) => {
    errors.push(err.message);
  });

  await page.waitForTimeout(duration);
  return errors;
}

/** Filter out non-critical errors (favicon, CORS, DevTools, CDN fetch, etc.) */
export function filterCriticalErrors(errors: string[]): string[] {
  return errors.filter(
    (e) =>
      !e.includes("favicon") &&
      !e.includes("404") &&
      !e.includes("DevTools") &&
      !e.includes("CORS") &&
      !e.includes("net::ERR_FAILED") &&
      !e.includes("Failed to fetch dynamically imported module"),
  );
}

/**
 * Read the full text content from the output Monaco editor.
 *
 * Monaco virtualizes rendering so DOM-based text extraction only captures
 * visible lines. This helper scrolls through the entire editor, collecting
 * all rendered lines to reconstruct the full content.
 */
export async function getOutputCode(page: Page): Promise<string> {
  const codeOutput = page.locator(".code-output .monaco-editor").first();
  await expect(codeOutput).toBeVisible({ timeout: 5000 });

  // Wait for lines to render
  const viewLines = codeOutput.locator(".view-lines .view-line");
  await viewLines.first().waitFor({ state: "attached", timeout: 5000 });

  // Collect all unique lines by scrolling through the editor.
  // Monaco virtualizes rendering, so we scroll and collect incrementally.
  const allLines = new Map<number, string>();
  let lastCount = 0;
  let stableIterations = 0;

  for (let i = 0; i < 20; i++) {
    const lines = await codeOutput.evaluate((el) => {
      const result: Array<[number, string]> = [];
      el.querySelectorAll(".view-lines .view-line").forEach((line) => {
        const style = (line as HTMLElement).style;
        const top = parseInt(style.top, 10) || 0;
        result.push([top, line.textContent ?? ""]);
      });
      return result;
    });

    for (const [top, text] of lines) {
      allLines.set(top, text);
    }

    if (allLines.size === lastCount) {
      stableIterations++;
      if (stableIterations >= 2) break;
    } else {
      stableIterations = 0;
      lastCount = allLines.size;
    }

    // Scroll down
    await codeOutput.click();
    await page.keyboard.press("PageDown");
    await page.waitForTimeout(200);
  }

  // Sort by vertical position and join
  const sorted = [...allLines.entries()]
    .sort(([a], [b]) => a - b)
    .map(([, text]) => text);
  return sorted.join("\n");
}

/** Add a new file via the playground's inline input */
export async function addFile(page: Page, filename: string) {
  const addButton = page.locator(".file-selector .add-btn");
  await addButton.click();

  const input = page.locator("input.new-file-input");
  await input.waitFor({ state: "visible", timeout: 3000 });
  await input.fill(filename);
  await input.press("Enter");
  await page.waitForTimeout(500);
}
