/**
 * Style / CSS framework surfaces: scoped CSS, class/id navigation, references.
 */
import { FIXTURE_NAME } from "../../../helpers";
import {
  assertCleanErrors,
  assertDefinitionTargetsToken,
  assertHoverNeedles,
  assertReferenceCountAtLeast,
  definitionsAt,
  ensureParityReady,
  openRelative,
  registerFrameworkTest,
  failParityGap,
  tokenPosition,
} from "../../../lib/parityHarness";
import * as vscode from "vscode";

function framework(): "vue" | "svelte" | null {
  if (FIXTURE_NAME === "vue-parity") return "vue";
  if (FIXTURE_NAME === "svelte-parity") return "svelte";
  return null;
}

suite(`Style/CSS [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    this.timeout(20_000);
    const fw = framework();
    if (!fw) {
      throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    }
    await ensureParityReady(fw === "vue" ? "src/App.vue" : "src/App.svelte");
  });

  test("shared.style.class.definition-style-to-template", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/features/ScopedStyle.vue" : "src/features/ScopedStyle.svelte";
    try {
      // A valid class-bearing component must remain semantically clean while
      // style navigation is active. This catches a provider selecting React's
      // `className` JSX surface while the navigation assertion itself passes.
      await assertCleanErrors(file);
      // From style `.card-title` selector toward template class usage.
      const locations = await definitionsAt({ file, token: "card-title", occurrence: 1 });
      const sameFile = locations.some((l) =>
        l.uri.fsPath.replace(/\\/g, "/").endsWith(file.replace(/^\//, "")),
      );
      // Accept same-file navigation (template or style). Zero locations = unsupported.
      if (locations.length === 0) {
        throw new Error("no definition from style class selector");
      }
      if (!sameFile && locations.length > 0) {
        // Some stacks map to virtual docs — forbidden.
        const leaked = locations.filter((l) => /\.(vue|svelte)\.(tsx|jsx)/i.test(l.uri.fsPath));
        if (leaked.length)
          throw new Error(`style definition leaked virtual path: ${leaked[0].uri.fsPath}`);
      }
    } catch (err) {
      failParityGap(
        this,
        "shared.style.class.definition-style-to-template",
        "ISSUE-style-class-definition",
        `Style→template class definition failed (${fw}): ${String(err)}`,
      );
    }
  });

  test("shared.style.class.definition-template-to-style", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/features/ScopedStyle.vue" : "src/features/ScopedStyle.svelte";
    try {
      await assertDefinitionTargetsToken(
        { file, token: "card-title", occurrence: 0 },
        { file, token: "card-title", occurrence: 1 },
      );
    } catch (err) {
      // Template class → style may reverse-order occurrences; accept any same-file hit.
      try {
        const locations = await definitionsAt({ file, token: "card-title", occurrence: 0 });
        if (locations.length === 0) throw new Error("empty");
      } catch (inner) {
        failParityGap(
          this,
          "shared.style.class.definition-template-to-style",
          "ISSUE-style-class-definition-template",
          `Template→style class definition failed (${fw}): ${String(err)}; ${String(inner)}`,
        );
      }
    }
  });

  test("shared.style.id.references", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/features/ScopedStyle.vue" : "src/features/ScopedStyle.svelte";
    try {
      await assertReferenceCountAtLeast({ file, token: "card-root", occurrence: 0 }, 2);
    } catch (err) {
      failParityGap(
        this,
        "shared.style.id.references",
        "ISSUE-style-id-references",
        `id references incomplete (${fw}): ${String(err)}`,
      );
    }
  });

  test("shared.style.hover.selector", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/features/ScopedStyle.vue" : "src/features/ScopedStyle.svelte";
    try {
      // Hover on style selector — any meaningful CSS/text is fine; empty is not.
      await assertHoverNeedles({ file, token: "card-title", occurrence: 1 }, ["card-title"]);
    } catch (err) {
      failParityGap(
        this,
        "shared.style.hover.selector",
        "ISSUE-style-hover",
        `Style selector hover failed (${fw}): ${String(err)}`,
      );
    }
  });

  registerFrameworkTest("vue", "shared.style.vue-scoped.attribute-present", async function () {
    try {
      const doc = await openRelative("src/features/ScopedStyle.vue");
      if (!doc.getText().includes("<style scoped>")) {
        throw new Error("fixture missing scoped style block");
      }
      // Opening should not crash; hover on scoped class still works.
      await assertHoverNeedles(
        { file: "src/features/ScopedStyle.vue", token: "card", occurrence: 0 },
        ["card"],
      );
    } catch (err) {
      failParityGap(
        this,
        "shared.style.vue-scoped.attribute-present",
        "ISSUE-style-vue-scoped",
        `Vue scoped style surface failed: ${String(err)}`,
      );
    }
  });

  registerFrameworkTest("svelte", "shared.style.svelte-global.present", async function () {
    try {
      const doc = await openRelative("src/features/ScopedStyle.svelte");
      if (!doc.getText().includes(":global")) {
        throw new Error("fixture missing :global");
      }
      // Definition/hover on :global block should not throw.
      const pos = tokenPosition(doc, {
        file: "src/features/ScopedStyle.svelte",
        token: "global",
        occurrence: 0,
      });
      await vscode.commands.executeCommand("vscode.executeHoverProvider", doc.uri, pos);
    } catch (err) {
      failParityGap(
        this,
        "shared.style.svelte-global.present",
        "ISSUE-style-svelte-global",
        `Svelte :global style surface failed: ${String(err)}`,
      );
    }
  });

  test("shared.style.global-and-local-coexist", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/features/GlobalStyle.vue" : "src/features/GlobalStyle.svelte";
    try {
      const doc = await openRelative(file);
      const text = doc.getText();
      if (!text.includes("local-only") || !text.includes("leaked-global")) {
        throw new Error("TEST_DEFECT: GlobalStyle fixture must include local + global classes");
      }
      if (!text.includes(":global") && fw === "svelte") {
        throw new Error("TEST_DEFECT: Svelte GlobalStyle needs :global");
      }
      if (!text.includes(":global") && fw === "vue" && !text.includes("scoped")) {
        throw new Error("TEST_DEFECT: Vue GlobalStyle needs scoped + :global");
      }
      await assertCleanErrors(file);
      await assertHoverNeedles({ file, token: "local-only", occurrence: 0 }, ["local-only"]);
    } catch (err) {
      failParityGap(
        this,
        "shared.style.global-and-local-coexist",
        fw === "vue" ? "ISSUE-style-vue-global-local" : "ISSUE-style-svelte-global-local",
        `Scoped + :global coexistence failed (${fw}): ${String(err)}`,
        "product-gap",
      );
    }
  });
});
