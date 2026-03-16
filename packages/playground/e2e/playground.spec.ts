/**
 * @ai-generated - E2E tests for the Verter playground.
 * Tests the full pipeline: WASM compilation → preview rendering → runtime behavior.
 * Verifies parity with Vue's official compiler behavior.
 */
import { test, expect } from "@playwright/test";
import { getPreviewFrame, filterCriticalErrors, addFile } from "./helpers";

test.describe("Playground UI rendering", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await page.waitForTimeout(4000);
  });

  // @ai-generated - Regression: Monaco editor must mount inside the editor container.
  // Previously, filter_setup_return in verter_host stripped template ref bindings
  // (ref="editorContainer") from the setup return object, causing the ref to not bind
  // and Monaco to never create its editor instance.
  test("Monaco editor mounts in the left panel", async ({ page }) => {
    const monacoEditor = page.locator(".monaco-editor");
    await expect(monacoEditor.first()).toBeVisible({ timeout: 10000 });
  });

  // @ai-generated - SplitPane must render both panes with visible content
  test("SplitPane renders both editor and output panels", async ({ page }) => {
    const firstPane = page.locator(".pane.first");
    const secondPane = page.locator(".pane.second");

    await expect(firstPane).toBeVisible({ timeout: 5000 });
    await expect(secondPane).toBeVisible({ timeout: 5000 });

    // Both panes must have non-zero dimensions
    const firstBox = await firstPane.boundingBox();
    const secondBox = await secondPane.boundingBox();
    expect(firstBox).not.toBeNull();
    expect(secondBox).not.toBeNull();
    expect(firstBox!.width).toBeGreaterThan(100);
    expect(secondBox!.width).toBeGreaterThan(100);
    expect(firstBox!.height).toBeGreaterThan(0);
    expect(secondBox!.height).toBeGreaterThan(0);
  });

  // @ai-generated - File tabs must be visible
  test("file tabs are visible with default App.vue", async ({ page }) => {
    const appTab = page.locator(".file-selector .tab", { hasText: "App.vue" });
    await expect(appTab).toBeVisible({ timeout: 5000 });
  });

  // @ai-generated - Output tabs must be visible
  test("output tabs are visible", async ({ page }) => {
    const previewTab = page.locator(".output-tabs button", { hasText: "Preview" });
    const jsTab = page.locator(".output-tabs button", { hasText: "JS" });
    const cssTab = page.getByRole("button", { name: "CSS", exact: true });
    await expect(previewTab).toBeVisible({ timeout: 5000 });
    await expect(jsTab).toBeVisible({ timeout: 5000 });
    await expect(cssTab).toBeVisible({ timeout: 5000 });
  });
});

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

test.describe("Playground edge cases", () => {
  // @ai-generated - Regression: template-only SFC (no script block) must not throw
  // "__sfc__ is not defined". The playground's mergeRenderIntoComponent must create
  // const __sfc__ = {} when assembling a template-only component.
  test("template-only SFC renders without __sfc__ error", async ({ page }) => {
    const errors: string[] = [];
    let capturing = false;
    page.on("console", (msg) => {
      if (capturing && msg.type() === "error") {
        errors.push(msg.text());
      }
    });
    page.on("pageerror", (err) => {
      if (capturing) {
        errors.push(err.message);
      }
    });

    await page.goto("/");
    await page.waitForTimeout(4000);

    // Replace editor content by focusing Monaco's hidden textarea and using insertText.
    const templateOnlySfc = `<template><div>Template Only</div></template>`;

    // Click the Monaco editor to focus it, then select all and replace
    const monacoEditor = page.locator(".monaco-editor").first();
    await monacoEditor.click();
    await page.keyboard.press("Control+a");
    await page.keyboard.insertText(templateOnlySfc);

    // Start capturing errors after content is set
    capturing = true;
    await page.waitForTimeout(4000);

    // Check that no __sfc__ errors occurred
    const sfcErrors = errors.filter((e) => e.includes("__sfc__"));
    expect(sfcErrors).toEqual([]);

    // Check the preview renders the template content
    const frame = await getPreviewFrame(page);
    const div = frame.locator("div");
    await expect(div.first()).toBeVisible({ timeout: 5000 });
  });

  // @ai-generated - Regression: adding text after root element (creating multi-root)
  // must not cause "nextSibling" runtime error. The preview must properly unmount
  // the previous Vue app before mounting the new one.
  test("multi-root template does not cause nextSibling error", async ({
    page,
  }) => {
    // Only capture errors after the final content is set
    const errors: string[] = [];
    let capturing = false;

    page.on("console", (msg) => {
      if (capturing && msg.type() === "error") {
        errors.push(msg.text());
      }
    });
    page.on("pageerror", (err) => {
      if (capturing) {
        errors.push(err.message);
      }
    });

    await page.goto("/");
    await page.waitForTimeout(4000);

    // Replace editor content by focusing Monaco's input textarea and using
    // select-all + insertText. Monaco uses a hidden textarea for input.
    const multiRootSfc = `<script setup>
import { ref } from 'vue'
const count = ref(0)
</script>

<template>
  <div>
    <h1>Hello</h1>
    <button @click="count++">Count: {{ count }}</button>
  </div>
  <p>Extra root element</p>
</template>`;

    // Click the Monaco editor to focus it, then select all and replace
    const monacoEditor = page.locator(".monaco-editor").first();
    await monacoEditor.click();
    await page.keyboard.press("Control+a");
    await page.keyboard.insertText(multiRootSfc);

    // Start capturing errors after content is set
    capturing = true;
    await page.waitForTimeout(4000);

    // Check preview renders the multi-root content
    const frame = await getPreviewFrame(page);
    const h1 = frame.locator("h1");
    // Wait for the new content to render — check text changed from default
    await expect(h1).toHaveText("Hello", { timeout: 10000 });
    const p = frame.locator("p");
    await expect(p).toBeVisible({ timeout: 5000 });
    await expect(p).toHaveText("Extra root element");

    // No nextSibling errors
    const nextSiblingErrors = errors.filter((e) => e.includes("nextSibling"));
    expect(nextSiblingErrors).toEqual([]);

    // No general runtime errors (filter out compilation errors from typing)
    const runtimeErrors = filterCriticalErrors(errors).filter(
      (e) => !e.includes("HostError::CompileError"),
    );
    expect(runtimeErrors).toEqual([]);
  });
});
