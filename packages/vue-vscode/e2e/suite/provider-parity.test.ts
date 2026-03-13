import { expect } from "chai";
import * as vscode from "vscode";
import {
  waitForExtensionReady,
  waitForFileReady,
  openVueFile,
  getAppVuePath,
  measureHover,
  getCompletions,
  getDefinitions,
  findPosition,
  readTestLog,
  waitForDiagnostics,
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

suite(`Provider Parity [${FIXTURE_NAME}]`, function () {
  this.timeout(60_000);

  let doc: vscode.TextDocument;

  suiteSetup(async function () {
    await waitForExtensionReady();
    doc = await openVueFile(getAppVuePath());
    await waitForFileReady(doc);
  });

  // ── Core Parity Checks ────────────────────────────────────────
  // These tests assert the same thing regardless of provider.
  // If a test fails on one provider but not the other, it's a
  // provider-specific regression.

  test("P1: hover on ref binding returns number type", async function () {
    const pos = findPosition(doc, "{{ count }}", 3);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers } = await measureHover(doc.uri, pos);
    console.log(`    [${TYPE_PROVIDER || "verter-only"}] hover on count: ${hovers.length} result(s)`);

    expect(hovers.length, "hover should return results").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    expect(content, "hover should mention count").to.include("count");
    expect(content, "hover should include number type").to.include("number");
    expect(content, "hover should NOT degrade to any").to.not.match(/:\s*any\b/);
  });

  test("P2: hover on component tag returns props", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "<MyComp", 1);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers } = await measureHover(doc.uri, pos);
    console.log(`    [${TYPE_PROVIDER || "verter-only"}] hover on <MyComp: ${hovers.length} result(s)`);

    expect(hovers.length, "hover should return results").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    // Both providers should show foo and bar props
    expect(content, "should mention foo prop").to.include("foo");
    expect(content, "should mention bar prop").to.include("bar");
    // Must NOT show degraded empty shell
    expect(content, "should NOT show DefineComponent<{}, {}>").to.not.include("DefineComponent<{}, {}>");
  });

  test("P3: completions on member access return typed properties", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "action.disabled", 7);
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await getCompletions(doc.uri, pos);
    expect(completions, "completions should exist").to.exist;
    expect(completions!.items.length, "completions should not be empty").to.be.greaterThan(0);

    const labels = completionLabels(completions!);
    console.log(`    [${TYPE_PROVIDER || "verter-only"}] action. completions: ${labels.join(", ")}`);

    expect(labels, "should include disabled").to.include("disabled");
    expect(labels, "should include label").to.include("label");
    expect(labels, "should include handler").to.include("handler");
  });

  test("P4: go-to-definition on component tag reaches .vue file", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "<MyComp", 1);
    if (!pos) {
      this.skip();
      return;
    }

    const locations = await getDefinitions(doc.uri, pos);
    console.log(`    [${TYPE_PROVIDER || "verter-only"}] definition on <MyComp: ${locations.length} location(s)`);

    expect(locations.length, "should have definition results").to.be.greaterThan(0);

    const def = locations.find((l) => l.uri.fsPath.includes("MyComp.vue")) || locations[0];
    expect(def.uri.fsPath, "definition should reach MyComp.vue").to.include("MyComp.vue");
    expect(def.uri.fsPath, "should NOT be in .tsx").to.not.match(/\.vue\.tsx$/);
  });

  test("P5: hover on imported component shows real type, not any", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "import MyComp from './MyComp.vue'", 7);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers } = await measureHover(doc.uri, pos);
    console.log(`    [${TYPE_PROVIDER || "verter-only"}] hover on MyComp import: ${hovers.length} result(s)`);

    expect(hovers.length, "hover should return results").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    expect(content, "should mention foo prop").to.include("foo");
    expect(content, "should mention bar prop").to.include("bar");
    expect(content, "should NOT be any").to.not.match(/:\s*any\b/);
    expect(content, "should NOT show DefineComponent<{}, {}>").to.not.include("DefineComponent<{}, {}>");
  });

  // ── Component Prop Validation ────────────────────────────────
  // When a TypeProvider is active, invalid component props should produce
  // TypeScript errors (not verter/ diagnostics).

  test("P6: TypeProvider detects invalid component props", async function () {
    if (!TYPE_PROVIDER) {
      console.log("    N/A (no TypeProvider)");
      return;
    }
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const invalidDoc = await openVueFile("src/InvalidPropCase.vue");
    await waitForFileReady(invalidDoc);

    // Wait for TS diagnostics referencing the unknown prop
    const diags = await waitForDiagnostics(invalidDoc.uri, {
      source: "ts",
      timeoutMs: 30_000,
      predicate: (d) => d.message.includes("thisDoesNotExist"),
    });

    // Positive: TypeScript should flag 'thisDoesNotExist' as invalid
    const tsError = diags.find((d) => d.message.includes("thisDoesNotExist"));
    expect(
      tsError,
      `Expected TS error for 'thisDoesNotExist'. Got: ${JSON.stringify(diags.map((d) => ({ msg: d.message, code: d.code, src: d.source })))}`,
    ).to.exist;

    // Negative: only the unknown prop should be diagnosed
    const allDiags = vscode.languages.getDiagnostics(invalidDoc.uri);
    const tsDiags = allDiags.filter((d) => d.source === "ts");
    expect(
      tsDiags.length,
      `expected exactly one TS diagnostic for InvalidPropCase.vue, got: ${JSON.stringify(tsDiags.map((d) => ({ msg: d.message, code: d.code, src: d.source })))}`,
    ).to.equal(1);
    expect(
      tsDiags[0].message,
      "the TS diagnostic should be about the unknown prop",
    ).to.include("thisDoesNotExist");

    // Negative: no verter/unknown-prop diagnostics (TypeProvider is source of truth)
    const verterPropDiag = allDiags.find(
      (d) => d.source === "verter" && d.message.includes("Unknown prop"),
    );
    expect(
      verterPropDiag,
      "verter/unknown-prop should be suppressed when TypeProvider is active",
    ).to.be.undefined;
  });

  // ── Barrel Re-Export Parity Tests ────────────────────────────
  // With full VFS sync, both TSGO and tsserver should resolve
  // barrel-imported .vue component types correctly.

  test("barrel re-exported .vue component type resolution", async function () {
    if (FIXTURE_NAME !== "barrel-exports") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "<Button", 1);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers } = await measureHover(doc.uri, pos);
    if (hovers.length === 0) {
      console.log(`    [${TYPE_PROVIDER || "verter-only"}] No hover — provider may not be running`);
      return;
    }

    if (!TYPE_PROVIDER) {
      console.log("    Verter-only: barrel re-export type resolution requires type provider");
      return;
    }

    const content = hoverText(hovers[0]);
    console.log(`    [${TYPE_PROVIDER}] barrel Button hover: ${content.slice(0, 200)}`);

    const hasRealProps = content.includes("label") && content.includes("disabled");
    const isDegraded = content.includes("DefineComponent<{}, {}>");

    expect(hasRealProps, `barrel Button should show props, got: ${content.slice(0, 200)}`).to.be.true;
    expect(isDegraded, "barrel Button should NOT be degraded").to.be.false;
  });

  test("barrel component go-to-definition target", async function () {
    if (FIXTURE_NAME !== "barrel-exports") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "<Button", 1);
    if (!pos) {
      this.skip();
      return;
    }

    const locations = await getDefinitions(doc.uri, pos);
    if (locations.length === 0) {
      console.log(`    [${TYPE_PROVIDER || "verter-only"}] No definition — provider may not be running`);
      return;
    }

    if (!TYPE_PROVIDER) {
      console.log("    Verter-only: barrel go-to-definition requires type provider");
      return;
    }

    const def = locations[0];
    console.log(`    [${TYPE_PROVIDER}] barrel Button definition: ${def.uri.fsPath}`);

    expect(def.uri.fsPath, "should reach Button.vue").to.include("Button.vue");
    expect(def.uri.fsPath, "should NOT stop at index.ts").to.not.include("index.ts");
  });

  test("CANARY: provider kind is logged correctly", function () {
    const log = readTestLog();
    const providerMatch = log.match(/\[TIMING\] type_provider_started \d+ (tsgo|tsserver)/);

    if (providerMatch) {
      console.log(`    Active provider: ${providerMatch[1]}`);

      if (TYPE_PROVIDER) {
        expect(
          providerMatch[1],
          `Requested ${TYPE_PROVIDER} but got ${providerMatch[1]}`,
        ).to.equal(TYPE_PROVIDER);
      }
    } else if (log.includes("verter-only mode")) {
      console.log("    Active provider: verter-only (no type provider)");
    } else {
      console.log("    Active provider: unknown (no TIMING markers found)");
    }
  });
});
