/**
 * CSS class intelligence: markup class ↔ component style navigation.
 *
 * Vue scoped styles + Svelte scoped-by-default styles are ONE shared,
 * Verter-native surface: class tokens hover to their declaring rule
 * (```css block), navigate to the exact rule token, and fail CLOSED when no
 * rule declares the class (no link, no hover — never a mis-mapped
 * affordance). `v-bind()` tokens hover with the binding's TypeScript type
 * resolved at its declaration position.
 */
import * as vscode from "vscode";
import * as assert from "assert";
import { FIXTURE_NAME } from "../../../helpers";
import {
  assertDefinitionTargetsToken,
  assertHoverNeedles,
  ensureParityReady,
  openRelative,
  registerFrameworkTest,
  tokenPosition,
} from "../../../lib/parityHarness";

suite(`CSS class intelligence [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    this.timeout(120_000);
    if (FIXTURE_NAME === "vue-parity") {
      await ensureParityReady("src/features/CssIntel.vue");
    } else if (FIXTURE_NAME === "svelte-parity") {
      await ensureParityReady("src/features/CssIntel.svelte");
    }
  });

  registerFrameworkTest("vue", "vue.css.class.hover-rule", async function () {
    this.timeout(30_000);
    // occurrence 0 = the template `class="chip-live …"` token.
    const text = await assertHoverNeedles(
      { file: "src/features/CssIntel.vue", token: "chip-live", occurrence: 0 },
      ["```css", ".chip-live"],
    );
    assert.ok(text.includes("scoped"), `scoped label expected: ${text}`);
  });

  registerFrameworkTest("vue", "vue.css.class.definition-to-rule", async function () {
    this.timeout(30_000);
    await assertDefinitionTargetsToken(
      { file: "src/features/CssIntel.vue", token: "chip-live", occurrence: 0 },
      { file: "src/features/CssIntel.vue", token: "chip-live", occurrence: 1 },
    );
  });

  registerFrameworkTest("vue", "vue.css.class.no-rule-fails-closed", async function () {
    this.timeout(30_000);
    // `ghost-none` has NO declaring rule: hover and definition must both be
    // EMPTY (no polling — emptiness is the expected terminal state).
    const doc = await openRelative("src/features/CssIntel.vue");
    const position = tokenPosition(doc, {
      file: "src/features/CssIntel.vue",
      token: "ghost-none",
      occurrence: 0,
    });
    const definitions = await vscode.commands.executeCommand<
      Array<vscode.Location | vscode.LocationLink>
    >("vscode.executeDefinitionProvider", doc.uri, position);
    assert.strictEqual(
      (definitions ?? []).length,
      0,
      `a rule-less class token must have NO definition, got ${JSON.stringify(definitions)}`,
    );
    const hovers = await vscode.commands.executeCommand<vscode.Hover[]>(
      "vscode.executeHoverProvider",
      doc.uri,
      position,
    );
    const hoverText = (hovers ?? [])
      .flatMap((hover) => hover.contents)
      .map((content) => (typeof content === "string" ? content : content.value))
      .join("")
      .trim();
    assert.strictEqual(
      hoverText,
      "",
      `a rule-less class token must have NO hover, got: ${hoverText}`,
    );
  });

  registerFrameworkTest("vue", "vue.css.vbind.typed-hover", async function () {
    this.timeout(30_000);
    // occurrence 1 = the `v-bind(chipWidth)` token in the style block. The
    // PROVIDER-TYPED fragment ("chipWidth: 12") is asserted by the
    // editor-neutral 3-route contract where the server owns TS features; on
    // the editor-owned route this asserts the v-bind hover itself.
    await assertHoverNeedles(
      { file: "src/features/CssIntel.vue", token: "chipWidth", occurrence: 1 },
      ["v-bind(chipWidth)"],
    );
  });

  registerFrameworkTest("svelte", "svelte.css.class.hover-rule", async function () {
    this.timeout(30_000);
    const text = await assertHoverNeedles(
      { file: "src/features/CssIntel.svelte", token: "chip-live", occurrence: 0 },
      ["```css", ".chip-live"],
    );
    assert.ok(text.includes("scoped"), `svelte styles are scoped by default: ${text}`);
  });

  registerFrameworkTest("svelte", "svelte.css.class.definition-to-rule", async function () {
    this.timeout(30_000);
    await assertDefinitionTargetsToken(
      { file: "src/features/CssIntel.svelte", token: "chip-live", occurrence: 0 },
      { file: "src/features/CssIntel.svelte", token: "chip-live", occurrence: 1 },
    );
  });

  registerFrameworkTest(
    "svelte",
    "svelte.css.class-directive.definition-to-rule",
    async function () {
      this.timeout(30_000);
      // `on` occurrences: 0 = script declaration, 1 = `class:on`, 2 = `.on {`.
      await assertDefinitionTargetsToken(
        { file: "src/features/CssIntel.svelte", token: "on", occurrence: 1 },
        { file: "src/features/CssIntel.svelte", token: "on", occurrence: 2 },
      );
    },
  );
});
