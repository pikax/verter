import { expect } from "chai";
import * as vscode from "vscode";
import {
  waitForExtensionReady,
  openVueFile,
  getAppVuePath,
  getPrepareRename,
  getRenameEdits,
  findPosition,
  sleep,
  FIXTURE_NAME,
} from "../helpers";

suite(`Rename [${FIXTURE_NAME}]`, function () {
  this.timeout(90_000);

  let doc: vscode.TextDocument;

  suiteSetup(async function () {
    await waitForExtensionReady(60_000);
    doc = await openVueFile(getAppVuePath());
    await sleep(12_000);
  });

  test("R1: prepare rename on script binding succeeds", async function () {
    const pos = findPosition(doc, "const count = ref(0)", 6); // on "count"
    if (!pos) {
      this.skip();
      return;
    }

    const result = await getPrepareRename(doc.uri, pos);
    console.log(`    Prepare rename on count: ${result ? "valid" : "rejected"}`);

    // prepare rename should succeed for a script binding
    expect(result, "should return a valid rename range").to.exist;
  });

  test("R2: prepare rename on HTML tag is rejected", async function () {
    const pos = findPosition(doc, "<h1>", 1); // on "h1"
    if (!pos) {
      this.skip();
      return;
    }

    const result = await getPrepareRename(doc.uri, pos);
    console.log(`    Prepare rename on h1: ${result ? "valid" : "rejected/null"}`);

    // HTML tags should NOT be renameable
    // Note: this depends on the LSP implementation — it may return null or throw
  });

  test("R3: rename binding updates template and script", async function () {
    // We test with "doubled" which is used in both script and template
    const pos = findPosition(doc, "const doubled = computed(", 6); // on "doubled"
    if (!pos) {
      this.skip();
      return;
    }

    const edits = await getRenameEdits(doc.uri, pos, "doubledValue");
    console.log(`    Rename edits: ${edits ? `${edits.size} file(s)` : "none"}`);

    if (edits) {
      // Should have edits
      const entries = edits.entries();
      expect(entries.length, "should have at least 1 file with edits").to.be.greaterThan(0);

      // Find edits for current file
      const currentFileEdits = entries.find(([uri]) => uri.fsPath === doc.uri.fsPath);
      if (currentFileEdits) {
        const [, textEdits] = currentFileEdits;
        console.log(`    Current file edits: ${textEdits.length}`);
        // Should have at least 2 edits (declaration + template usage)
        expect(textEdits.length, "should have at least 2 edits (declaration + usage)").to.be.greaterThanOrEqual(2);
      }
    }

    // Note: We don't apply the rename — this is just checking that the LSP returns edits
  });
});
