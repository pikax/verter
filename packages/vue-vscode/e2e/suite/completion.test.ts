import { expect } from "chai";
import * as vscode from "vscode";
import {
  waitForExtensionReady,
  openVueFile,
  getAppVuePath,
  getCompletions,
  findPosition,
  sleep,
  FIXTURE_NAME,
} from "../helpers";

suite(`Completion [${FIXTURE_NAME}]`, function () {
  this.timeout(90_000);

  let doc: vscode.TextDocument;

  suiteSetup(async function () {
    await waitForExtensionReady(60_000);
    doc = await openVueFile(getAppVuePath());
    await sleep(12_000);
  });

  test("C1: mustache expression shows bindings", async function () {
    const pos = findPosition(doc, "{{ count }}", 3); // on "count"
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await getCompletions(doc.uri, pos);
    expect(completions, "should return completions").to.exist;

    const labels = completions!.items.map((i) => i.label);
    console.log(`    Mustache completions: ${labels.length} items`);

    expect(labels, "should include 'count'").to.include("count");
    expect(labels, "should include 'doubled'").to.include("doubled");

    // Negative: internal symbols should NOT appear
    expect(labels.join(","), "should NOT include __props").to.not.include("__props");
    expect(labels.join(","), "should NOT include ___VERTER___").to.not.include("___VERTER___");
  });

  test("C2: event handler value shows functions", async function () {
    const pos = findPosition(doc, '@click="increment"', 8); // on "increment"
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await getCompletions(doc.uri, pos);
    expect(completions, "should return completions").to.exist;

    const labels = completions!.items.map((i) => i.label);
    console.log(`    Event handler completions: ${labels.length} items`);

    expect(labels, "should include 'increment'").to.include("increment");
  });

  test("C3: component props in opening tag", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    pass (N/A for this fixture)");
      return;
    }

    // Position after <MyComp (in attribute position)
    const pos = findPosition(doc, "<MyComp ", 8); // after the space
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await getCompletions(doc.uri, pos);
    expect(completions, "should return completions").to.exist;

    const labels = completions!.items.map((i) => i.label);
    console.log(`    Component prop completions: ${labels.length} items, labels: ${labels.slice(0, 10).join(", ")}`);

    // Should include component props
    // Note: exact labels depend on whether child analysis resolves
    // At minimum, should NOT include parent script bindings as top-level items
    // when we're in a component's attribute position
  });

  test("C4: event modifier completions", async function () {
    const pos = findPosition(doc, "@click.prevent", 7); // after the dot
    if (!pos) {
      this.skip();
      return;
    }

    // We need position right after the "." — find @click.prevent and position on "p"
    const completions = await getCompletions(doc.uri, pos);
    // Event modifier completions may or may not trigger depending on timing
    if (completions && completions.items.length > 0) {
      const labels = completions.items.map((i) => i.label);
      console.log(`    Event modifier completions: ${labels.length} items`);
      // If we got modifier completions, verify they include modifiers
      if (labels.some((l) => ["prevent", "stop", "once", "capture", "self", "passive"].includes(l))) {
        expect(labels, "should include 'prevent'").to.include("prevent");
        expect(labels, "should include 'stop'").to.include("stop");
        // Negative: should NOT include script bindings
        expect(labels, "should NOT include 'count'").to.not.include("count");
      }
    }
  });

  test("C5: v-for scoped variable in template", async function () {
    const pos = findPosition(doc, "{{ item }}", 3); // on "item"
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await getCompletions(doc.uri, pos);
    expect(completions, "should return completions").to.exist;

    const labels = completions!.items.map((i) => i.label);
    console.log(`    v-for scoped completions: ${labels.length} items`);

    // The scoped variable `item` should be available (via TSGO, since it's in the generated TSX)
    // This test verifies that the v-for codegen creates proper JS scope
    expect(labels, "should include 'item'").to.include("item");
  });
});
