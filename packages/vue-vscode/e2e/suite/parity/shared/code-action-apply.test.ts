/**
 * Code action apply paths (organize imports) for TS carriers.
 */
import * as vscode from "vscode";
import { FIXTURE_NAME, sleep } from "../../../helpers";
import {
  codeActionsForFile,
  ensureParityReady,
  openRelative,
  registerFrameworkTest,
  failParityGap,
} from "../../../lib/parityHarness";

function framework(): "vue" | "svelte" | null {
  if (FIXTURE_NAME === "vue-parity") return "vue";
  if (FIXTURE_NAME === "svelte-parity") return "svelte";
  return null;
}

suite(`Code action apply [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    this.timeout(60_000);
    const fw = framework();
    if (!fw) {
      throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    }
    await ensureParityReady(fw === "vue" ? "src/App.vue" : "src/App.svelte");
  });

  registerFrameworkTest("vue", "shared.code-action.apply.organize-imports", async function () {
    this.timeout(45_000);
    try {
      const doc = await openRelative("src/features/OrganizeImports.vue");
      await vscode.window.showTextDocument(doc);
      const before = doc.getText();
      const actions = await codeActionsForFile(
        "src/features/OrganizeImports.vue",
        vscode.CodeActionKind.SourceOrganizeImports,
      );
      const org = actions.find(
        (a): a is vscode.CodeAction =>
          "kind" in a && !!a.kind && a.kind.value.startsWith("source.organizeImports"),
      );
      if (!org) throw new Error(`no organizeImports action; count=${actions.length}`);
      if (org.edit) {
        const ok = await vscode.workspace.applyEdit(org.edit);
        if (!ok) throw new Error("organizeImports edit failed to apply");
        await sleep(200);
        const after = (await vscode.workspace.openTextDocument(doc.uri)).getText();
        // Either reordered imports or stable — must not corrupt template
        if (!after.includes("<template>") || !after.includes("computed")) {
          throw new Error("organizeImports corrupted SFC content");
        }
        // restore
        const restore = new vscode.WorkspaceEdit();
        const full = await vscode.workspace.openTextDocument(doc.uri);
        restore.replace(
          doc.uri,
          new vscode.Range(full.positionAt(0), full.positionAt(full.getText().length)),
          before,
        );
        await vscode.workspace.applyEdit(restore);
      } else if (org.command) {
        await vscode.commands.executeCommand(org.command.command, ...(org.command.arguments ?? []));
        await sleep(300);
        await vscode.commands.executeCommand("workbench.action.files.revert");
      } else {
        throw new Error("organizeImports action has neither edit nor command");
      }
    } catch (err) {
      try {
        await vscode.commands.executeCommand("workbench.action.files.revert");
      } catch {
        /* best-effort */
      }
      failParityGap(
        this,
        "shared.code-action.apply.organize-imports",
        "ISSUE-code-action-apply-organize",
        String(err),
        "product-gap",
      );
    }
  });

  test("shared.code-action.quickfix-or-source-present", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file =
      fw === "vue" ? "src/diagnostics/BadPropParent.vue" : "src/diagnostics/BadPropParent.svelte";
    try {
      const actions = await codeActionsForFile(file);
      // On a file with type errors, quickfix may appear; otherwise source actions may.
      if (!Array.isArray(actions)) throw new Error("non-array actions");
      // Soft presence: provider responds. Empty is a product gap on error files.
      if (actions.length === 0) {
        throw new Error("no code actions on diagnostic fixture");
      }
    } catch (err) {
      failParityGap(
        this,
        "shared.code-action.quickfix-or-source-present",
        "ISSUE-code-action-on-errors",
        String(err),
        "product-gap",
      );
    }
  });
});
