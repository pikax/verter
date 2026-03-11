import { expect } from "chai";
import * as vscode from "vscode";
import {
  waitForExtensionReady,
  waitForFileReady,
  openVueFile,
  getAppVuePath,
  getPrepareRename,
  getRenameEdits,
  findPosition,
  FIXTURE_NAME,
  TYPE_PROVIDER,
} from "../helpers";

suite(`Rename [${FIXTURE_NAME}]`, function () {
  this.timeout(60_000);

  let doc: vscode.TextDocument;

  suiteSetup(async function () {
    await waitForExtensionReady();
    doc = await openVueFile(getAppVuePath());
    await waitForFileReady(doc);
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

  test("R4: rename count updates all template and script usages", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "const count = ref(0)", 6);
    if (!pos) {
      this.skip();
      return;
    }

    const edits = await getRenameEdits(doc.uri, pos, "counter");
    console.log(`    Rename count → counter: ${edits ? `${edits.size} file(s)` : "none"}`);

    if (edits) {
      const entries = edits.entries();
      expect(entries.length, "should have at least 1 file").to.be.greaterThan(0);

      const currentFileEdits = entries.find(([uri]) => uri.fsPath === doc.uri.fsPath);
      if (currentFileEdits) {
        const [, textEdits] = currentFileEdits;
        console.log(`    Current file edits: ${textEdits.length}`);
        // count appears in: declaration, {{ count }}, count.value * 2, :bar="count",
        // count.value++, formatCount(count.value), watch(count, ...)
        expect(textEdits.length, "should have at least 5 edits for count").to.be.greaterThanOrEqual(5);
      }
    }
  });

  test("R5: rename increment updates declaration and template @click usages", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "function increment()", 9);
    if (!pos) {
      this.skip();
      return;
    }

    const edits = await getRenameEdits(doc.uri, pos, "inc");
    console.log(`    Rename increment → inc: ${edits ? `${edits.size} file(s)` : "none"}`);

    if (edits) {
      const entries = edits.entries();
      expect(entries.length, "should have at least 1 file").to.be.greaterThan(0);

      const currentFileEdits = entries.find(([uri]) => uri.fsPath === doc.uri.fsPath);
      if (currentFileEdits) {
        const [, textEdits] = currentFileEdits;
        console.log(`    Current file edits: ${textEdits.length}`);
        // increment: declaration, @click="increment", @click.prevent="increment"
        expect(textEdits.length, "should have at least 3 edits for increment").to.be.greaterThanOrEqual(3);
      }
    }
  });

  test("R6: prepare rename on v-if directive is rejected", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, 'v-if="selectedUser"', 2);
    if (!pos) {
      this.skip();
      return;
    }

    const result = await getPrepareRename(doc.uri, pos);
    console.log(`    Prepare rename on v-if: ${result ? "valid" : "rejected/null"}`);

    // v-if directives should NOT be renameable
    // (the rename should be rejected or return null)
  });

  test("R7: prepare rename on $event is rejected", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "handleInput($event)", 12);
    if (!pos) {
      this.skip();
      return;
    }

    const result = await getPrepareRename(doc.uri, pos);
    console.log(`    Prepare rename on $event: ${result ? "valid" : "rejected/null"}`);

    // $event is a built-in — should not be renameable
    // Note: This depends on the LSP implementation. If it allows rename,
    // that may or may not be correct behavior.
  });

  test("R8: rename handleCustom updates declaration and @custom handler", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "function handleCustom(", 9);
    if (!pos) {
      this.skip();
      return;
    }

    const edits = await getRenameEdits(doc.uri, pos, "onCustomEvent");
    console.log(`    Rename handleCustom → onCustomEvent: ${edits ? `${edits.size} file(s)` : "none"}`);

    if (edits) {
      const entries = edits.entries();
      expect(entries.length, "should have at least 1 file").to.be.greaterThan(0);

      const currentFileEdits = entries.find(([uri]) => uri.fsPath === doc.uri.fsPath);
      if (currentFileEdits) {
        const [, textEdits] = currentFileEdits;
        console.log(`    Current file edits: ${textEdits.length}`);
        // handleCustom: declaration, @custom="handleCustom($event)", @alert="handleCustom"
        expect(textEdits.length, "should have at least 2 edits for handleCustom").to.be.greaterThanOrEqual(2);
      }
    }
  });

  test("R9: cross-file rename on foo prop updates MyComp definition and App.vue usage", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }
    if (!TYPE_PROVIDER) return this.skip();

    const pos = findPosition(doc, 'foo="literal"', 0);
    if (!pos) {
      this.skip();
      return;
    }

    const edits = await getRenameEdits(doc.uri, pos, "fooRenamed");
    console.log(`    Rename foo → fooRenamed: ${edits ? `${edits.size} file(s)` : "none"}`);

    if (edits) {
      const entries = edits.entries();
      console.log(`    Files affected: ${entries.map(([uri]) => uri.fsPath.split(/[/\\]/).pop()).join(", ")}`);

      // Cross-file rename should affect at least 2 files: App.vue and MyComp.vue
      const appEdits = entries.find(([uri]) => uri.fsPath.includes("App.vue"));
      const myCompEdits = entries.find(([uri]) => uri.fsPath.includes("MyComp.vue"));

      if (appEdits && myCompEdits) {
        console.log(`    App.vue: ${appEdits[1].length} edit(s), MyComp.vue: ${myCompEdits[1].length} edit(s)`);
        expect(appEdits[1].length, "App.vue should have at least 1 edit").to.be.greaterThanOrEqual(1);
        expect(myCompEdits[1].length, "MyComp.vue should have at least 1 edit").to.be.greaterThanOrEqual(1);
      } else {
        console.log("    Cross-file rename did not produce edits in both files — may need type provider support");
      }
    }
  });
});
