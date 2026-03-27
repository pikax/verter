/**
 * @ai-generated - This test file was generated with AI assistance.
 * Comprehensive E2E tests for @verter/unplugin across all bundlers.
 * Tests rendering, reactivity, interactivity, TypeScript support, styles, and more.
 */

import { test, expect } from "@playwright/test";
import { collectConsoleErrors, waitForApp, filterKnownErrors } from "./helpers";

test.describe("Verter E2E Tests", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await waitForApp(page);
  });

  // ─── Core ───────────────────────────────────────────────

  test("app renders without critical console errors", async ({ page }) => {
    const errors = collectConsoleErrors(page);
    // Navigate fresh to capture errors from load
    await page.goto("/");
    await waitForApp(page);
    const critical = filterKnownErrors(errors);
    expect(critical).toEqual([]);
  });

  test("app title and all sections render", async ({ page }) => {
    await expect(page.getByTestId("app-title")).toHaveText("Verter E2E Test App");
    const sections = [
      "section-reactivity",
      "section-typescript",
      "section-options-api",
      "section-directives",
      "section-slots",
      "section-styles",
      "section-composables",
      "section-edge-cases",
      "section-integration",
    ];
    for (const s of sections) {
      await expect(page.getByTestId(s)).toBeVisible();
    }
  });

  // ─── Reactivity ─────────────────────────────────────────

  test("RefCounter: increment updates count and computed", async ({ page }) => {
    await expect(page.getByTestId("count")).toHaveText("0");
    await expect(page.getByTestId("doubled")).toHaveText("0");

    await page.getByTestId("increment").click();
    await expect(page.getByTestId("count")).toHaveText("1");
    await expect(page.getByTestId("doubled")).toHaveText("2");

    await page.getByTestId("increment").click();
    await expect(page.getByTestId("count")).toHaveText("2");
    await expect(page.getByTestId("doubled")).toHaveText("4");
  });

  test("RefCounter: watch log updates after change", async ({ page }) => {
    await page.getByTestId("increment").click();
    const log = page.getByTestId("watch-log");
    await expect(log.locator("li")).toHaveCount(1);
    await expect(log.locator("li").first()).toContainText("0 → 1");
  });

  test("ReactiveObject: change name, add item, reset", async ({ page }) => {
    await expect(page.getByTestId("name")).toHaveText("Hello");

    await page.getByTestId("change-name").click();
    await expect(page.getByTestId("name")).toHaveText("World");

    const items = page.getByTestId("items").locator("li");
    await expect(items).toHaveCount(3);

    await page.getByTestId("add-item").click();
    await expect(items).toHaveCount(4);

    await page.getByTestId("reset").click();
    await expect(page.getByTestId("name")).toHaveText("Hello");
    await expect(items).toHaveCount(3);
  });

  // ─── TypeScript ─────────────────────────────────────────

  test("TypedPropsEmits: prop passing and emit", async ({ page }) => {
    await expect(page.getByTestId("child-message")).toHaveText("from-parent");
    await expect(page.getByTestId("received")).toHaveText("");

    await page.getByTestId("child-reply").click();
    await expect(page.getByTestId("received")).toHaveText("child-reply");

    await page.getByTestId("update-message").click();
    await expect(page.getByTestId("child-message")).toHaveText("updated-parent");
  });

  test("WithDefaults: renders default prop values", async ({ page }) => {
    await expect(page.getByTestId("defaults-label")).toHaveText("default-label");
    await expect(page.getByTestId("defaults-count")).toHaveText("42");
    await expect(page.getByTestId("override-state")).toHaveText("default");

    await page.getByTestId("toggle-override").click();
    await expect(page.getByTestId("override-state")).toHaveText("overridden");
  });

  test("DefineModel: two-way binding and reset", async ({ page }) => {
    await expect(page.getByTestId("parent-value")).toHaveText("initial");
    await expect(page.getByTestId("model-display")).toHaveText("initial");

    await page.getByTestId("model-input").fill("typed-value");
    await expect(page.getByTestId("parent-value")).toHaveText("typed-value");

    await page.getByTestId("model-reset").click();
    await expect(page.getByTestId("parent-value")).toHaveText("");
    await expect(page.getByTestId("model-display")).toHaveText("");
  });

  test("DefineExpose: parent calls exposed method", async ({ page }) => {
    await expect(page.getByTestId("expose-count")).toHaveText("0");
    await expect(page.getByTestId("expose-result")).toHaveText("");

    await page.getByTestId("call-exposed").click();
    await expect(page.getByTestId("expose-count")).toHaveText("1");
    await expect(page.getByTestId("expose-result")).toHaveText("count: 1");
  });

  test("GenericComponent: cycles through typed items", async ({ page }) => {
    await expect(page.getByTestId("generic-current")).toHaveText("Alpha");
    await expect(page.getByTestId("generic-index")).toHaveText("0");

    await page.getByTestId("generic-next").click();
    await expect(page.getByTestId("generic-current")).toHaveText("Beta");
    await expect(page.getByTestId("generic-index")).toHaveText("1");

    await page.getByTestId("generic-next").click();
    await expect(page.getByTestId("generic-current")).toHaveText("Gamma");

    await page.getByTestId("generic-next").click();
    await expect(page.getByTestId("generic-current")).toHaveText("Alpha");
  });

  // ─── Options API ────────────────────────────────────────

  test("OptionsCounter: data, methods, computed, watch", async ({ page }) => {
    await expect(page.getByTestId("options-count")).toHaveText("0");
    await expect(page.getByTestId("options-doubled")).toHaveText("0");

    await page.getByTestId("options-increment").click();
    await expect(page.getByTestId("options-count")).toHaveText("1");
    await expect(page.getByTestId("options-doubled")).toHaveText("2");

    const history = page.getByTestId("options-watch-history");
    await expect(history.locator("li")).toHaveCount(1);
  });

  test("OptionsComponent: props and emits", async ({ page }) => {
    await expect(page.getByTestId("options-label")).toHaveText("test-label");
    await expect(page.getByTestId("options-received")).toHaveText("");

    await page.getByTestId("options-action").click();
    await expect(page.getByTestId("options-received")).toHaveText("options-action");
  });

  // ─── Directives ─────────────────────────────────────────

  test("ConditionalRendering: v-if cycling and v-show", async ({ page }) => {
    await expect(page.getByTestId("cond-a")).toBeVisible();
    await expect(page.getByTestId("cond-b")).not.toBeVisible();

    await page.getByTestId("cycle-condition").click();
    await expect(page.getByTestId("cond-b")).toBeVisible();
    await expect(page.getByTestId("cond-a")).not.toBeVisible();

    await page.getByTestId("cycle-condition").click();
    await expect(page.getByTestId("cond-c")).toBeVisible();

    // v-show
    await expect(page.getByTestId("v-show-target")).toBeVisible();
    await page.getByTestId("toggle-visible").click();
    await expect(page.getByTestId("v-show-target")).toBeHidden();
    await page.getByTestId("toggle-visible").click();
    await expect(page.getByTestId("v-show-target")).toBeVisible();
  });

  test("ListRendering: add, remove, sort, reverse", async ({ page }) => {
    const items = page.getByTestId("list-item");
    await expect(items).toHaveCount(3);

    await page.getByTestId("list-add").click();
    await expect(items).toHaveCount(4);

    await page.getByTestId("list-remove").click();
    await expect(items).toHaveCount(3);

    // Sort and verify order
    await page.getByTestId("list-sort").click();
    const sorted = await items.allTextContents();
    const isSorted = sorted.every((v, i, a) => i === 0 || a[i - 1] <= v);
    expect(isSorted).toBe(true);

    // Reverse
    await page.getByTestId("list-reverse").click();
    const reversed = await items.allTextContents();
    expect(reversed).toEqual([...sorted].reverse());
  });

  test("FormInputs: all input types with v-model", async ({ page }) => {
    // Text
    await page.getByTestId("text-input").fill("hello");
    await expect(page.getByTestId("text-mirror")).toHaveText("hello");

    // Checkbox
    await page.getByTestId("checkbox-input").check();
    await expect(page.getByTestId("checkbox-mirror")).toHaveText("true");

    // Radio
    await page.getByTestId("radio-opt2").check();
    await expect(page.getByTestId("radio-mirror")).toHaveText("opt2");

    // Select
    await page.getByTestId("select-input").selectOption("b");
    await expect(page.getByTestId("select-mirror")).toHaveText("b");

    // Textarea
    await page.getByTestId("textarea-input").fill("multiline");
    await expect(page.getByTestId("textarea-mirror")).toHaveText("multiline");
  });

  test("EventModifiers: regular, once, stop, enter", async ({ page }) => {
    // Regular - increments each time
    await page.getByTestId("regular-btn").click();
    await page.getByTestId("regular-btn").click();
    await expect(page.getByTestId("regular-count")).toHaveText("2");

    // Once - only first click increments
    await page.getByTestId("once-btn").click();
    await page.getByTestId("once-btn").click();
    await expect(page.getByTestId("once-count")).toHaveText("1");

    // Stop propagation - parent handler NOT called
    await page.getByTestId("stop-btn").click();
    await expect(page.getByTestId("parent-count")).toHaveText("0");

    // keyup.enter
    await page.getByTestId("enter-input").press("Enter");
    await expect(page.getByTestId("enter-count")).toHaveText("1");
  });

  test("DynamicBindings: class and style toggling", async ({ page }) => {
    const target = page.getByTestId("class-target");
    await expect(target).not.toHaveClass(/active/);

    await page.getByTestId("toggle-active").click();
    await expect(target).toHaveClass(/active/);

    await page.getByTestId("toggle-error").click();
    await expect(target).toHaveClass(/error/);

    // Style binding
    const styleTarget = page.getByTestId("style-target");
    await expect(styleTarget).toHaveCSS("color", "rgb(0, 0, 255)");

    await page.getByTestId("toggle-color").click();
    await expect(styleTarget).toHaveCSS("color", "rgb(255, 0, 0)");
  });

  // ─── Slots ──────────────────────────────────────────────

  test("SlotShowcase: all slot types work", async ({ page }) => {
    await expect(page.getByTestId("custom-header")).toHaveText("Custom Header Content");
    await expect(page.getByTestId("custom-default")).toHaveText("Custom Default Content");
    await expect(page.getByTestId("custom-footer")).toHaveText("Custom Footer Content");

    // Scoped slot
    await expect(page.getByTestId("scoped-count")).toHaveText("0");
    await page.getByTestId("scoped-increment").click();
    await expect(page.getByTestId("scoped-count")).toHaveText("1");
  });

  // ─── Styles ─────────────────────────────────────────────

  test("ScopedCss: scoped attribute and styles apply", async ({ page }) => {
    const text = page.getByTestId("scoped-text");
    // Scoped styles should apply red color
    await expect(text).toHaveCSS("color", "rgb(255, 0, 0)");

    // Check data-v-* attribute exists (scoped CSS)
    const attrs = await text.evaluate((el) =>
      Array.from(el.attributes).some((a) => a.name.startsWith("data-v-")),
    );
    expect(attrs).toBe(true);
  });

  test("CssModules: hashed class names and toggle", async ({ page }) => {
    const text = page.getByTestId("module-text");
    // Class should be hashed, not the raw name
    const className = await text.getAttribute("class");
    expect(className).toBeTruthy();
    expect(className).not.toBe("inactive");
    expect(className).not.toBe("active");

    // Should have inactive color initially
    await expect(text).toHaveCSS("color", "rgb(128, 128, 128)");

    await page.getByTestId("module-toggle").click();
    await expect(text).toHaveCSS("color", "rgb(0, 128, 0)");
    await expect(text).toHaveCSS("font-weight", "700");
  });

  test("ScssDemo: SCSS compiled styles apply", async ({ page }) => {
    const text = page.getByTestId("scss-text");
    await expect(text).toHaveCSS("color", "rgb(0, 100, 200)");

    await page.getByTestId("scss-toggle").click();
    // After toggling dark theme, styles should change
    const container = page.getByTestId("scss-demo");
    await expect(container).toHaveClass(/dark/);
  });

  test("LessDemo: Less compiled styles apply", async ({ page }) => {
    const text = page.getByTestId("less-text");
    await expect(text).toHaveCSS("color", "rgb(0, 128, 0)");

    await page.getByTestId("less-toggle").click();
    await expect(text).toHaveCSS("color", "rgb(200, 100, 0)");
  });

  test("CssVBind: reactive CSS values update", async ({ page }) => {
    const text = page.getByTestId("vbind-text");
    await expect(text).toHaveCSS("color", "rgb(255, 0, 0)");

    await page.getByTestId("vbind-change").click();
    await expect(text).toHaveCSS("color", "rgb(0, 0, 255)");
  });

  // ─── Composables ────────────────────────────────────────

  test("ComposableDemo: useCounter composable works", async ({ page }) => {
    await expect(page.getByTestId("composable-count")).toHaveText("0");
    await expect(page.getByTestId("composable-doubled")).toHaveText("0");

    await page.getByTestId("composable-increment").click();
    await expect(page.getByTestId("composable-count")).toHaveText("1");
    await expect(page.getByTestId("composable-doubled")).toHaveText("2");

    await page.getByTestId("composable-decrement").click();
    await expect(page.getByTestId("composable-count")).toHaveText("0");

    await page.getByTestId("composable-increment").click();
    await page.getByTestId("composable-increment").click();
    await page.getByTestId("composable-reset").click();
    await expect(page.getByTestId("composable-count")).toHaveText("0");
  });

  test("ProvideInject: parent provides, child injects", async ({ page }) => {
    await expect(page.getByTestId("provider-value")).toHaveText("light");
    await expect(page.getByTestId("injected-value")).toHaveText("light");

    await page.getByTestId("toggle-theme").click();
    await expect(page.getByTestId("provider-value")).toHaveText("dark");
    await expect(page.getByTestId("injected-value")).toHaveText("dark");
  });

  // ─── Edge Cases ─────────────────────────────────────────

  test("FragmentRoots: multiple root elements work independently", async ({ page }) => {
    await expect(page.getByTestId("fragment-count-a")).toHaveText("0");
    await expect(page.getByTestId("fragment-count-b")).toHaveText("0");

    await page.getByTestId("fragment-btn-a").click();
    await expect(page.getByTestId("fragment-count-a")).toHaveText("1");
    await expect(page.getByTestId("fragment-count-b")).toHaveText("0");

    await page.getByTestId("fragment-btn-b").click();
    await expect(page.getByTestId("fragment-count-b")).toHaveText("1");
  });

  test("MultiInstance: 3 instances have independent state", async ({ page }) => {
    const instances = page.getByTestId("multi-instance-wrapper").getByTestId("instance-count");
    await expect(instances).toHaveCount(3);

    // All start at 0
    for (let i = 0; i < 3; i++) {
      await expect(instances.nth(i)).toHaveText("0");
    }

    // Click first instance only
    const buttons = page.getByTestId("multi-instance-wrapper").getByTestId("instance-btn");
    await buttons.nth(0).click();
    await expect(instances.nth(0)).toHaveText("1");
    await expect(instances.nth(1)).toHaveText("0");
    await expect(instances.nth(2)).toHaveText("0");
  });

  test("ArrayMutations: push, pop, splice, sort, reverse", async ({ page }) => {
    const items = page.getByTestId("array-item");
    await expect(items).toHaveCount(5); // [3, 1, 4, 1, 5]

    await page.getByTestId("array-push").click();
    await expect(items).toHaveCount(6);

    await page.getByTestId("array-pop").click();
    await expect(items).toHaveCount(5);

    await page.getByTestId("array-splice").click();
    await expect(items).toHaveCount(4);

    await page.getByTestId("array-sort").click();
    const sorted = await items.allTextContents();
    const nums = sorted.map(Number);
    expect(nums).toEqual([...nums].sort((a, b) => a - b));

    await page.getByTestId("array-reverse").click();
    const reversed = await items.allTextContents();
    expect(reversed).toEqual([...sorted].reverse());
  });

  test("AsyncUpdate: state updates after delay", async ({ page }) => {
    await expect(page.getByTestId("async-status")).toHaveText("idle");

    await page.getByTestId("async-trigger").click();
    await expect(page.getByTestId("async-status")).toHaveText("loading");

    // Wait for async update (100ms timeout in component)
    await expect(page.getByTestId("async-status")).toHaveText("done", { timeout: 5000 });
  });

  test("TemplateRefs: focus and read value", async ({ page }) => {
    await page.getByTestId("ref-read").click();
    await expect(page.getByTestId("ref-value")).toHaveText("hello-ref");

    await page.getByTestId("ref-focus").click();
    await expect(page.getByTestId("ref-input")).toBeFocused();
  });

  test("DynamicComponent: switches between components", async ({ page }) => {
    await expect(page.getByTestId("dynamic-comp-a")).toBeVisible();

    await page.getByTestId("dynamic-switch").click();
    await expect(page.getByTestId("dynamic-comp-b")).toBeVisible();
    await expect(page.getByTestId("dynamic-comp-a")).not.toBeVisible();

    await page.getByTestId("dynamic-switch").click();
    await expect(page.getByTestId("dynamic-comp-c")).toBeVisible();

    await page.getByTestId("dynamic-switch").click();
    await expect(page.getByTestId("dynamic-comp-a")).toBeVisible();
  });

  test("DeepNested: leaf updates root via provide/inject", async ({ page }) => {
    await expect(page.getByTestId("deep-root-value")).toHaveText("root-initial");
    await expect(page.getByTestId("deep-leaf-value")).toHaveText("root-initial");

    await page.getByTestId("deep-leaf-btn").click();
    await expect(page.getByTestId("deep-root-value")).toHaveText("leaf-updated");
    await expect(page.getByTestId("deep-leaf-value")).toHaveText("leaf-updated");
  });

  // @ai-generated - Regression: scoped component with "export default" in a comment
  // must compile without duplicate export default errors
  test("ExportDefaultComment: renders with scoped style and comment containing export default", async ({
    page,
  }) => {
    await expect(page.getByTestId("export-comment-label")).toHaveText("export default in comment");
    await expect(page.getByTestId("export-comment-count")).toHaveText("0");

    await page.getByTestId("export-comment-btn").click();
    await expect(page.getByTestId("export-comment-count")).toHaveText("1");
  });

  // @ai-generated - Regression: scoped component with "function render" in a comment
  // must compile and render correctly in both dev and production (inline render) builds.
  // In production, the render is inlined and "function render" in a comment must not
  // cause a false positive _sfc_main.render = render attachment.
  test("RenderInComment: renders with scoped style and comment containing function render", async ({
    page,
  }) => {
    await expect(page.getByTestId("render-comment-label")).toHaveText("function render in comment");
    await expect(page.getByTestId("render-comment-count")).toHaveText("0");

    await page.getByTestId("render-comment-btn").click();
    await expect(page.getByTestId("render-comment-count")).toHaveText("1");
  });

  // @ai-generated - Text nodes mixed with element children must render correctly.
  // Without _createTextVNode wrapping, raw strings in children arrays don't mount
  // inside block elements (v-for, root template).
  test("MixedTextChildren: text nodes render alongside elements", async ({ page }) => {
    // Static text between elements
    const staticDiv = page.getByTestId("mixed-text-static");
    await expect(staticDiv).toContainText("middle");

    // Interpolation between elements
    const interpDiv = page.getByTestId("mixed-text-interp");
    await expect(interpDiv).toContainText("static");

    // v-for with mixed text+element children
    await expect(page.getByTestId("mixed-item-1")).toContainText("Alpha");
    await expect(page.getByTestId("mixed-badge-1")).toHaveText("A");
    await expect(page.getByTestId("mixed-item-2")).toContainText("Beta");
    await expect(page.getByTestId("mixed-badge-2")).toHaveText("B");
  });

  // ─── Integration ────────────────────────────────────────

  test("TodoApp: full CRUD workflow", async ({ page }) => {
    const todoInput = page.getByTestId("todo-input");
    const addBtn = page.getByTestId("todo-add");
    const items = page.getByTestId("todo-item");
    const remaining = page.getByTestId("todo-remaining");

    // Add todos
    await todoInput.fill("Buy groceries");
    await addBtn.click();
    await todoInput.fill("Walk the dog");
    await addBtn.click();
    await todoInput.fill("Write tests");
    await addBtn.click();

    await expect(items).toHaveCount(3);
    await expect(remaining).toHaveText("3 remaining");

    // Complete first todo
    await items.nth(0).getByTestId("todo-checkbox").check();
    await expect(remaining).toHaveText("2 remaining");
    await expect(items.nth(0)).toHaveAttribute("data-completed", "true");

    // Delete second todo
    await items.nth(1).getByTestId("todo-delete").click();
    await expect(items).toHaveCount(2);

    // Filter: active only
    await page.getByTestId("filter-active").click();
    await expect(items).toHaveCount(1);
    await expect(items.nth(0).getByTestId("todo-text")).toHaveText("Write tests");

    // Filter: completed only
    await page.getByTestId("filter-completed").click();
    await expect(items).toHaveCount(1);
    await expect(items.nth(0).getByTestId("todo-text")).toHaveText("Buy groceries");

    // Filter: all
    await page.getByTestId("filter-all").click();
    await expect(items).toHaveCount(2);
  });

  // ─── Vapor ─────────────────────────────────────────────

  // @ai-generated - Tests vapor counter component with click handler and reactive text
  test("VaporCounter: increment updates count", async ({ page }) => {
    await expect(page.getByTestId("vapor-count")).toHaveText("0");
    await page.getByTestId("vapor-increment").click();
    await expect(page.getByTestId("vapor-count")).toHaveText("1");
    await page.getByTestId("vapor-increment").click();
    await page.getByTestId("vapor-increment").click();
    await expect(page.getByTestId("vapor-count")).toHaveText("3");
  });

  // @ai-generated - Tests vapor component with dynamic :style binding and computed styles
  test("VaporBindings: style binding and reactive text", async ({ page }) => {
    // Check initial text values
    await expect(page.getByTestId("vapor-color-val")).toHaveText("blue");
    await expect(page.getByTestId("vapor-size-val")).toHaveText("16");

    // Toggle color
    await page.getByTestId("vapor-color").click();
    await expect(page.getByTestId("vapor-color-val")).toHaveText("red");

    // Toggle back
    await page.getByTestId("vapor-color").click();
    await expect(page.getByTestId("vapor-color-val")).toHaveText("blue");

    // Increase font size
    await page.getByTestId("vapor-bigger").click();
    await expect(page.getByTestId("vapor-size-val")).toHaveText("18");

    // Check style is applied
    const styled = page.getByTestId("vapor-styled");
    await expect(styled).toHaveCSS("color", "rgb(0, 0, 255)");
    await expect(styled).toHaveCSS("font-size", "18px");
  });
});
