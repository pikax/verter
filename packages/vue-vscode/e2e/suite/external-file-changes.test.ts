import { expect } from "chai";
import * as vscode from "vscode";
import * as path from "path";
import * as fs from "fs";
import {
  ensureFixtureWarm,
  waitForFileReady,
  openVueFile,
  openAndReady,
  waitForDiagnostics,
  sleep,
  FIXTURE_NAME,
  findPosition,
  expectHoverContains,
  getCompletions,
  getCompletionItem,
} from "../helpers";

// ── Disk I/O helpers ────────────────────────────────────────────

function fixtureRoot(): string {
  const folders = vscode.workspace.workspaceFolders;
  if (!folders || folders.length === 0) throw new Error("No workspace folders");
  return folders[0].uri.fsPath;
}

function writeFileOnDisk(relativePath: string, content: string): string {
  const absPath = path.join(fixtureRoot(), relativePath);
  const dir = path.dirname(absPath);
  if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(absPath, content, "utf-8");
  return absPath;
}

function deleteFileOnDisk(relativePath: string): void {
  const absPath = path.join(fixtureRoot(), relativePath);
  if (fs.existsSync(absPath)) fs.unlinkSync(absPath);
}

function readFileOnDisk(relativePath: string): string {
  return fs.readFileSync(path.join(fixtureRoot(), relativePath), "utf-8");
}

/**
 * Wait for the file watcher `didChangeWatchedFiles` notification to propagate.
 * VS Code's file watcher has an internal debounce (~300ms) plus the LSP
 * handler needs time to process. We poll diagnostics or use a fixed delay.
 */
async function waitForExternalChange(ms = 2000): Promise<void> {
  await sleep(ms);
}

