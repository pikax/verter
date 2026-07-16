/**
 * Deeper typing/editing DX scenarios for both frameworks.
 */
import * as vscode from "vscode";
import { FIXTURE_NAME, sleep } from "../../../helpers";
import {
  assertHoverNeedles,
  assertHoverRangeCoversToken,
  completionsAtOffset,
  ensureParityReady,
  findOffset,
  openRelative,
  failParityGap,
} from "../../../lib/parityHarness";

function framework(): "vue" | "svelte" | null {
  if (FIXTURE_NAME === "vue-parity") return "vue";
  if (FIXTURE_NAME === "svelte-parity") return "svelte";
  return null;
}

suite(`Typing DX deep [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    this.timeout(60_000);
    const fw = framework();
    if (!fw) {
      throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    }
    await ensureParityReady(fw === "vue" ? "src/App.vue" : "src/App.svelte");
  });

  test("shared.typing.undo-preserves-hover", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    this.timeout(45_000);
    const file = fw === "vue" ? "src/DailyBinding.vue" : "src/DailyBinding.svelte";
    try {
      const doc = await openRelative(file);
      const editor = await vscode.window.showTextDocument(doc);
      const end = doc.positionAt(doc.getText().length);
      await editor.edit((eb) => eb.insert(end, "\n// dx\n"));
      await sleep(150);
      await vscode.commands.executeCommand("undo");
      await sleep(200);
      await assertHoverNeedles({ file, token: "dailyValue", occurrence: 3 }, ["dailyValue"]);
      await assertHoverRangeCoversToken({ file, token: "dailyValue", occurrence: 3 });
    } catch (err) {
      failParityGap(this, "shared.typing.undo-preserves-hover", "ISSUE-typing-undo", String(err));
    }
  });

  test("shared.typing.multi-cursor-not-required-but-single-edit-ok", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/features/MappingCase.vue" : "src/features/MappingCase.svelte";
    try {
      const doc = await openRelative(file);
      const editor = await vscode.window.showTextDocument(doc);
      const offset = findOffset(doc, "mapValue.count") + "mapValue.".length;
      editor.selection = new vscode.Selection(doc.positionAt(offset), doc.positionAt(offset));
      const labels = await completionsAtOffset(file, offset, ".");
      if (!labels.some((l) => l.startsWith("count") || l === "count")) {
        throw new Error(`count missing: ${labels.slice(0, 20).join(",")}`);
      }
    } catch (err) {
      failParityGap(
        this,
        "shared.typing.multi-cursor-not-required-but-single-edit-ok",
        "ISSUE-typing-member-after-select",
        String(err),
      );
    }
  });

  test("shared.typing.js-surface-completion", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/matrix/JsSurface.vue" : "src/matrix/JsSurface.svelte";
    try {
      const doc = await openRelative(file);
      const offset = findOffset(doc, "jsUser.name") + "jsUser.".length;
      const labels = await completionsAtOffset(file, offset);
      if (!labels.some((l) => l === "name" || l.startsWith("name"))) {
        throw new Error(`js member completion missing name: ${labels.slice(0, 20).join(",")}`);
      }
    } catch (err) {
      failParityGap(
        this,
        "shared.typing.js-surface-completion",
        "ISSUE-typing-js-completion",
        String(err),
      );
    }
  });

  test("shared.typing.rapid-toggle-save-dirty", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    this.timeout(45_000);
    const file = fw === "vue" ? "src/DailyBinding.vue" : "src/DailyBinding.svelte";
    try {
      const doc = await openRelative(file);
      const editor = await vscode.window.showTextDocument(doc);
      for (let i = 0; i < 5; i++) {
        await editor.edit((eb) => eb.insert(doc.positionAt(0), " "));
        await sleep(50);
        await vscode.commands.executeCommand("undo");
        await sleep(50);
      }
      await assertHoverNeedles({ file, token: "dailyValue", occurrence: 3 }, ["dailyValue"]);
    } catch (err) {
      failParityGap(
        this,
        "shared.typing.rapid-toggle-save-dirty",
        "ISSUE-typing-rapid-edit",
        String(err),
      );
    }
  });
});
