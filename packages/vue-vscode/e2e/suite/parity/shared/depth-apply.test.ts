/**
 * Coverage depth: apply rename, reject HTML rename, mapping on events/slots,
 * undo-safe edit (shared Vue + Svelte).
 */
import * as vscode from "vscode";
import { FIXTURE_NAME, sleep } from "../../../helpers";
import {
  assertHoverRangeCoversToken,
  assertRenameCoversAndRestores,
  ensureParityReady,
  openRelative,
  prepareRenameAt,
  failParityGap,
} from "../../../lib/parityHarness";

function framework(): "vue" | "svelte" | null {
  if (FIXTURE_NAME === "vue-parity") return "vue";
  if (FIXTURE_NAME === "svelte-parity") return "svelte";
  return null;
}

suite(`Depth apply + mapping [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    const fw = framework();
    if (!fw) {
      throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    }
    this.timeout(20_000);
    await ensureParityReady(fw === "vue" ? "src/App.vue" : "src/App.svelte");
  });

  test("depth.rename.script-and-markup.min-two-edits", async function () {
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
        "depth.rename.script-and-markup.min-two-edits",
        "ISSUE-depth-rename-apply",
        `Rename must edit script+markup for ${fw}: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("depth.rename.reject-html-tag", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/DailyBinding.vue" : "src/DailyBinding.svelte";
    try {
      // prepareRename on a plain HTML tag name should fail closed (not rename "button"/"section")
      const prepared = await prepareRenameAt({
        file,
        token: fw === "vue" ? "button" : "section",
        occurrence: 0,
      });
      if (prepared) {
        throw new Error("prepareRename succeeded on HTML tag — expected reject");
      }
    } catch (err) {
      if (String(err).includes("expected reject")) {
        failParityGap(
          this,
          "depth.rename.reject-html-tag",
          "ISSUE-rename-reject-html",
          String(err),
          "product-gap",
        );
      }
      // provider throw = reject (ok)
    }
  });

  test("depth.mapping.event-handler-hover-range", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/ide/IdeSurfaceParent.vue" : "src/ide/IdeSurfaceParent.svelte";
    try {
      await assertHoverRangeCoversToken({ file, token: "onPick", occurrence: 1 });
    } catch (err) {
      failParityGap(
        this,
        "depth.mapping.event-handler-hover-range",
        "ISSUE-depth-mapping-event",
        `Event handler hover range mapping failed (${fw}): ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("depth.mapping.slot-prop-hover-range", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/slots/SlotCorrect.vue" : "src/slots/SnippetCorrect.svelte";
    try {
      await assertHoverRangeCoversToken({ file, token: "title", occurrence: 1 });
    } catch (err) {
      failParityGap(
        this,
        "depth.mapping.slot-prop-hover-range",
        "ISSUE-depth-mapping-slot",
        `Slot prop hover range mapping failed (${fw}): ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("depth.edit-undo.preserves-hover-token", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/DailyBinding.vue" : "src/DailyBinding.svelte";
    try {
      const doc = await openRelative(file);
      const editor = await vscode.window.showTextDocument(doc);
      const end = doc.positionAt(doc.getText().length);
      await editor.edit((eb) => eb.insert(end, "\n// depth-dx\n"));
      await sleep(100);
      await vscode.commands.executeCommand("undo");
      await sleep(150);
      await assertHoverRangeCoversToken({ file, token: "dailyValue", occurrence: 0 });
    } catch (err) {
      failParityGap(
        this,
        "depth.edit-undo.preserves-hover-token",
        "ISSUE-depth-undo-hover",
        `Undo after edit lost typed hover (${fw}): ${String(err)}`,
        "product-gap",
      );
    }
  });
});
