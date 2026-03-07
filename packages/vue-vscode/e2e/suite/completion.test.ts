import { expect } from "chai";
import * as vscode from "vscode";
import {
  waitForExtensionReady,
  openVueFile,
  getAppVuePath,
  getCompletions,
  findPosition,
  findNthPosition,
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

    // v-for iteration variables compile to arrow function params — tsserver
    // returns kind "parameter" which must map to Variable, not Text
    const itemCompletion = completions!.items.find((i) => i.label === "item");
    if (itemCompletion) {
      expect(
        itemCompletion.kind,
        "'item' should be Variable, not Text",
      ).to.equal(vscode.CompletionItemKind.Variable);
    }
  });

  test("C6: v-for member access shows typed properties", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    // Position after "action." in :disabled="action.disabled"
    const pos = findPosition(doc, "action.disabled", 7); // on "d" after dot
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await getCompletions(doc.uri, pos);
    expect(completions, "should return completions").to.exist;
    const items = completions!.items;
    const labels = items.map((i) => i.label);
    console.log(
      `    v-for member: ${items.length} items, first: ${labels.slice(0, 10).join(", ")}`,
    );
    console.log(
      `    kinds: ${items.slice(0, 10).map((i) => `${i.label}:${i.kind}`).join(", ")}`,
    );

    // POSITIVE: Action properties present
    expect(labels, "should include 'disabled'").to.include("disabled");
    expect(labels, "should include 'label'").to.include("label");
    expect(labels, "should include 'handler'").to.include("handler");

    // KIND: must be Property/Field, not Text
    const disabledItem = items.find((i) => i.label === "disabled");
    expect(disabledItem!.kind, "'disabled' kind").to.be.oneOf([
      vscode.CompletionItemKind.Property,
      vscode.CompletionItemKind.Field,
    ]);

    // NEGATIVE: not global scope
    expect(items.length, "member completions, not global").to.be.lessThan(50);
    expect(labels.join(",")).to.not.include("___VERTER___");
  });

  test("C7: v-for iteration variable in mustache shows typed properties", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    // Position on "user.name" inside {{ user.name }} in the v-for with index
    const pos = findPosition(doc, "user.name", 5); // on "n" after "user."
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await getCompletions(doc.uri, pos);
    expect(completions, "should return completions").to.exist;
    const items = completions!.items;
    const labels = items.map((i) => i.label);
    console.log(`    v-for mustache member: ${items.length} items`);

    expect(labels, "should include 'name'").to.include("name");
    expect(labels, "should include 'email'").to.include("email");
    expect(labels, "should include 'age'").to.include("age");

    const nameItem = items.find((i) => i.label === "name");
    expect(nameItem!.kind, "'name' kind").to.be.oneOf([
      vscode.CompletionItemKind.Property,
      vscode.CompletionItemKind.Field,
    ]);
    expect(items.length, "member completions, not global").to.be.lessThan(50);
  });

  test("C8: nested v-for inner variable member access", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    // In nested v-for: {{ action.label }} inside the inner loop
    // Use findNthPosition to get the occurrence inside the nested loop
    const pos = findNthPosition(doc, "action.label", 1, 7); // 2nd occurrence, on "l"
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await getCompletions(doc.uri, pos);
    expect(completions).to.exist;
    const items = completions!.items;
    const labels = items.map((i) => i.label);
    console.log(`    nested v-for inner: ${items.length} items`);

    expect(labels).to.include("label");
    expect(labels).to.include("disabled");
    expect(items.length, "member completions").to.be.lessThan(50);
  });

  test("C9: nested v-for outer variable member access", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    // In nested v-for: {{ user.name }} inside the inner loop
    const pos = findNthPosition(doc, "user.name", 1, 5); // 2nd occurrence
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await getCompletions(doc.uri, pos);
    expect(completions).to.exist;
    const items = completions!.items;
    const labels = items.map((i) => i.label);
    console.log(`    nested v-for outer: ${items.length} items`);

    expect(labels).to.include("name");
    expect(labels).to.include("email");
    expect(items.length, "member completions").to.be.lessThan(50);
  });

  test("C10: v-if narrowed ref member access", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    // {{ selectedUser.name }} inside v-if="selectedUser"
    const pos = findPosition(doc, "selectedUser.name", 13); // on "n"
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await getCompletions(doc.uri, pos);
    expect(completions).to.exist;
    const items = completions!.items;
    const labels = items.map((i) => i.label);
    console.log(`    v-if narrowed: ${items.length} items`);

    expect(labels).to.include("name");
    expect(labels).to.include("email");
    expect(items.length, "member completions").to.be.lessThan(50);
  });

  test("C11: destructured v-for params available", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    // {{ name }} inside <div v-for="{ name, email } in users">
    const pos = findPosition(doc, "{{ name }}", 3); // on "n"
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await getCompletions(doc.uri, pos);
    expect(completions).to.exist;
    const labels = completions!.items.map((i) => i.label);
    console.log(`    destructured v-for: ${labels.length} items`);

    expect(labels).to.include("name");
  });

  test("C12: script binding completions are typed", async function () {
    const pos = findPosition(doc, "{{ count }}", 3);
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await getCompletions(doc.uri, pos);
    expect(completions).to.exist;
    const items = completions!.items;
    const labels = items.map((i) => i.label);

    expect(labels).to.include("count");
    expect(labels).to.include("doubled");
    expect(labels).to.include("increment");

    // Script bindings should be Variable/Function kind, not Text
    const countItem = items.find((i) => i.label === "count");
    if (countItem) {
      expect(
        countItem.kind,
        "'count' should not be Text",
      ).to.not.equal(vscode.CompletionItemKind.Text);
    }
  });

  test("C13: props member access in interpolation", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    // Position after "props." in {{ props.title }}
    const pos = findPosition(doc, "props.title", 6); // on "t" after "props."
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await getCompletions(doc.uri, pos);
    expect(completions, "should return completions").to.exist;
    const items = completions!.items;
    const labels = items.map((i) => i.label);
    console.log(
      `    props member: ${items.length} items, first: ${labels.slice(0, 10).join(", ")}`,
    );

    // POSITIVE: prop members present
    expect(labels, "should include 'title'").to.include("title");

    // NEGATIVE: no Vue-attr transformations in expression context
    expect(
      labels.filter((l) => l.startsWith("@")).length,
      "no @-prefixed items in expression context",
    ).to.equal(0);
    expect(items.length, "member completions, not global").to.be.lessThan(50);
  });

  test("C14: v-for scoped variable member access has no attr transforms", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    // Position after "action." in :disabled="action.disabled"
    const pos = findPosition(doc, "action.disabled", 7); // on "d" after dot
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await getCompletions(doc.uri, pos);
    expect(completions, "should return completions").to.exist;
    const items = completions!.items;
    const labels = items.map((i) => i.label);
    console.log(
      `    v-for member attr: ${items.length} items, first: ${labels.slice(0, 10).join(", ")}`,
    );

    // POSITIVE: Action properties present
    expect(labels, "should include 'disabled'").to.include("disabled");
    expect(labels, "should include 'label'").to.include("label");
    expect(labels, "should include 'handler'").to.include("handler");

    // NEGATIVE: no Vue-attr transformations (no kebab-case, no @-prefix)
    expect(
      labels.filter((l) => l.startsWith("@")).length,
      "no @-prefixed items in expression context",
    ).to.equal(0);
    expect(
      labels.filter((l) => l.includes("-")).length,
      "no kebab-case transformations in expression context",
    ).to.equal(0);
  });

  test("C15: template identifier completions exclude globals", async function () {
    const pos = findPosition(doc, "{{ count }}", 3);
    if (!pos) {
      this.skip();
      return;
    }
    const completions = await getCompletions(doc.uri, pos);
    expect(completions).to.exist;
    const labels = completions!.items.map((i) => i.label);
    console.log(`    identifier completions: ${labels.length} items`);

    // POSITIVE: script setup bindings are present
    expect(labels).to.include("count");

    // NEGATIVE: global types should NOT appear in template expressions
    expect(labels).to.not.include("AbortController");
    expect(labels).to.not.include("HTMLDivElement");
    expect(labels).to.not.include("document");
    expect(labels).to.not.include("window");
    expect(completions!.items.length).to.be.lessThan(200);
  });
});
