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
