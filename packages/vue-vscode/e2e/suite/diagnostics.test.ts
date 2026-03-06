import { expect } from "chai";
import * as vscode from "vscode";
import {
  waitForExtensionReady,
  openVueFile,
  getAppVuePath,
  waitForDiagnostics,
  findPosition,
  sleep,
  FIXTURE_NAME,
} from "../helpers";
import { getTimer } from "../timer";

suite(`Diagnostics [${FIXTURE_NAME}]`, function () {
  this.timeout(90_000);

  suiteSetup(async function () {
    await waitForExtensionReady(60_000);
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
    await sleep(3_000);
  });

  test("diagnostics API returns for .vue file", async function () {
    const doc = await openVueFile(getAppVuePath());

    // Give the LSP time to process
    await sleep(5_000);

    const diags = vscode.languages.getDiagnostics(doc.uri);
    // We don't assert on count — valid files may have zero diagnostics.
    // The key assertion is that the API call succeeds without error.
    expect(diags).to.be.an("array");

    console.log(
      `    Diagnostics count: ${diags.length}`,
    );
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

    console.log(
      `    Time to diagnostics: ${elapsed}ms (${diags.length} diagnostics)`,
    );
  });

  test("diagnostics have valid ranges", async function () {
    const doc = await openVueFile(getAppVuePath());
    await sleep(5_000);

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

  test("undeclared variable in script setup gets TS2304", async function () {
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
      // Wait for TS diagnostics to appear (source: "ts")
      const diags = await waitForDiagnostics(doc.uri, {
        source: "ts",
        minCount: 1,
        timeoutMs: 15_000,
      });

      // Positive: at least one TS diagnostic referencing the undeclared variable
      const ts2304 = diags.find(
        (d) =>
          d.message.includes("Cannot find name") &&
          d.message.includes("unknownVar123"),
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
    const doc = await openVueFile(getAppVuePath());

    // The fixture has @custom="handleCustom($event)" on <MyComp>
    const eventPos = findPosition(doc, '@custom="handleCustom($event)"');
    if (!eventPos) {
      console.log('    @custom="handleCustom($event)" not in fixture — skip');
      this.skip();
      return;
    }

    // Wait for diagnostics to settle
    await sleep(8_000);

    const diags = vscode.languages.getDiagnostics(doc.uri);
    const ts7006 = diags.find(
      (d) =>
        String(d.code) === "7006" &&
        d.message.includes("$event") &&
        d.range.start.line === eventPos.line,
    );

    // Negative: $event should NOT have implicit any when emits-to-props is working
    expect(
      ts7006,
      `$event should not have TS7006 implicit any on @custom handler. ` +
        `Diagnostics on line ${eventPos.line}: ${JSON.stringify(
          diags
            .filter((d) => d.range.start.line === eventPos.line)
            .map((d) => ({ msg: d.message, code: d.code })),
        )}`,
    ).to.be.undefined;
  });

  test("TS errors persist after inserting newlines", async function () {
    const doc = await openVueFile(getAppVuePath());

    // Insert undeclared variable to create a known TS error
    const insertPos = findPosition(doc, "const count = ref(0)");
    expect(insertPos, "should find insertion point").to.exist;

    const lineEnd = doc.lineAt(insertPos!.line).range.end;
    const edit = new vscode.WorkspaceEdit();
    edit.insert(doc.uri, lineEnd, "\nunknownVar456");
    await vscode.workspace.applyEdit(edit);

    try {
      // Wait for the TS error to appear
      const initialDiags = await waitForDiagnostics(doc.uri, {
        source: "ts",
        minCount: 1,
        timeoutMs: 15_000,
      });
      const initialTs2304 = initialDiags.find((d) =>
        d.message.includes("unknownVar456"),
      );
      expect(
        initialTs2304,
        "TS error for unknownVar456 should appear before newline edit",
      ).to.exist;

      // Now insert a blank newline elsewhere (at the top of script, after imports)
      const importLine = findPosition(doc, "import { ref");
      expect(importLine, "should find import line").to.exist;
      const importLineEnd = doc.lineAt(importLine!.line).range.end;
      const newlineEdit = new vscode.WorkspaceEdit();
      newlineEdit.insert(doc.uri, importLineEnd, "\n");
      await vscode.workspace.applyEdit(newlineEdit);

      // Wait for diagnostics to settle after the newline insertion
      await sleep(3_000);

      // TS diagnostics should still be present
      const afterDiags = await waitForDiagnostics(doc.uri, {
        source: "ts",
        minCount: 1,
        timeoutMs: 15_000,
      });
      const afterTs2304 = afterDiags.find((d) =>
        d.message.includes("unknownVar456"),
      );
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
});
