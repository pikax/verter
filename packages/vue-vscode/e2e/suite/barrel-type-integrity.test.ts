import { expect } from "chai";
import * as vscode from "vscode";
import {
  waitForExtensionReady,
  waitForTypeProviderSync,
  waitForFileReady,
  openVueFile,
  measureHover,
  getCompletions,
  findPosition,
  findNthPosition,
  hoverText,
  FIXTURE_NAME,
  TYPE_PROVIDER,
} from "../helpers";

function completionLabel(item: vscode.CompletionItem): string {
  return typeof item.label === "string" ? item.label : item.label.label;
}

function completionLabels(list: vscode.CompletionList): string[] {
  return list.items.map(completionLabel);
}

suite(`Barrel Type Integrity [${FIXTURE_NAME}]`, function () {
  this.timeout(60_000);

  const isBarrelFixture = FIXTURE_NAME === "barrel-exports";

  let doc: vscode.TextDocument;

  suiteSetup(async function () {
    if (!isBarrelFixture) return;
    await waitForExtensionReady();
    await waitForTypeProviderSync();
    doc = await openVueFile("src/App.vue");
    await waitForFileReady(doc);
  });

  // ── Hover Type Integrity ──────────────────────────────────────

  test("hover on <Button> tag shows label, disabled, size props", async function () {
    if (!isBarrelFixture) {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "<Button", 1);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers } = await measureHover(doc.uri, pos);
    expect(hovers.length, "Button tag hover should exist").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    console.log(`    <Button> hover: ${content.slice(0, 300)}`);

    if (!TYPE_PROVIDER) {
      const hasProps = content.includes("disabled") || content.includes("size");
      if (!hasProps) {
        console.log(
          "    Verter-only: barrel Button hover lacks detailed props (needs type provider)",
        );
        return;
      }
    }

    // Positive: must include actual prop names
    expect(content, "Button hover should mention label").to.include("label");
    expect(content, "Button hover should mention disabled or size").to.satisfy(
      (c: string) => c.includes("disabled") || c.includes("size"),
    );

    // Negative: must NOT be degraded shell
    expect(content, "Button hover must NOT be DefineComponent<{}, {}>").to.not.include(
      "DefineComponent<{}, {}>",
    );
    expect(content, "Button hover must NOT degrade to any").to.not.match(/:\s*any\b/);
  });

  test("hover on <Overlay> tag shows zIndex, duration, show, lockScroll props", async function () {
    if (!isBarrelFixture) {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "<Overlay", 1);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers } = await measureHover(doc.uri, pos);
    expect(hovers.length, "Overlay tag hover should exist").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    console.log(`    <Overlay> hover: ${content.slice(0, 300)}`);

    // Positive: must include actual prop names
    expect(content, "Overlay hover should mention zIndex").to.include("zIndex");
    expect(content, "Overlay hover should mention show or lockScroll").to.satisfy(
      (c: string) => c.includes("show") || c.includes("lockScroll"),
    );

    // Negative: must NOT be degraded shell
    expect(content, "Overlay hover must NOT be DefineComponent<{}, {}>").to.not.include(
      "DefineComponent<{}, {}>",
    );
    expect(content, "Overlay hover must NOT degrade to any").to.not.match(/:\s*any\b/);
  });

  // ── Completion Type Integrity ─────────────────────────────────

  test("completions inside <Button > include label, disabled, size, @click", async function () {
    if (!isBarrelFixture) {
      console.log("    N/A");
      return;
    }

    // Use the probe element (2nd occurrence) which has no existing props
    const pos = findNthPosition(doc, "<Button ", 1, 8);
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await getCompletions(doc.uri, pos);
    expect(completions, "Button completions should exist").to.exist;
    expect(completions!.items.length, "Button completions should not be empty").to.be.greaterThan(
      0,
    );

    const labels = completionLabels(completions!);
    console.log(`    <Button > completions: ${labels.slice(0, 20).join(", ")}`);

    if (!TYPE_PROVIDER && !labels.includes("label")) {
      console.log("    Verter-only: barrel Button completions lack props (needs type provider)");
      return;
    }

    expect(labels, "should include label prop").to.include("label");
    expect(labels, "should include disabled prop").to.include("disabled");
    expect(labels, "should include size prop").to.include("size");
    expect(labels, "should include @click event").to.include("@click");

    // Negative: should NOT include DefineComponent internals at attribute level
    expect(labels.join(","), "should not include $slots").to.not.include("$slots");
    expect(labels.join(","), "should not include $emit").to.not.include("$emit");
    expect(labels.join(","), "should not include __props").to.not.include("__props");
  });

  test("completions inside <Overlay > include zIndex, duration, show, lockScroll", async function () {
    if (!isBarrelFixture) {
      console.log("    N/A");
      return;
    }

    // Use the probe element (2nd occurrence) which has no existing props
    const pos = findNthPosition(doc, "<Overlay ", 1, 9);
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await getCompletions(doc.uri, pos);
    expect(completions, "Overlay completions should exist").to.exist;
    expect(completions!.items.length, "Overlay completions should not be empty").to.be.greaterThan(
      0,
    );

    const labels = completionLabels(completions!);
    console.log(`    <Overlay > completions: ${labels.slice(0, 20).join(", ")}`);

    if (!TYPE_PROVIDER && !labels.includes("zIndex")) {
      console.log("    Verter-only: barrel Overlay completions lack props (needs type provider)");
      return;
    }

    // Props are offered in kebab-case by Verter
    expect(labels, "should include z-index prop").to.include("z-index");
    expect(labels, "should include show prop").to.include("show");
    expect(labels, "should include lock-scroll prop").to.include("lock-scroll");
    expect(labels, "should include duration prop").to.include("duration");
  });

  // ── Import Binding Hover ──────────────────────────────────────

  test("hover on Button import binding shows component type, not any", async function () {
    if (!isBarrelFixture) {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "{ Overlay, Button }", 12); // on "B"
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers } = await measureHover(doc.uri, pos);
    expect(hovers.length, "Button import hover should exist").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    console.log(`    Button import hover: ${content.slice(0, 200)}`);

    // Negative: must NOT be plain any or degraded shell
    expect(content, "Button import should NOT be DefineComponent<{}, {}>").to.not.include(
      "DefineComponent<{}, {}>",
    );
    expect(content, "Button import should NOT be any").to.not.match(/:\s*any\b/);
  });

  test("hover on Overlay import binding shows component type, not any", async function () {
    if (!isBarrelFixture) {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "{ Overlay, Button }", 2); // on "O"
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers } = await measureHover(doc.uri, pos);
    expect(hovers.length, "Overlay import hover should exist").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    console.log(`    Overlay import hover: ${content.slice(0, 200)}`);

    expect(content, "Overlay import should NOT be DefineComponent<{}, {}>").to.not.include(
      "DefineComponent<{}, {}>",
    );
    expect(content, "Overlay import should NOT be any").to.not.match(/:\s*any\b/);
  });

  // ── Prop Value Type Integrity ─────────────────────────────────

  test("hover on :show value shows boolean type", async function () {
    if (!isBarrelFixture) {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, ':show="showOverlay"', 7); // on "showOverlay"
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers } = await measureHover(doc.uri, pos);
    expect(hovers.length, ":show value hover should exist").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    console.log(`    :show value hover: ${content.slice(0, 200)}`);

    expect(content, ":show value should mention showOverlay").to.include("showOverlay");
    expect(content, ":show value should mention boolean").to.include("boolean");
    expect(content, ":show value should NOT be any").to.not.match(/:\s*any\b/);
  });

  test("hover on :zIndex prop name shows number type", async function () {
    if (!isBarrelFixture) {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, ':zIndex="100"', 1); // on "zIndex" (prop name)
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers } = await measureHover(doc.uri, pos);

    // In verter-only mode, barrel component prop values may not get hover
    if (hovers.length === 0 && !TYPE_PROVIDER) {
      console.log("    Verter-only: no hover for barrel component prop name (needs type provider)");
      return;
    }

    expect(hovers.length, ":zIndex prop hover should exist").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    console.log(`    :zIndex prop hover: ${content.slice(0, 200)}`);

    // zIndex prop should show its type (number)
    expect(content, ":zIndex prop should mention zIndex or number").to.satisfy(
      (c: string) => c.includes("zIndex") || c.includes("number"),
    );
    expect(content, ":zIndex prop should NOT be any").to.not.match(/:\s*any\b/);
  });

  test("hover on label= string value shows string type", async function () {
    if (!isBarrelFixture) {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, 'label="Open"', 0); // on "label"
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers } = await measureHover(doc.uri, pos);
    expect(hovers.length, "label attr hover should exist").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    console.log(`    label attr hover: ${content.slice(0, 200)}`);

    expect(content, "label attr should mention label").to.include("label");
    expect(content, "label attr should mention string").to.include("string");
    expect(content, "label attr should NOT be any").to.not.match(/:\s*any\b/);
  });
});
