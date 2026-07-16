/**
 * Vue deep fallthrough / root inheritance.
 *
 * Verter owns multi-hop fallthrough (component root → component root → native).
 * These cases assert that undeclared attrs on outer components are accepted when
 * a native root eventually inherits them, and rejected for fragment roots.
 */
import * as vscode from "vscode";
import { FIXTURE_NAME, waitForDiagnosticsSettled } from "../../../helpers";
import {
  absoluteFile,
  assertCompletionsInclude,
  ensureParityReady,
  openRelative,
  failParityGap,
  verterUnknownPropDiags,
} from "../../../lib/parityHarness";

function onlyVueParity(ctx: Mocha.Context): void {
  if (FIXTURE_NAME !== "vue-parity")
    throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
}

function unknownPropsOnLine(doc: vscode.TextDocument, lineSubstring: string): vscode.Diagnostic[] {
  const lines = doc.getText().split(/\r?\n/);
  const lineIndex = lines.findIndex((line) => line.includes(lineSubstring));
  if (lineIndex === -1) return [];
  return verterUnknownPropDiags(doc.uri).filter((d) => d.range.start.line === lineIndex);
}

function messageMentions(diags: vscode.Diagnostic[], needle: string): boolean {
  return diags.some((d) => d.message.toLowerCase().includes(needle.toLowerCase()));
}

suite(`Vue deep fallthrough [${FIXTURE_NAME}]`, function () {
  let consumer: vscode.TextDocument;

  suiteSetup(async function () {
    this.timeout(60_000);
    onlyVueParity(this);
    await ensureParityReady("src/App.vue");
    consumer = await openRelative("src/fallthrough/DeepConsumer.vue");
    await waitForDiagnosticsSettled(consumer.uri, { timeoutMs: 12_000, stableMs: 700 });
  });

  test("vue.fallthrough.deep-class-accepted", async function () {
    onlyVueParity(this);
    // OuterWrap → MidWrap → LeafNative → <button>: class must not be unknown-prop
    const diags = unknownPropsOnLine(consumer, 'tone="primary"');
    if (messageMentions(diags, "class")) {
      failParityGap(
        this,
        "vue.fallthrough.deep-class-accepted",
        "ISSUE-vue-deep-fallthrough-class",
        `class flagged as unknown on three-hop component chain: ${diags.map((d) => d.message).join("; ")}`,
      );
    }
  });

  test("vue.fallthrough.deep-data-attr-accepted", async function () {
    onlyVueParity(this);
    const diags = unknownPropsOnLine(consumer, 'tone="primary"');
    if (
      messageMentions(diags, "data-testid") ||
      messageMentions(diags, "dataTestid") ||
      messageMentions(diags, "testid")
    ) {
      failParityGap(
        this,
        "vue.fallthrough.deep-data-attr-accepted",
        "ISSUE-vue-deep-fallthrough-data",
        `data-testid flagged on deep chain: ${diags.map((d) => d.message).join("; ")}`,
      );
    }
  });

  test("vue.fallthrough.deep-style-accepted", async function () {
    onlyVueParity(this);
    const diags = unknownPropsOnLine(consumer, 'tone="primary"');
    if (messageMentions(diags, "style")) {
      failParityGap(
        this,
        "vue.fallthrough.deep-style-accepted",
        "ISSUE-vue-deep-fallthrough-style",
        `style flagged on deep chain: ${diags.map((d) => d.message).join("; ")}`,
      );
    }
  });

  test("vue.fallthrough.fragment-extra-flagged", async function () {
    onlyVueParity(this);
    const diags = unknownPropsOnLine(consumer, "FragmentRoot");
    const flagged =
      messageMentions(diags, "data-test") ||
      messageMentions(diags, "dataTest") ||
      diags.some((d) => /unknown/i.test(d.message));
    if (!flagged) {
      failParityGap(
        this,
        "vue.fallthrough.fragment-extra-flagged",
        "ISSUE-vue-fragment-fallthrough",
        "Fragment root did not flag undeclared data-test (or unknown-prop diagnostics unavailable)",
      );
    }
  });

  test("vue.fallthrough.no-inherit-extra-accepted", async function () {
    onlyVueParity(this);
    const diags = unknownPropsOnLine(consumer, "NoInherit");
    if (messageMentions(diags, "data-custom") || messageMentions(diags, "dataCustom")) {
      failParityGap(
        this,
        "vue.fallthrough.no-inherit-extra-accepted",
        "ISSUE-vue-no-inherit-attrs",
        `inheritAttrs:false still flagged data-custom: ${diags.map((d) => d.message).join("; ")}`,
      );
    }
  });

  test("vue.fallthrough.deep-completion-includes-class", async function () {
    onlyVueParity(this);
    try {
      // Completions on OuterWrap attributes should include inherited native attrs (class).
      await assertCompletionsInclude(
        {
          file: "src/fallthrough/DeepConsumer.vue",
          token: "OuterWrap",
          occurrence: 1,
          caretOffset: 9,
        },
        ["class"],
      );
    } catch (err) {
      failParityGap(
        this,
        "vue.fallthrough.deep-completion-includes-class",
        "ISSUE-vue-deep-fallthrough-completion",
        `Deep fallthrough attrs not offered in completion on OuterWrap: ${String(err)}`,
      );
    }
  });

  test("vue.fallthrough.deep-aria-accepted", async function () {
    onlyVueParity(this);
    // Extra coverage beyond required IDs: aria-* through deep chain.
    const diags = unknownPropsOnLine(consumer, 'tone="primary"');
    if (messageMentions(diags, "aria-label") || messageMentions(diags, "ariaLabel")) {
      failParityGap(
        this,
        "vue.fallthrough.deep-aria-accepted",
        "ISSUE-vue-deep-fallthrough-aria",
        `aria-label flagged on deep chain: ${diags.map((d) => d.message).join("; ")}`,
      );
    }
  });

  test("vue.fallthrough.consumer-path-is-authored", function () {
    onlyVueParity(this);
    // Sanity: fixture is the authored DeepConsumer path, not a generated companion.
    const fsPath = absoluteFile("src/fallthrough/DeepConsumer.vue");
    if (!consumer.uri.fsPath.replace(/\\/g, "/").endsWith("DeepConsumer.vue")) {
      throw new Error(
        `expected DeepConsumer.vue open, got ${consumer.uri.fsPath} (want under ${fsPath})`,
      );
    }
  });
});
