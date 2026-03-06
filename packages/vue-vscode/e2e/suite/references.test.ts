import { expect } from "chai";
import * as vscode from "vscode";
import {
  waitForExtensionReady,
  openVueFile,
  getAppVuePath,
  getReferences,
  findPosition,
  sleep,
  FIXTURE_NAME,
} from "../helpers";

suite(`References [${FIXTURE_NAME}]`, function () {
  this.timeout(90_000);

  let doc: vscode.TextDocument;

  suiteSetup(async function () {
    await waitForExtensionReady(60_000);
    doc = await openVueFile(getAppVuePath());
    await sleep(12_000);
  });

  test("Ref1: binding has multiple references", async function () {
    const pos = findPosition(doc, "const count = ref(0)", 6); // on "count"
    if (!pos) {
      this.skip();
      return;
    }

    const refs = await getReferences(doc.uri, pos);
    console.log(`    References for count: ${refs.length} location(s)`);

    // count is used in declaration, template ({{ count }}), doubled computed, :bar="count", etc.
    expect(refs.length, "should have at least 3 references").to.be.greaterThanOrEqual(3);

    // All references should be in the same file (single-file test)
    for (const ref of refs) {
      expect(ref.uri.fsPath, "reference should be in same file").to.equal(doc.uri.fsPath);
    }

    // Negative: should NOT reference .tsx files
    for (const ref of refs) {
      expect(ref.uri.fsPath, "should NOT reference .tsx").to.not.match(/\.vue\.tsx$/);
    }
  });

  test("Ref2: function has references in template and script", async function () {
    const pos = findPosition(doc, "function increment()", 9); // on "increment"
    if (!pos) {
      this.skip();
      return;
    }

    const refs = await getReferences(doc.uri, pos);
    console.log(`    References for increment: ${refs.length} location(s)`);

    // increment is used in declaration + @click="increment" (at least 2 occurrences)
    expect(refs.length, "should have at least 2 references").to.be.greaterThanOrEqual(2);
  });

  test("Ref3: imported function has references", async function () {
    const pos = findPosition(doc, "formatCount", 0);
    if (!pos) {
      this.skip();
      return;
    }

    const refs = await getReferences(doc.uri, pos);
    console.log(`    References for formatCount: ${refs.length} location(s)`);

    // formatCount: import statement + usage in const formatted = formatCount(...)
    expect(refs.length, "should have at least 2 references").to.be.greaterThanOrEqual(2);
  });
});
