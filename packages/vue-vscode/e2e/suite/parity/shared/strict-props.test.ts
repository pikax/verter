/**
 * Strict-first props contract (Vue + Svelte).
 *
 * Vue (Verter product):
 * - Undeclared attrs that are NOT fallthrough → MUST fail (verter/unknown-prop / TS).
 * - Fallthrough-proven attrs (class/style/data-* and aria-* on a native-root chain) → MUST pass.
 * - Fragment roots → non-declared extras MUST fail.
 * Unlike Volar-loose, existence of a random prop name is never "just fine".
 *
 * Svelte (no Vue fallthrough):
 * - Undeclared props MUST fail.
 * - Rest props (`...rest`) are the only opt-in extra surface (not multi-hop inheritance).
 */
import * as vscode from "vscode";
import { FIXTURE_NAME, waitForDiagnosticsSettled } from "../../../helpers";
import {
  assertCleanErrors,
  assertHasErrorMatching,
  ensureParityReady,
  errorDiagnostics,
  openRelative,
  registerFrameworkTest,
  failParityGap,
  verterUnknownPropDiags,
} from "../../../lib/parityHarness";

function parityFramework(): "vue" | "svelte" | null {
  if (FIXTURE_NAME === "vue-parity") return "vue";
  if (FIXTURE_NAME === "svelte-parity") return "svelte";
  return null;
}

const UNKNOWNish =
  /unknown-prop|unknown prop|2322|2353|2551|does not exist|not assignable|Property|totallyFake/i;

function mentions(diags: readonly vscode.Diagnostic[], needle: string): boolean {
  return diags.some((d) => d.message.toLowerCase().includes(needle.toLowerCase()));
}