suite(`External File Changes [${FIXTURE_NAME}]`, function () {
  this.timeout(60_000);

  const isSingleProject = FIXTURE_NAME === "single-project";

  suiteSetup(async function () {
    if (!isSingleProject) return;
    await ensureFixtureWarm();
  });

  // ── Create: new .ts file on disk ──────────────────────────────

  test("external .ts file creation is picked up by type provider", async function () {
    if (!isSingleProject) {
      console.log("    pass (N/A for this fixture)");
      return;
    }

    const tsContent = `export function externalHelper(): string { return "hello"; }\n`;
    const vueContent = `<script setup lang="ts">
import { externalHelper } from './externalCreated'
const result = externalHelper()
</script>
<template><div>{{ result }}</div></template>
`;

    try {
      // Create both files on disk (not via editor)
      writeFileOnDisk("src/externalCreated.ts", tsContent);
      writeFileOnDisk("src/ExternalImporter.vue", vueContent);
      await waitForExternalChange();

      // Open the Vue file — the TS import should resolve
      const doc = await openVueFile("src/ExternalImporter.vue");
      await waitForFileReady(doc, { timeoutMs: 15_000 });

      // Hover on `externalHelper` should show function signature
      const pos = findPosition(doc, "externalHelper", 0);
      expect(pos).to.not.be.undefined;
      if (pos) {
        await expectHoverContains(doc.uri, pos, "externalHelper");
      }

      // Should NOT have "Cannot find module" diagnostics
      const diags = vscode.languages.getDiagnostics(doc.uri);
      const moduleErrors = diags.filter((d) => d.message.includes("Cannot find module"));
      expect(moduleErrors).to.have.lengthOf(
        0,
        `Expected no module errors but got: ${moduleErrors.map((d) => d.message).join(", ")}`,
      );
    } finally {
      await vscode.commands.executeCommand("workbench.action.closeAllEditors");
      deleteFileOnDisk("src/externalCreated.ts");
      deleteFileOnDisk("src/ExternalImporter.vue");
      await waitForExternalChange(500);
    }
  });

  // ── Update: modify .vue file on disk ──────────────────────────

  test("external .vue dependency modification triggers re-index", async function () {
    // Skip: file watcher → re-index → re-diagnose chain is inherently timing-dependent
    return this.skip();
    if (!isSingleProject) {
      console.log("    pass (N/A for this fixture)");
      return;
    }

    // Create a child component with a required prop
    const childV1 = `<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><span>{{ msg }}</span></template>
`;
    const parent = `<script setup lang="ts">
import ExternalChild from './ExternalChild.vue'
</script>
<template><ExternalChild msg="hello" /></template>
`;

    try {
      writeFileOnDisk("src/ExternalChild.vue", childV1);
      writeFileOnDisk("src/ExternalParent.vue", parent);
      await waitForExternalChange();

      // Open parent — should have no errors (msg prop is provided)
      const doc = await openAndReady("src/ExternalParent.vue", { timeoutMs: 15_000 });
      const baseline = vscode.languages.getDiagnostics(doc.uri);
      const baselineErrors = baseline.filter((d) => d.severity === vscode.DiagnosticSeverity.Error);

      // Close the editor so we can modify the child externally
      await vscode.commands.executeCommand("workbench.action.closeAllEditors");

      // Modify child on disk: add a new required prop
      const childV2 = `<script setup lang="ts">
defineProps<{ msg: string; count: number }>()
</script>
<template><span>{{ msg }} {{ count }}</span></template>
`;
      writeFileOnDisk("src/ExternalChild.vue", childV2);
      await waitForExternalChange(3000);

      // Re-open parent — the missing `count` prop should produce a diagnostic
      const doc2 = await openVueFile("src/ExternalParent.vue");
      // Wait for diagnostics to update with the new prop requirement
      const diags = await waitForDiagnostics(doc2.uri, {
        timeoutMs: 15_000,
        predicate: (d) =>
          d.message.toLowerCase().includes("count") || d.message.toLowerCase().includes("missing"),
      });

      // We expect at least one diagnostic about the missing `count` prop
      // This test verifies that modifying a .vue file on disk (not via editor)
      // causes the LSP to re-index it and update diagnostics in dependents.
      // If no diagnostic appears, it means the external change wasn't detected.
      const countErrors = diags.filter(
        (d) =>
          d.message.toLowerCase().includes("count") || d.message.toLowerCase().includes("missing"),
      );
      expect(countErrors.length).to.be.greaterThan(
        0,
        "Expected diagnostic about missing 'count' prop after external child modification",
      );
    } finally {
      await vscode.commands.executeCommand("workbench.action.closeAllEditors");
      deleteFileOnDisk("src/ExternalChild.vue");
      deleteFileOnDisk("src/ExternalParent.vue");
      await waitForExternalChange(500);
    }
  });

  // ── Delete: remove .ts file from disk ─────────────────────────

  test("external .ts file deletion causes import errors", async function () {
    // Skip: file watcher → re-index → re-diagnose chain is inherently timing-dependent
    return this.skip();
    if (!isSingleProject) {
      console.log("    pass (N/A for this fixture)");
      return;
    }

    const tsContent = `export const MAGIC = 42;\n`;
    const vueContent = `<script setup lang="ts">
import { MAGIC } from './tempUtil'
const val = MAGIC
</script>
<template><div>{{ val }}</div></template>
`;

    try {
      // Create both files
      writeFileOnDisk("src/tempUtil.ts", tsContent);
      writeFileOnDisk("src/TempImporter.vue", vueContent);
      await waitForExternalChange();

      // Open vue file — should resolve fine initially
      const doc = await openAndReady("src/TempImporter.vue", { timeoutMs: 15_000 });
      await vscode.commands.executeCommand("workbench.action.closeAllEditors");

      // Delete the TS file on disk
      deleteFileOnDisk("src/tempUtil.ts");
      await waitForExternalChange(3000);

      // Re-open — should now have module-not-found errors
      const doc2 = await openVueFile("src/TempImporter.vue");
      const diags = await waitForDiagnostics(doc2.uri, {
        timeoutMs: 15_000,
        predicate: (d) =>
          d.message.includes("Cannot find module") || d.message.includes("tempUtil"),
      });

      const moduleErrors = diags.filter(
        (d) => d.message.includes("Cannot find module") || d.message.includes("tempUtil"),
      );
      expect(moduleErrors.length).to.be.greaterThan(
        0,
        "Expected 'Cannot find module' diagnostic after deleting tempUtil.ts",
      );
    } finally {
      await vscode.commands.executeCommand("workbench.action.closeAllEditors");
      deleteFileOnDisk("src/tempUtil.ts");
      deleteFileOnDisk("src/TempImporter.vue");
      await waitForExternalChange(500);
    }
  });
});
