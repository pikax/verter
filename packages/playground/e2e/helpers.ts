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

/** Filter out non-critical errors (favicon, CORS, DevTools, etc.) */
export function filterCriticalErrors(errors: string[]): string[] {
  return errors.filter(
    (e) =>
      !e.includes("favicon") &&
      !e.includes("404") &&
      !e.includes("DevTools") &&
      !e.includes("CORS") &&
      !e.includes("net::ERR_FAILED"),
  );
}
