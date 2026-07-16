/**
 * JS and TS language surfaces: rename, code actions, clean diagnostics.
 * Covers both carriers so "only TS works" regressions are visible.
 */
import * as vscode from "vscode";
import { FIXTURE_NAME } from "../../../helpers";
import {
  assertCleanErrors,
  assertHoverNeedles,
  assertReferenceCountAtLeast,
  assertRenameCoversAndRestores,
  codeActionsForFile,
  ensureParityReady,
  prepareRenameAt,
  registerFrameworkTest,
  failParityGap,
} from "../../../lib/parityHarness";

function framework(): "vue" | "svelte" | null {
  if (FIXTURE_NAME === "vue-parity") return "vue";
  if (FIXTURE_NAME === "svelte-parity") return "svelte";
  return null;
}

suite(`JS/TS actions [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    this.timeout(60_000);
    const fw = framework();
    if (!fw) {
      throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    }
    await ensureParityReady(fw === "vue" ? "src/App.vue" : "src/App.svelte");
  });

  test("shared.ts.rename.from-script", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/DailyBinding.vue" : "src/DailyBinding.svelte";
    try {
      await assertRenameCoversAndRestores(
        { file, token: "dailyValue", occurrence: 0 },
        "dailyDatum",
        { minEdits: 2 },
      );
    } catch (err) {
      failParityGap(
        this,
        "shared.ts.rename.from-script",
        "ISSUE-ts-rename-script",
        `TS rename failed (${fw}): ${String(err)}`,
      );
    }
  });

  test("shared.js.rename.from-script", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/features/JsRename.vue" : "src/features/JsRename.svelte";
    try {
      await assertCleanErrors(file);
      await assertRenameCoversAndRestores(
        { file, token: "jsRenameValue", occurrence: 0 },
        "jsRenameDatum",
        { minEdits: 2 },
      );
    } catch (err) {
      failParityGap(
        this,
        "shared.js.rename.from-script",
        "ISSUE-js-rename-script",
        `JS (@ts-check) rename failed (${fw}): ${String(err)}`,
      );
    }
  });

  test("shared.js.rename.from-markup", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/features/JsRename.vue" : "src/features/JsRename.svelte";
    try {
      // Markup occurrence of jsRenameValue (after script uses).
      await assertRenameCoversAndRestores(
        { file, token: "jsRenameValue", occurrence: 2 },
        "jsRenameDatum",
        { minEdits: 2 },
      );
    } catch (err) {
      failParityGap(
        this,
        "shared.js.rename.from-markup",
        "ISSUE-js-rename-markup",
        `JS rename from markup failed (${fw}): ${String(err)}`,
      );
    }
  });

  test("shared.js.hover.typed-markup", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/features/JsRename.vue" : "src/features/JsRename.svelte";
    try {
      await assertHoverNeedles({ file, token: "jsRenameValue", occurrence: 2 }, [
        "jsRenameValue",
        "label",
      ]);
    } catch (err) {
      failParityGap(
        this,
        "shared.js.hover.typed-markup",
        "ISSUE-js-hover-markup",
        `JS markup hover untyped (${fw}): ${String(err)}`,
      );
    }
  });

  test("shared.js.references.script-and-markup", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/features/JsRename.vue" : "src/features/JsRename.svelte";
    try {
      await assertReferenceCountAtLeast({ file, token: "jsRenameValue", occurrence: 0 }, 3);
    } catch (err) {
      failParityGap(
        this,
        "shared.js.references.script-and-markup",
        "ISSUE-js-references",
        `JS references incomplete (${fw}): ${String(err)}`,
      );
    }
  });

  registerFrameworkTest("vue", "shared.ts.code-action.source-kinds", async function () {
    try {
      const actions = await codeActionsForFile("src/features/OrganizeImports.vue");
      const kinds = actions
        .filter((a): a is vscode.CodeAction => "kind" in a && !!a.kind)
        .map((a) => a.kind!.value);
      if (kinds.length === 0) throw new Error("no code actions returned");
      // At least one source.* or quickfix-ish action should appear on a real SFC.
      const useful = kinds.some(
        (k) => k.startsWith("source.") || k.startsWith("quickfix") || k.startsWith("refactor"),
      );
      if (!useful) {
        throw new Error(`no source/quickfix/refactor kinds; got ${kinds.slice(0, 12).join(", ")}`);
      }
    } catch (err) {
      failParityGap(
        this,
        "shared.ts.code-action.source-kinds",
        "ISSUE-ts-code-actions",
        `TS code actions incomplete: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("shared.js.prepare-rename.rejects-html-tag", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/features/JsRename.vue" : "src/features/JsRename.svelte";
    try {
      const prepared = await prepareRenameAt({ file, token: "p", occurrence: 0 });
      // HTML/Svelte tags should not rename as identifiers — null/throw is success.
      // If prepare succeeds, it must not produce multi-file wholesale tag renames.
      if (prepared) {
        // Soft: some stacks allow prepare on text nodes; require range length === 1 for "p"
        const range = "range" in prepared ? prepared.range : prepared;
        const len =
          range.end.line === range.start.line ? range.end.character - range.start.character : 99;
        if (len > 1) {
          throw new Error(`prepareRename allowed wide range on tag-like token (len=${len})`);
        }
      }
    } catch (err) {
      failParityGap(
        this,
        "shared.js.prepare-rename.rejects-html-tag",
        "ISSUE-rename-reject-html",
        `prepareRename HTML rejection unclear (${fw}): ${String(err)}`,
      );
    }
  });
});
