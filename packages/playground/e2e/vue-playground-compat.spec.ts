/**
 * @ai-generated - E2E tests for Vue playground URL compatibility.
 * Verifies that fflate-encoded hashes (matching play.vuejs.org format)
 * can be loaded by the Verter playground.
 */
import { test, expect } from "@playwright/test";
import { zlibSync, strToU8, strFromU8, unzlibSync } from "fflate";

/** Encode a flat object to a hash using the shared fflate zlib + base64 format. */
function encodeFlat(obj: Record<string, string>): string {
  const json = JSON.stringify(obj);
  const compressed = zlibSync(strToU8(json), { level: 9 });
  return btoa(strFromU8(compressed, true));
}

/** Decode a hash back to a flat object. */
function decodeHash(hash: string): Record<string, string> {
  const binary = atob(hash);
  const bytes = Uint8Array.from(binary, (c) => c.charCodeAt(0));
  const json = strFromU8(unzlibSync(bytes));
  return JSON.parse(json);
}

test.describe("Vue playground URL compatibility", () => {
  test("loads Vue-encoded hash with _version and multiple files", async ({ page }) => {
    const flat: Record<string, string> = {
      "App.vue":
        '<script setup>\nimport { ref } from "vue"\nconst msg = ref("hello")\n</script>\n<template>{{ msg }}</template>',
      "Child.vue": "<template><div>child</div></template>",
      _version: "3.5.26",
    };
    const hash = encodeFlat(flat);

    await page.goto(`/#${hash}`);
    await page.waitForTimeout(4000);

    // App.vue tab should be visible
    const appTab = page.locator(".file-selector .tab", { hasText: "App.vue" });
    await expect(appTab).toBeVisible({ timeout: 5000 });

    // Child.vue tab should also be visible
    const childTab = page.locator(".file-selector .tab", { hasText: "Child.vue" });
    await expect(childTab).toBeVisible({ timeout: 5000 });
  });

  test("loads Verter-encoded hash with metadata", async ({ page }) => {
    const flat: Record<string, string> = {
      "App.vue": "<template><h1>verter test</h1></template>",
      _outputMode: "js",
    };
    const hash = encodeFlat(flat);

    await page.goto(`/#${hash}`);
    await page.waitForTimeout(4000);

    // App.vue tab should be visible
    const appTab = page.locator(".file-selector .tab", { hasText: "App.vue" });
    await expect(appTab).toBeVisible({ timeout: 5000 });

    // JS output tab should be active (since _outputMode: "js")
    const jsTab = page.locator(".output-tabs .output-tab.active", { hasText: "JS" });
    await expect(jsTab).toBeVisible({ timeout: 5000 });
  });

  test("roundtrip: load hash → hash updates → reload → same files", async ({ page }) => {
    const flat: Record<string, string> = {
      "App.vue": "<template><p>roundtrip</p></template>",
      "Helper.vue": "<template><span>helper</span></template>",
    };
    const hash = encodeFlat(flat);

    await page.goto(`/#${hash}`);
    await page.waitForTimeout(4000);

    // Trigger state change to force hash update (toggle DEV/PROD)
    const devProdToggle = page.locator("button.toggle-btn", {
      hasText: /DEV|PROD/,
    });
    await devProdToggle.click();
    await page.waitForTimeout(2000);

    // Read the updated hash
    const newHash = await page.evaluate(() => window.location.hash.slice(1));
    expect(newHash.length).toBeGreaterThan(1);

    // Decode and verify files are still present
    const decoded = decodeHash(newHash);
    expect(decoded["App.vue"]).toBe("<template><p>roundtrip</p></template>");
    expect(decoded["Helper.vue"]).toBe("<template><span>helper</span></template>");

    // Reload with the new hash and verify state persists
    await page.goto(`/#${newHash}`);
    await page.waitForTimeout(4000);

    const appTab = page.locator(".file-selector .tab", { hasText: "App.vue" });
    await expect(appTab).toBeVisible({ timeout: 5000 });

    const helperTab = page.locator(".file-selector .tab", { hasText: "Helper.vue" });
    await expect(helperTab).toBeVisible({ timeout: 5000 });
  });
});
