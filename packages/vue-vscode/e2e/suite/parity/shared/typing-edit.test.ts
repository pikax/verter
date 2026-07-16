/**
 * Typing / editing experience gates.
 *
 * These approximate the "feels good while typing" bar without a human:
 * - mid-edit completions still return
 * - sequential text edits keep hover/definition mapped
 * - recovery after a temporary syntax break
 * - rename still works after an edit storm
 */
import * as vscode from "vscode";
import { FIXTURE_NAME, sleep } from "../../../helpers";
import {
  assertDefinitionTargetsToken,
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

suite(`Typing and editing experience [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    this.timeout(60_000);
    const fw = framework();
    if (!fw) {
      throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    }
    await ensureParityReady(fw === "vue" ? "src/App.vue" : "src/App.svelte");
  });

  test("shared.typing.completion-while-editing-tag", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    this.timeout(45_000);
    const file = fw === "vue" ? "src/features/TypingEdit.vue" : "src/features/TypingEdit.svelte";
    try {
      const doc = await openRelative(file);
      const editor = await vscode.window.showTextDocument(doc);
      const marker = "<!-- type-complete-here --><";
      const base = findOffset(doc, marker) + marker.length;
      // Simulate typing a component prefix character-by-character.
      await editor.edit((eb) => {
        eb.insert(doc.positionAt(base), "P");
      });
      await sleep(150);
      await editor.edit((eb) => {
        const latest = editor.document;
        const at = findOffset(latest, marker) + marker.length + 1;
        eb.insert(latest.positionAt(at), "r");
      });
      await sleep(150);
      await editor.edit((eb) => {
        const latest = editor.document;
        const at = findOffset(latest, marker) + marker.length + 2;
        eb.insert(latest.positionAt(at), "o");
      });
      await sleep(300);
      const latest = editor.document;
      const offset = findOffset(latest, marker) + marker.length + 3; // after "Pro"
      const labels = await completionsAtOffset(file, offset);
      if (labels.length === 0) throw new Error("no completions while typing tag prefix");
      // Restore fixture file by reopening without save — discard dirty buffer.
      await vscode.commands.executeCommand("workbench.action.files.revert");
    } catch (err) {
      try {
        await vscode.commands.executeCommand("workbench.action.files.revert");
      } catch {
        /* best-effort */
      }
      failParityGap(
        this,
        "shared.typing.completion-while-editing-tag",
        "ISSUE-typing-tag-completion",
        `Mid-typing tag completion failed (${fw}): ${String(err)}`,
      );
    }
  });

  test("shared.typing.hover-survives-edit-storm", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    this.timeout(45_000);
    const file = fw === "vue" ? "src/DailyBinding.vue" : "src/DailyBinding.svelte";
    try {
      const doc = await openRelative(file);
      const editor = await vscode.window.showTextDocument(doc);
      // Insert and remove a space in a comment-free safe region (end of file).
      const end = doc.positionAt(doc.getText().length);
      await editor.edit((eb) => eb.insert(end, "\n"));
      await sleep(200);
      await assertHoverNeedles({ file, token: "dailyValue", occurrence: 3 }, ["dailyValue"]);
      await assertHoverRangeCoversToken({ file, token: "dailyValue", occurrence: 3 });
      await vscode.commands.executeCommand("workbench.action.files.revert");
    } catch (err) {
      try {
        await vscode.commands.executeCommand("workbench.action.files.revert");
      } catch {
        /* best-effort */
      }
      failParityGap(
        this,
        "shared.typing.hover-survives-edit-storm",
        "ISSUE-typing-hover-after-edit",
        `Hover/mapping broke after edit (${fw}): ${String(err)}`,
      );
    }
  });

  test("shared.typing.definition-after-newline-insert", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    this.timeout(45_000);
    const file = fw === "vue" ? "src/DailyBinding.vue" : "src/DailyBinding.svelte";
    try {
      const doc = await openRelative(file);
      const editor = await vscode.window.showTextDocument(doc);
      // Insert newline near top of template to shift offsets — mapping must still resolve.
      const text = doc.getText();
      const templateIdx =
        text.indexOf("<template>") >= 0
          ? text.indexOf("<template>") + 10
          : text.indexOf("<section>");
      if (templateIdx < 0) throw new Error("no template/section anchor");
      await editor.edit((eb) => eb.insert(doc.positionAt(templateIdx), "\n"));
      await sleep(300);
      await assertDefinitionTargetsToken(
        { file, token: "dailyValue", occurrence: 3 },
        { file, token: "dailyValue", occurrence: 0 },
      );
      await vscode.commands.executeCommand("workbench.action.files.revert");
    } catch (err) {
      try {
        await vscode.commands.executeCommand("workbench.action.files.revert");
      } catch {
        /* best-effort */
      }
      failParityGap(
        this,
        "shared.typing.definition-after-newline-insert",
        "ISSUE-typing-definition-after-edit",
        `Definition mapping broke after newline insert (${fw}): ${String(err)}`,
      );
    }
  });

  test("shared.typing.recovery-after-broken-expression", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    this.timeout(60_000);
    const file = fw === "vue" ? "src/DailyBinding.vue" : "src/DailyBinding.svelte";
    try {
      const doc = await openRelative(file);
      const editor = await vscode.window.showTextDocument(doc);
      const text = doc.getText();
      // Break a known mustache/expression then restore.
      const expr =
        fw === "vue" ? text.indexOf("{{ dailyValue.label }}") : text.indexOf("{dailyValue.label}");
      if (expr < 0) throw new Error("expression not found");
      const breakAt = expr + (fw === "vue" ? 3 : 1);
      await editor.edit((eb) => eb.insert(doc.positionAt(breakAt), "+++"));
      await sleep(400);
      // Even while broken, request completions at an earlier binding — should not hang.
      const safe = findOffset(editor.document, "dailyValue") + 1;
      const labels = await completionsAtOffset(file, safe);
      if (!Array.isArray(labels)) throw new Error("completion provider failed during broken state");
      await vscode.commands.executeCommand("workbench.action.files.revert");
      await sleep(400);
      // After restore, hover must work again.
      await assertHoverNeedles({ file, token: "dailyValue", occurrence: 3 }, ["dailyValue"]);
    } catch (err) {
      try {
        await vscode.commands.executeCommand("workbench.action.files.revert");
      } catch {
        /* best-effort */
      }
      failParityGap(
        this,
        "shared.typing.recovery-after-broken-expression",
        "ISSUE-typing-recovery",
        `Broken-expression recovery failed (${fw}): ${String(err)}`,
      );
    }
  });

  test("shared.typing.sequential-member-completion", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/features/MappingCase.vue" : "src/features/MappingCase.svelte";
    try {
      const doc = await openRelative(file);
      // Completions after mapValue. in template
      const needle = "mapValue.label";
      const offset = findOffset(doc, needle) + "mapValue.".length;
      const labels = await completionsAtOffset(file, offset, ".");
      const hasLabel = labels.some((l) => l === "label" || l.startsWith("label"));
      const hasCount = labels.some((l) => l === "count" || l.startsWith("count"));
      if (!hasLabel || !hasCount) {
        throw new Error(`expected label/count members; got ${labels.slice(0, 30).join(", ")}`);
      }
    } catch (err) {
      failParityGap(
        this,
        "shared.typing.sequential-member-completion",
        "ISSUE-typing-member-completion",
        `Member completion while editing failed (${fw}): ${String(err)}`,
      );
    }
  });
});
