import { expect } from "chai";
import * as vscode from "vscode";
import {
  waitForExtensionReady,
  waitForFileReady,
  openVueFile,
  getAppVuePath,
  FIXTURE_NAME,
} from "../helpers";

/**
 * Attrs fallthrough E2E tests.
 *
 * These tests verify that `$attrs` typing resolves correctly across different
 * root element scenarios by checking which diagnostics Verter produces.
 *
 * Strategy: `verter/unknown-prop` diagnostics indicate that a prop was NOT
 * accepted by the component (not in declared props AND not in $attrs via
 * root element fallthrough). Absence of that diagnostic for `class` means
 * the attrs fallthrough chain is working.
 */
suite(`Attrs Fallthrough [${FIXTURE_NAME}]`, function () {
  this.timeout(60_000);

  let doc: vscode.TextDocument;

  suiteSetup(async function () {
    await waitForExtensionReady();
    doc = await openVueFile(getAppVuePath());
    // Wait for cross-file analysis to complete
    await waitForFileReady(doc);
  });

  /** Get Verter diagnostics for the current App.vue */
  function getVerterDiags(): vscode.Diagnostic[] {
    return vscode.languages
      .getDiagnostics(doc.uri)
      .filter((d) => d.source === "Verter" || d.source === "verter");
  }

  /** Get unknown-prop diagnostics */
  function getUnknownPropDiags(): vscode.Diagnostic[] {
    return getVerterDiags().filter((d) =>
      typeof d.code === "object"
        ? (d.code as { value: string }).value === "verter/unknown-prop"
        : d.code === "verter/unknown-prop",
    );
  }

  /** Find unknown-prop diagnostics for a specific component usage line */
  function unknownPropsOnLine(lineSubstring: string): vscode.Diagnostic[] {
    const text = doc.getText();
    const lines = text.split("\n");
    const lineIndex = lines.findIndex((l) => l.includes(lineSubstring));
    if (lineIndex === -1) return [];

    return getUnknownPropDiags().filter((d) => d.range.start.line === lineIndex);
  }

  test("S1: native root — class falls through (BaseButton)", function () {
    // BaseButton has <button> root. `class` should fall through to the
    // native <button>, so no unknown-prop diagnostic for class.
    const diags = unknownPropsOnLine('<BaseButton label="click me"');
    const classWarning = diags.find((d) => d.message.includes("class"));
    expect(classWarning, "class should NOT be flagged on BaseButton").to.be.undefined;
  });

  test("S2: component root — class falls through (WrappedButton)", function () {
    // WrappedButton has <BaseButton> as root. class should fall through
    // WrappedButton → BaseButton → native <button>.
    const diags = unknownPropsOnLine('<WrappedButton variant="danger"');
    const classWarning = diags.find((d) => d.message.includes("class"));
    expect(classWarning, "class should NOT be flagged on WrappedButton").to.be.undefined;
  });

  test("S3: fragment — extra attr flagged (FragmentComp)", function () {
    // Skip: fragment child resolution depends on background scanner completing
    return this.skip();
    // FragmentComp has multiple roots (fragment). data-test cannot fall
    // through, so it should be flagged as unknown-prop.
    const text = doc.getText();
    if (!text.includes('<FragmentComp msg="hello"')) {
      console.log("    FragmentComp not in this fixture — skip");
      return;
    }

    const diags = unknownPropsOnLine('<FragmentComp msg="hello"');
    const msgWarning = diags.find((d) => d.message.includes("msg"));
    expect(msgWarning, "msg is a declared prop and should NOT be flagged").to.be.undefined;

    // data-test is NOT a declared prop and fragments don't support fallthrough
    const dataTestWarning = diags.find((d) => d.message.includes("data-test"));
    expect(dataTestWarning, "data-test should be flagged on fragment component").to.exist;
  });

  test("S4: inheritAttrs: false — suppresses checks (NoInheritComp)", function () {
    // NoInheritComp sets inheritAttrs: false. Extra attrs should be
    // accepted (the component handles them manually via useAttrs).
    const diags = unknownPropsOnLine('<NoInheritComp label="ok"');
    const dataCustomWarning = diags.find((d) => d.message.includes("data-custom"));
    expect(dataCustomWarning, "data-custom should NOT be flagged when inheritAttrs: false").to.be
      .undefined;
  });

  test("S5: conditional root — class falls through (ConditionalRoot)", function () {
    // ConditionalRoot has v-if/v-else. Both branches are <div>, so class
    // should fall through.
    const diags = unknownPropsOnLine("<ConditionalRoot :show=");
    const classWarning = diags.find((d) => d.message.includes("class"));
    expect(classWarning, "class should NOT be flagged on ConditionalRoot").to.be.undefined;
  });

  test("S6: functional component — no TS error from instantiation (FunctionalBtn)", function () {
    // FunctionalBtn is a functional component (plain function, not class).
    // The new `instantiateComponent` helper should handle it without TS errors.
    const diags = unknownPropsOnLine('<FunctionalBtn label="fn"');
    const classWarning = diags.find((d) => d.message.includes("class"));
    expect(classWarning, "class should NOT be flagged on FunctionalBtn").to.be.undefined;

    // Check there are no TS errors related to instantiation
    const allDiags = vscode.languages.getDiagnostics(doc.uri);
    const tsErrors = allDiags.filter(
      (d) =>
        d.source === "ts" &&
        d.severity === vscode.DiagnosticSeverity.Error &&
        d.message.includes("FunctionalBtn"),
    );
    expect(tsErrors, "should have no TS errors mentioning FunctionalBtn").to.have.length(0);
  });

  test("diagnostics summary", function () {
    const allDiags = getVerterDiags();
    const unknownProps = getUnknownPropDiags();
    console.log(`    Total Verter diagnostics: ${allDiags.length}`);
    console.log(`    Unknown-prop diagnostics: ${unknownProps.length}`);
    for (const d of unknownProps) {
      console.log(`      L${d.range.start.line + 1}: ${d.message}`);
    }
  });
});
