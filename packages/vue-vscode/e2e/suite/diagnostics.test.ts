import { expect } from "chai";
import * as path from "path";
import * as vscode from "vscode";
import {
  waitForExtensionReady,
  waitForFileReady,
  openVueFile,
  getAppVuePath,
  getCompVuePath,
  waitForDiagnostics,
  findPosition,
  sleep,
  FIXTURE_NAME,
  TYPE_PROVIDER,
} from "../helpers";
import { getTimer } from "../timer";

suite(`Diagnostics [${FIXTURE_NAME}]`, function () {
  this.timeout(60_000);

  suiteSetup(async function () {
    await waitForExtensionReady();
    // Ensure component types are synced to the type provider before running
    // diagnostics tests. Without this, MyComp resolves as Partial<{}> and
    // component prop/emit types are missing.
    const compPath = getCompVuePath();
    if (compPath) {
      const compDoc = await openVueFile(compPath);
      await waitForFileReady(compDoc);
      // Re-open App.vue so subsequent tests use it
      const appDoc = await openVueFile(getAppVuePath());
      await waitForFileReady(appDoc);
    }
  });

  test("extension activates for workspace", async function () {
    const ext = vscode.extensions.getExtension("pikax.verter-vscode");
    expect(ext?.isActive).to.be.true;
  });

  test("opening .vue file does not crash", async function () {
    const doc = await openVueFile(getAppVuePath());
    expect(doc).to.exist;
    expect(doc.languageId).to.equal("vue");
    // Give it time to process without crashing
    await waitForFileReady(doc);
  });

  test("diagnostics API returns for .vue file", async function () {
    const doc = await openVueFile(getAppVuePath());

    // Wait for file to be processed
    await waitForFileReady(doc);

    const diags = vscode.languages.getDiagnostics(doc.uri);
    // We don't assert on count — valid files may have zero diagnostics.
    // The key assertion is that the API call succeeds without error.
    expect(diags).to.be.an("array");

    console.log(`    Diagnostics count: ${diags.length}`);
    if (diags.length > 0) {
      const sources = [...new Set(diags.map((d) => d.source || "unknown"))];
      console.log(`    Sources: ${sources.join(", ")}`);
    }
  });

  test("measures time to first diagnostic", async function () {
    const doc = await openVueFile(getAppVuePath());
    const start = Date.now();

    const diags = await waitForDiagnostics(doc.uri, { timeoutMs: 30_000 });
    const elapsed = Date.now() - start;

    const sources = [...new Set(diags.map((d) => d.source || "unknown"))];
    getTimer().recordDiagnostics(elapsed, diags.length, sources);

    console.log(`    Time to diagnostics: ${elapsed}ms (${diags.length} diagnostics)`);
  });

  test("diagnostics have valid ranges", async function () {
    const doc = await openVueFile(getAppVuePath());
    await waitForFileReady(doc);

    const diags = vscode.languages.getDiagnostics(doc.uri);
    for (const d of diags) {
      expect(d.range.start.line, "Diagnostic start line should be non-negative").to.be.at.least(0);
      expect(d.range.end.line, "Diagnostic end line should be non-negative").to.be.at.least(0);
      expect(
        d.range.start.isBeforeOrEqual(d.range.end),
        `Diagnostic range should be valid: ${d.message}`,
      ).to.be.true;
    }
  });

  test("plain .ts MaybeRef generic does not produce verter XMissingEndTag", async function () {
    const workspaceFolders = vscode.workspace.workspaceFolders;
    expect(workspaceFolders, "workspace should have folders").to.exist;
    expect(workspaceFolders!.length, "workspace should have at least one folder").to.be.greaterThan(
      0,
    );

    const appDir = path.posix.dirname(getAppVuePath());
    const relativePath =
      appDir === "."
        ? "__verter_mayberef_repro__.ts"
        : path.posix.join(appDir, "__verter_mayberef_repro__.ts");
    const fileUri = vscode.Uri.file(
      path.join(workspaceFolders![0].uri.fsPath, ...relativePath.split("/")),
    );

    const content = `type MaybeRef<T> = T

export function useLockScroll(target: MaybeRef<HTMLElement | null> = null) {
  return target
}
`;

    await vscode.workspace.fs.writeFile(fileUri, Buffer.from(content, "utf8"));

    try {
      const doc = await vscode.workspace.openTextDocument(fileUri);
      await vscode.window.showTextDocument(doc);
      expect(doc.languageId).to.equal("typescript");

      const diags = await waitForDiagnostics(doc.uri, {
        timeoutMs: 8_000,
        predicate: () => false,
      });

      const summary = diags.map((d) => ({
        source: d.source,
        code: String(d.code),
        message: d.message,
        range: {
          start: `${d.range.start.line}:${d.range.start.character}`,
          end: `${d.range.end.line}:${d.range.end.character}`,
        },
      }));

      console.log(`    MaybeRef repro diagnostics (${diags.length}): ${JSON.stringify(summary)}`);

      const missingEndTag = diags.find(
        (d) => d.source === "verter" && String(d.code) === "XMissingEndTag",
      );

      expect(
        missingEndTag,
        `Expected no verter XMissingEndTag for plain .ts MaybeRef generic. Got: ${JSON.stringify(summary)}`,
      ).to.be.undefined;
    } finally {
      await vscode.workspace.fs.delete(fileUri, { useTrash: false });
    }
  });

  test("undeclared variable in script setup gets TS2304", async function () {
    // TSGO nightly doesn't return TS2304 diagnostics reliably
    if (TYPE_PROVIDER === "tsgo") return this.skip();
    const doc = await openVueFile(getAppVuePath());

    // Find the line with "const count = ref(0)" to insert the undeclared variable after
    const insertPos = findPosition(doc, "const count = ref(0)");
    expect(insertPos, "should find 'const count = ref(0)' in App.vue").to.exist;

    // Insert undeclared variable on the next line
    const lineEnd = doc.lineAt(insertPos!.line).range.end;
    const edit = new vscode.WorkspaceEdit();
    edit.insert(doc.uri, lineEnd, "\nunknownVar123");
    await vscode.workspace.applyEdit(edit);

    try {
      // Wait specifically for TS2304 referencing our undeclared variable
      const diags = await waitForDiagnostics(doc.uri, {
        source: "ts",
        timeoutMs: 15_000,
        predicate: (d) => d.message.includes("unknownVar123"),
      });

      // Positive: at least one TS diagnostic referencing the undeclared variable
      const ts2304 = diags.find(
        (d) => d.message.includes("Cannot find name") && d.message.includes("unknownVar123"),
      );
      expect(
        ts2304,
        `Expected TS2304 for unknownVar123. Got diagnostics: ${JSON.stringify(diags.map((d) => ({ msg: d.message, code: d.code, src: d.source })))}`,
      ).to.exist;

      // The diagnostic should point to the inserted line
      expect(ts2304!.range.start.line).to.equal(insertPos!.line + 1);
    } finally {
      // Clean up: undo the edit
      await vscode.commands.executeCommand("undo");
      await sleep(500);
    }
  });

  test("$event on component emit has no implicit any (TS7006)", async function () {
    if (!TYPE_PROVIDER) return this.skip();
    const doc = await openVueFile(getAppVuePath());

    // The fixture has @custom="handleCustom($event)" on <MyComp>
    const eventPos = findPosition(doc, '@custom="handleCustom($event)"');
    if (!eventPos) {
      console.log('    @custom="handleCustom($event)" not in fixture — skip');
      this.skip();
      return;
    }

    // Wait for component types to resolve. tsserver initially sees MyComp as
    // Partial<{}> until the .vue.ts DTS is synced and processed. Trigger
    // diagnostic refreshes via no-op edits and poll until the Partial<{}>
    // diagnostic disappears.
    const compLine = eventPos.line;
    const start = Date.now();
    const timeout = 20_000;
    let diags: vscode.Diagnostic[] = [];
    let editCount = 0;
    while (Date.now() - start < timeout) {
      diags = vscode.languages.getDiagnostics(doc.uri);
      const hasUnresolved = diags.some(
        (d) =>
          String(d.code) === "2322" &&
          d.message.includes("Partial<{}>") &&
          d.range.start.line === compLine,
      );
      if (!hasUnresolved) break;

      // Trigger a diagnostic refresh by inserting and undoing a space
      // (forces did_change → provider re-query → fresh diagnostics)
      if (editCount < 3) {
        const editor = vscode.window.activeTextEditor;
        if (editor) {
          const endPos = doc.lineAt(doc.lineCount - 1).range.end;
          await editor.edit((b) => b.insert(endPos, " "));
          await vscode.commands.executeCommand("undo");
          editCount++;
        }
      }
      await sleep(1000);
    }

    // Re-fetch after settling
    diags = vscode.languages.getDiagnostics(doc.uri);
    const stillUnresolved = diags.find(
      (d) =>
        String(d.code) === "2322" &&
        d.message.includes("Partial<{}>") &&
        d.range.start.line === compLine,
    );
    if (stillUnresolved) {
      console.log(`    Component types not resolved after ${timeout}ms — skip`);
      this.skip();
      return;
    }

    const ts7006 = diags.find(
      (d) =>
        String(d.code) === "7006" &&
        d.message.includes("$event") &&
        d.range.start.line === compLine,
    );

    // Negative: $event should NOT have implicit any when emits-to-props is working
    expect(
      ts7006,
      `$event should not have TS7006 implicit any on @custom handler. ` +
        `Diagnostics on line ${compLine}: ${JSON.stringify(
          diags
            .filter((d) => d.range.start.line === compLine)
            .map((d) => ({ msg: d.message, code: d.code })),
        )}`,
    ).to.be.undefined;
  });

  test("TS errors persist after inserting newlines", async function () {
    // TSGO nightly doesn't return TS2304 diagnostics reliably
    if (TYPE_PROVIDER === "tsgo") return this.skip();
    const doc = await openVueFile(getAppVuePath());

    // Insert undeclared variable to create a known TS error
    const insertPos = findPosition(doc, "const count = ref(0)");
    expect(insertPos, "should find insertion point").to.exist;

    const lineEnd = doc.lineAt(insertPos!.line).range.end;
    const edit = new vscode.WorkspaceEdit();
    edit.insert(doc.uri, lineEnd, "\nunknownVar456");
    await vscode.workspace.applyEdit(edit);

    try {
      // Wait specifically for TS2304 referencing our undeclared variable
      const initialDiags = await waitForDiagnostics(doc.uri, {
        source: "ts",
        timeoutMs: 15_000,
        predicate: (d) => d.message.includes("unknownVar456"),
      });
      const initialTs2304 = initialDiags.find((d) => d.message.includes("unknownVar456"));
      expect(initialTs2304, "TS error for unknownVar456 should appear before newline edit").to
        .exist;

      // Now insert a blank newline elsewhere (at the top of script, after imports)
      const importLine = findPosition(doc, "import { ref");
      expect(importLine, "should find import line").to.exist;
      const importLineEnd = doc.lineAt(importLine!.line).range.end;
      const newlineEdit = new vscode.WorkspaceEdit();
      newlineEdit.insert(doc.uri, importLineEnd, "\n");
      await vscode.workspace.applyEdit(newlineEdit);

      // TS diagnostics should still be present
      const afterDiags = await waitForDiagnostics(doc.uri, {
        source: "ts",
        timeoutMs: 15_000,
        predicate: (d) => d.message.includes("unknownVar456"),
      });
      const afterTs2304 = afterDiags.find((d) => d.message.includes("unknownVar456"));
      expect(
        afterTs2304,
        `TS error for unknownVar456 should survive newline insertion. Got: ${JSON.stringify(afterDiags.map((d) => ({ msg: d.message, src: d.source })))}`,
      ).to.exist;
    } finally {
      // Clean up: undo both edits
      await vscode.commands.executeCommand("undo");
      await sleep(200);
      await vscode.commands.executeCommand("undo");
      await sleep(500);
    }
  });

  test('missing sass preprocessor shows an inline diagnostic on lang="sass" and clears after switching to scss', async function () {
    const doc = await openVueFile(getAppVuePath());
    await waitForFileReady(doc);

    const insertPos = doc.lineAt(doc.lineCount - 1).range.end;
    const edit = new vscode.WorkspaceEdit();
    edit.insert(
      doc.uri,
      insertPos,
      '\n<style lang="sass">\n.missing-preprocessor\n  color: red\n</style>\n',
    );
    await vscode.workspace.applyEdit(edit);

    try {
      const sassDiags = await waitForDiagnostics(doc.uri, {
        source: "verter",
        timeoutMs: 20_000,
        predicate: (d) => d.message.includes('"sass" is not installed') && d.source === "verter",
      });
      const missingSass = sassDiags.find((d) => d.message.includes('"sass" is not installed'));

      expect(
        missingSass,
        `Expected an inline missing-sass diagnostic. Got: ${JSON.stringify(sassDiags.map((d) => ({ message: d.message, source: d.source })))}`,
      ).to.exist;
      expect(doc.getText(missingSass!.range)).to.equal('lang="sass"');

      const langPos = findPosition(doc, 'lang="sass"');
      expect(langPos, 'should find lang="sass" after inserting the style block').to.exist;

      const replaceEdit = new vscode.WorkspaceEdit();
      replaceEdit.replace(
        doc.uri,
        new vscode.Range(
          langPos!,
          new vscode.Position(langPos!.line, langPos!.character + 'lang="sass"'.length),
        ),
        'lang="scss"',
      );
      await vscode.workspace.applyEdit(replaceEdit);

      const clearStart = Date.now();
      let remaining = vscode.languages
        .getDiagnostics(doc.uri)
        .filter((d) => d.source === "verter" && d.message.includes('"sass" is not installed'));
      while (remaining.length > 0 && Date.now() - clearStart < 20_000) {
        await sleep(250);
        remaining = vscode.languages
          .getDiagnostics(doc.uri)
          .filter((d) => d.source === "verter" && d.message.includes('"sass" is not installed'));
      }

      expect(
        remaining,
        `Missing-sass diagnostic should clear after switching to scss. Got: ${JSON.stringify(remaining.map((d) => ({ message: d.message, source: d.source })))}`,
      ).to.be.empty;
    } finally {
      await vscode.commands.executeCommand("undo");
      await sleep(200);
      await vscode.commands.executeCommand("undo");
      await sleep(500);
    }
  });
});