suite(`Strict props + fallthrough contract [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    const fw = parityFramework();
    if (!fw) {
      throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    }
    this.timeout(20_000);
    await ensureParityReady(fw === "vue" ? "src/App.vue" : "src/App.svelte");
  });

  test("strict.unknown-prop.must-fail", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file =
      fw === "vue" ? "src/strict/StrictUnknownProp.vue" : "src/strict/StrictUnknownProp.svelte";
    try {
      await assertHasErrorMatching(file, UNKNOWNish);
      // Prefer Verter unknown-prop when present (Vue path)
      if (fw === "vue") {
        const doc = await openRelative(file);
        await waitForDiagnosticsSettled(doc.uri, { timeoutMs: 8_000, stableMs: 400 });
        const verter = verterUnknownPropDiags(doc.uri);
        const fake =
          mentions(verter, "totallyFake") ||
          mentions(verter, "totallyfake") ||
          verter.some((d) => /unknown/i.test(d.message));
        const all = await errorDiagnostics(file);
        if (!fake && !all.some((d) => UNKNOWNish.test(`${d.code}:${d.message}`))) {
          throw new Error(
            `expected totallyFakeProp unknown-prop or type error; verter=${verter
              .map((d) => d.message)
              .join("; ")}; all=${all.map((d) => d.message).join("; ")}`,
          );
        }
      }
    } catch (err) {
      failParityGap(
        this,
        "strict.unknown-prop.must-fail",
        fw === "vue" ? "ISSUE-vue-strict-unknown-prop" : "ISSUE-svelte-strict-unknown-prop",
        `Strict-first: undeclared prop must fail (Volar-loose accept is wrong for Verter): ${String(err)}`,
        "product-gap",
      );
    }
  });

  registerFrameworkTest("vue", "strict.vue.fallthrough-attrs-accepted", async function () {
    try {
      // Fallthrough-proven native attrs must NOT be unknown-prop
      await assertCleanErrors("src/strict/StrictFallthroughOk.vue");
      const doc = await openRelative("src/strict/StrictFallthroughOk.vue");
      await waitForDiagnosticsSettled(doc.uri, { timeoutMs: 8_000, stableMs: 400 });
      const verter = verterUnknownPropDiags(doc.uri);
      for (const needle of ["class", "style", "data-testid", "aria-label", "ariaLabel"]) {
        if (mentions(verter, needle)) {
          throw new Error(
            `fallthrough attr incorrectly flagged as unknown (${needle}): ${verter
              .map((d) => d.message)
              .join("; ")}`,
          );
        }
      }
    } catch (err) {
      failParityGap(
        this,
        "strict.vue.fallthrough-attrs-accepted",
        "ISSUE-vue-strict-fallthrough-accept",
        `Fallthrough attrs must be accepted (proven native root), not rejected: ${String(err)}`,
        "product-gap",
      );
    }
  });

  registerFrameworkTest("vue", "strict.vue.fragment-unknown-flagged", async function () {
    try {
      // Existing deep-consumer fragment line: data-test on FragmentRoot
      const doc = await openRelative("src/fallthrough/DeepConsumer.vue");
      await waitForDiagnosticsSettled(doc.uri, { timeoutMs: 10_000, stableMs: 500 });
      const lines = doc.getText().split(/\r?\n/);
      const lineIndex = lines.findIndex((l) => l.includes("FragmentRoot"));
      if (lineIndex < 0) throw new Error("TEST_DEFECT: FragmentRoot usage missing");
      const onLine = verterUnknownPropDiags(doc.uri).filter(
        (d) => d.range.start.line === lineIndex,
      );
      const flagged =
        mentions(onLine, "data-test") ||
        mentions(onLine, "dataTest") ||
        onLine.some((d) => /unknown/i.test(d.message));
      if (!flagged) {
        // TS may report instead of verter/unknown-prop
        const all = vscode.languages
          .getDiagnostics(doc.uri)
          .filter((d) => d.severity === vscode.DiagnosticSeverity.Error);
        const lineAll = all.filter((d) => d.range.start.line === lineIndex);
        if (!lineAll.some((d) => UNKNOWNish.test(`${d.code}:${d.message}`))) {
          throw new Error(
            `fragment extra attr not flagged; verter=${onLine.map((d) => d.message).join("; ")}`,
          );
        }
      }
    } catch (err) {
      failParityGap(
        this,
        "strict.vue.fragment-unknown-flagged",
        "ISSUE-vue-fragment-fallthrough",
        `Fragment root must reject non-declared attrs (no fallthrough): ${String(err)}`,
        "product-gap",
      );
    }
  });

  registerFrameworkTest("svelte", "strict.svelte.rest-props-opt-in", async function () {
    try {
      // Author-declared rest is the Svelte way to accept extras — not Vue fallthrough.
      await assertCleanErrors("src/strict/StrictRestOk.svelte");
    } catch (err) {
      failParityGap(
        this,
        "strict.svelte.rest-props-opt-in",
        "ISSUE-svelte-strict-rest-props",
        `Svelte rest props should accept extras when declared: ${String(err)}`,
        "product-gap",
      );
    }
  });

  registerFrameworkTest("vue", "strict.vue.deep-fallthrough-still-clean", async function () {
    // Reinforce differentiator: deep OuterWrap chain stays free of unknown-prop on class/data.
    try {
      const doc = await openRelative("src/fallthrough/DeepConsumer.vue");
      await waitForDiagnosticsSettled(doc.uri, { timeoutMs: 10_000, stableMs: 500 });
      const lines = doc.getText().split(/\r?\n/);
      const lineIndex = lines.findIndex((l) => l.includes('tone="primary"'));
      if (lineIndex < 0) throw new Error("TEST_DEFECT: OuterWrap deep usage missing");
      const onLine = verterUnknownPropDiags(doc.uri).filter(
        (d) => d.range.start.line === lineIndex,
      );
      for (const needle of ["class", "style", "data-testid", "aria-label"]) {
        if (mentions(onLine, needle)) {
          throw new Error(
            `deep fallthrough incorrectly unknown (${needle}): ${onLine
              .map((d) => d.message)
              .join("; ")}`,
          );
        }
      }
    } catch (err) {
      failParityGap(
        this,
        "strict.vue.deep-fallthrough-still-clean",
        "ISSUE-vue-deep-fallthrough-class",
        `Deep fallthrough contract regression: ${String(err)}`,
        "product-gap",
      );
    }
  });
});
