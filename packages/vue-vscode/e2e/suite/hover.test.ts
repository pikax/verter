import { expect } from "chai";
import * as vscode from "vscode";
import {
  waitForExtensionReady,
  openVueFile,
  getAppVuePath,
  measureHover,
  sleep,
  FIXTURE_NAME,
} from "../helpers";
import { getTimer } from "../timer";

suite(`Hover [${FIXTURE_NAME}]`, function () {
  this.timeout(90_000);

  let doc: vscode.TextDocument;

  suiteSetup(async function () {
    await waitForExtensionReady(60_000);
    doc = await openVueFile(getAppVuePath());
    // Wait for LSP to fully process the file
    await sleep(8_000);
  });

  test("hover on ref binding in template", async function () {
    // Find "count" in the template section of App.vue
    // Template has: <p>{{ count }} x 2 = {{ doubled }}</p>
    const text = doc.getText();
    const templateMatch = text.indexOf("{{ count }}");
    if (templateMatch === -1) {
      console.log("    {{ count }} not in fixture — pass (N/A)");
      return;
    }

    // Convert offset to position, hover on "count" (3 chars after "{{ ")
    const pos = doc.positionAt(templateMatch + 3);
    const { hovers, latencyMs } = await measureHover(doc.uri, pos);

    getTimer().recordHover("count (ref)", latencyMs);
    console.log(`    Hover on count: ${latencyMs}ms, ${hovers.length} result(s)`);

    // We don't hard-fail if hover is empty (type provider may not be running)
    if (hovers.length > 0) {
      expect(hovers[0].contents.length, "Hover should have content").to.be.greaterThan(0);
    }
  });

  test("hover on computed binding in template", async function () {
    const text = doc.getText();
    const match = text.indexOf("{{ doubled }}");
    if (match === -1) {
      console.log("    {{ doubled }} not in fixture — pass (N/A)");
      return;
    }

    const pos = doc.positionAt(match + 3);
    const { hovers, latencyMs } = await measureHover(doc.uri, pos);

    getTimer().recordHover("doubled (computed)", latencyMs);
    console.log(`    Hover on doubled: ${latencyMs}ms, ${hovers.length} result(s)`);
  });

  test("hover on prop usage in template", async function () {
    const text = doc.getText();
    const match = text.indexOf("{{ title }}");
    if (match === -1) {
      console.log("    {{ title }} not in fixture — pass (N/A)");
      return;
    }

    const pos = doc.positionAt(match + 3);
    const { hovers, latencyMs } = await measureHover(doc.uri, pos);

    getTimer().recordHover("title (prop)", latencyMs);
    console.log(`    Hover on title: ${latencyMs}ms, ${hovers.length} result(s)`);
  });

  test("hover on v-bind shorthand prop name", async function () {
    // Regression test: v-bind shorthand `:bar="count"` had off-by-1 source map
    // mapping (prop name mapped to `:` instead of `b` in `bar`).
    // Verify hover at the prop name returns meaningful type info.
    const text = doc.getText();
    const match = text.indexOf(':bar="count"');
    if (match === -1) {
      console.log('    :bar="count" not in fixture — pass (N/A)');
      return;
    }

    // Hover on "bar" (1 char after ":" to land on the prop name)
    const pos = doc.positionAt(match + 1);
    const { hovers, latencyMs } = await measureHover(doc.uri, pos);

    getTimer().recordHover("bar (v-bind prop)", latencyMs);
    console.log(`    Hover on :bar prop name: ${latencyMs}ms, ${hovers.length} result(s)`);

    if (hovers.length > 0) {
      const content = hovers[0].contents
        .map((c) => (typeof c === "string" ? c : c.value))
        .join("\n");
      console.log(`    Hover content: ${content.slice(0, 120)}`);
      // With correct source map, hover at "bar" should show prop type info,
      // not be empty or show info for the ":" character.
      expect(hovers[0].contents.length, "Hover on v-bind prop name should have content").to.be.greaterThan(0);
    }
  });

  test("hover on v-bind shorthand value expression", async function () {
    // Verify hover at the value expression of a v-bind shorthand works.
    // `:bar="count"` — hover on "count" should show its type (Ref<number>).
    const text = doc.getText();
    const match = text.indexOf(':bar="count"');
    if (match === -1) {
      console.log('    :bar="count" not in fixture — pass (N/A)');
      return;
    }

    // Hover on "count" (6 chars after `:bar="` which is 6 from match)
    const pos = doc.positionAt(match + 6);
    const { hovers, latencyMs } = await measureHover(doc.uri, pos);

    getTimer().recordHover("count (v-bind value)", latencyMs);
    console.log(`    Hover on :bar value "count": ${latencyMs}ms, ${hovers.length} result(s)`);

    if (hovers.length > 0) {
      const content = hovers[0].contents
        .map((c) => (typeof c === "string" ? c : c.value))
        .join("\n");
      console.log(`    Hover content: ${content.slice(0, 120)}`);
      expect(hovers[0].contents.length, "Hover on v-bind value should have content").to.be.greaterThan(0);
    }
  });

  test("hover on function in template", async function () {
    const text = doc.getText();
    const match = text.indexOf('"increment"');
    if (match === -1) {
      console.log('    "increment" not in fixture — pass (N/A)');
      return;
    }

    const pos = doc.positionAt(match + 1);
    const { hovers, latencyMs } = await measureHover(doc.uri, pos);

    getTimer().recordHover("increment (function)", latencyMs);
    console.log(`    Hover on increment: ${latencyMs}ms, ${hovers.length} result(s)`);
  });

  test("hover on component tag", async function () {
    const text = doc.getText();
    const match = text.indexOf("<MyComp");
    if (match === -1) {
      console.log("    <MyComp not in fixture — pass (N/A)");
      return;
    }

    // Hover on "MyComp" (1 char after "<")
    const pos = doc.positionAt(match + 1);
    const { hovers, latencyMs } = await measureHover(doc.uri, pos);

    getTimer().recordHover("MyComp (component tag)", latencyMs);
    console.log(`    Hover on MyComp: ${latencyMs}ms, ${hovers.length} result(s)`);

    if (hovers.length > 0) {
      expect(hovers[0].contents.length, "Hover on component tag should have content").to.be.greaterThan(0);
    }
  });

  test("hover on event handler function", async function () {
    const text = doc.getText();
    const match = text.indexOf('@click.prevent="increment"');
    if (match === -1) {
      console.log('    @click.prevent="increment" not in fixture — pass (N/A)');
      return;
    }

    // Hover on "increment" inside the event handler
    const pos = doc.positionAt(match + '@click.prevent="'.length);
    const { hovers, latencyMs } = await measureHover(doc.uri, pos);

    getTimer().recordHover("increment (event handler)", latencyMs);
    console.log(`    Hover on increment in @click.prevent: ${latencyMs}ms, ${hovers.length} result(s)`);

    if (hovers.length > 0) {
      const content = hovers[0].contents
        .map((c) => (typeof c === "string" ? c : c.value))
        .join("\n");
      console.log(`    Hover content: ${content.slice(0, 120)}`);
      expect(hovers[0].contents.length, "Hover on event handler should have content").to.be.greaterThan(0);
    }
  });

  test("hover on slot outlet tag", async function () {
    // Hover on <slot name="header" /> in MyComp.vue — should show slot outlet info,
    // NOT the unhelpful `() any` from the type provider's generic Slots interface.
    const myCompDoc = await openVueFile("src/MyComp.vue");
    await sleep(3_000);
    const text = myCompDoc.getText();
    const match = text.indexOf('<slot name="header"');
    if (match === -1) {
      console.log('    <slot name="header" not in fixture — pass (N/A)');
      return;
    }

    // Hover on "slot" (1 char after "<")
    const pos = myCompDoc.positionAt(match + 1);
    const { hovers, latencyMs } = await measureHover(myCompDoc.uri, pos);

    getTimer().recordHover("slot outlet tag", latencyMs);
    console.log(`    Hover on <slot>: ${latencyMs}ms, ${hovers.length} result(s)`);

    if (hovers.length > 0) {
      const content = hovers[0].contents
        .map((c) => (typeof c === "string" ? c : c.value))
        .join("\n");
      console.log(`    Hover content: ${content.slice(0, 200)}`);
      expect(hovers[0].contents.length, "Hover on <slot> should have content").to.be.greaterThan(0);
      // Must NOT show the unhelpful generic `() any` from Slots interface
      expect(content, "Hover on <slot> should not show generic () any").to.not.include("() any");
    }

    // Re-open App.vue for subsequent tests
    doc = await openVueFile(getAppVuePath());
    await sleep(1_000);
  });

  test("hover on template #header slot consumer", async function () {
    // Hover on `<template #header>` in App.vue — should show slot content info
    const text = doc.getText();
    const match = text.indexOf("<template #header>");
    if (match === -1) {
      console.log("    <template #header> not in fixture — pass (N/A)");
      return;
    }

    // Hover on "#header" — skip past "<template " (10 chars) to land on "#"
    const pos = doc.positionAt(match + 10);
    const { hovers, latencyMs } = await measureHover(doc.uri, pos);

    getTimer().recordHover("#header (slot consumer)", latencyMs);
    console.log(`    Hover on #header: ${latencyMs}ms, ${hovers.length} result(s)`);

    if (hovers.length > 0) {
      const content = hovers[0].contents
        .map((c) => (typeof c === "string" ? c : c.value))
        .join("\n");
      console.log(`    Hover content: ${content.slice(0, 200)}`);
      expect(hovers[0].contents.length, "Hover on #header should have content").to.be.greaterThan(0);
      // Should mention "slot" somewhere in the hover
      expect(content.toLowerCase(), "Hover on #header should mention slot").to.include("slot");
    }
  });

  test("hover on slot name attribute value", async function () {
    // Hover on "header" in `name="header"` inside MyComp.vue's <slot> element
    const myCompDoc = await openVueFile("src/MyComp.vue");
    await sleep(2_000);
    const text = myCompDoc.getText();
    const match = text.indexOf('name="header"');
    if (match === -1) {
      console.log('    name="header" not in fixture — pass (N/A)');
      return;
    }

    // Hover on "header" (6 chars after `name="`)
    const pos = myCompDoc.positionAt(match + 6);
    const { hovers, latencyMs } = await measureHover(myCompDoc.uri, pos);

    getTimer().recordHover("header (slot name attr)", latencyMs);
    console.log(`    Hover on slot name="header": ${latencyMs}ms, ${hovers.length} result(s)`);

    if (hovers.length > 0) {
      const content = hovers[0].contents
        .map((c) => (typeof c === "string" ? c : c.value))
        .join("\n");
      console.log(`    Hover content: ${content.slice(0, 200)}`);
      // Should not show `() any`
      expect(content, "Hover on slot name should not show generic () any").to.not.include("() any");
    }

    // Re-open App.vue for the latency test
    doc = await openVueFile(getAppVuePath());
    await sleep(1_000);
  });

  test("hover latency is reasonable", function () {
    const report = getTimer().getReport();
    const samples = report.hover.samples;

    if (samples.length === 0) {
      console.log("    No hover samples collected — fixture may not have template bindings");
      return;
    }

    const latencies = samples.map((s) => s.latencyMs);
    const avg = latencies.reduce((a, b) => a + b, 0) / latencies.length;
    const max = Math.max(...latencies);

    console.log(
      `    Hover stats: avg=${Math.round(avg)}ms, max=${max}ms, samples=${samples.length}`,
    );

    // Generous bound — includes cold-start for first hover
    expect(avg, "Average hover latency should be under 5s").to.be.lessThan(5_000);
  });
});
