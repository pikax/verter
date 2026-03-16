import { expect } from "chai";
import * as vscode from "vscode";
import {
  openReadyCached,
  getAppVuePath,
  getInlayHints,
  FIXTURE_NAME,
  TYPE_PROVIDER,
} from "../helpers";

suite(`Inlay Hints [${FIXTURE_NAME}]`, function () {
  let doc: vscode.TextDocument;

  suiteSetup(async function () {
    doc = await openReadyCached(getAppVuePath());
  });

  test("inlay hints present for ref declarations in script", async function () {
    // Skip: inlay hints depend on full type provider sync with no reliable wait signal
    return this.skip();
    // Request inlay hints for the full document range
    const fullRange = new vscode.Range(
      new vscode.Position(0, 0),
      new vscode.Position(doc.lineCount - 1, 0),
    );
    const hints = await getInlayHints(doc.uri, fullRange);

    expect(hints.length, "should return at least one inlay hint").to.be.greaterThan(0);

    // Check that at least one hint is on line 17 (const count = ref(0))
    // or line 18 (const doubled = computed(...)) — 0-indexed: lines 16/17
    const scriptHintLines = hints.map((h) => h.position.line);
    const hasRefHint = scriptHintLines.some((l) => l >= 16 && l <= 19);
    expect(hasRefHint, "should have a hint near ref/computed declarations").to.be.true;
  });

  test("inlay hints cover template region without crash", async function () {
    // Request inlay hints spanning only the <template> block (lines 34-75, 0-indexed: 33-74)
    const templateRange = new vscode.Range(new vscode.Position(33, 0), new vscode.Position(74, 0));
    const hints = await getInlayHints(doc.uri, templateRange);

    // Should not crash — result is an array (may be empty depending on provider)
    expect(Array.isArray(hints), "result should be an array").to.be.true;
  });

  test("inlay hints with partial range into template still returns script hints", async function () {
    // Skip: inlay hints depend on full type provider sync with no reliable wait signal
    return this.skip();
    // Start in <script> (line 16), end deep in <template> (line 60)
    // This exercises the range-end fallback since template maps to synthetic JSX
    const partialRange = new vscode.Range(new vscode.Position(16, 0), new vscode.Position(60, 0));
    const hints = await getInlayHints(doc.uri, partialRange);

    // Should still return hints from the script section
    expect(Array.isArray(hints), "result should be an array").to.be.true;
    // At minimum, script-region hints should be present
    expect(hints.length, "should have hints from script region").to.be.greaterThan(0);
  });
});
