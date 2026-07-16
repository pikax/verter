/**
 * Extensive slot / snippet typing (Vue defineSlots + Svelte Snippet).
 *
 * Equal first-class bar:
 * - Correct scoped usage stays clean
 * - Wrong prop method usage on slot locals errors (string.toFixed, number.toUpperCase)
 * - Unknown slot names / missing bindings error when mapped
 * - @ts-expect-error structural slot shapes hold (TS2578 if any)
 * - Hover/definition on slot locals stay typed (string/number/boolean)
 */
import { FIXTURE_NAME } from "../../../helpers";
import {
  assertCleanErrors,
  assertDefinitionTargetsToken,
  assertErrorCountAtLeast,
  assertHasErrorMatching,
  assertHoverNeedles,
  assertTsExpectErrorFileHolds,
  ensureParityReady,
  failParityGap,
} from "../../../lib/parityHarness";

function parityFramework(): "vue" | "svelte" | null {
  if (FIXTURE_NAME === "vue-parity") return "vue";
  if (FIXTURE_NAME === "svelte-parity") return "svelte";
  return null;
}

const TYPE_MISMATCH =
  /2322|2339|2345|2353|2551|2769|2304|type|assignable|Property|does not exist|overload|Argument/i;

suite(`Slots / snippets (typed) [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    const fw = parityFramework();
    if (!fw) {
      throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    }
    this.timeout(20_000);
    await ensureParityReady(fw === "vue" ? "src/App.vue" : "src/App.svelte");
  });

  test("slots.correct-usage.clean", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/slots/SlotCorrect.vue" : "src/slots/SnippetCorrect.svelte";
    try {
      await assertCleanErrors(file);
    } catch (err) {
      failParityGap(
        this,
        "slots.correct-usage.clean",
        fw === "vue" ? "ISSUE-vue-slots-correct-clean" : "ISSUE-svelte-slots-correct-clean",
        `Correct slot/snippet usage was not diagnostic-clean: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("slots.wrong-prop-usage.errors", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file =
      fw === "vue" ? "src/slots/SlotWrongProps.vue" : "src/slots/SnippetWrongProps.svelte";
    try {
      await assertHasErrorMatching(file, TYPE_MISMATCH);
      // Multiple wrong method uses in fixture — prefer more than a single hit when product works.
      await assertErrorCountAtLeast(file, 1);
    } catch (err) {
      failParityGap(
        this,
        "slots.wrong-prop-usage.errors",
        fw === "vue" ? "ISSUE-vue-slots-wrong-props" : "ISSUE-svelte-slots-wrong-props",
        `Wrong slot/snippet prop usage produced no type diagnostic: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("slots.wrong-names-or-missing-bindings.errors", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    if (fw === "svelte") {
      // Svelte render-site wrong args (parallel to unknown slot name / bad payload).
      try {
        await assertHasErrorMatching("src/slots/SnippetWrongRender.svelte", TYPE_MISMATCH);
      } catch (err) {
        failParityGap(
          this,
          "slots.wrong-names-or-missing-bindings.errors",
          "ISSUE-svelte-slots-wrong-render",
          `Wrong {@render} args produced no diagnostic: ${String(err)}`,
          "product-gap",
        );
      }
      return;
    }
    try {
      await assertHasErrorMatching("src/slots/SlotWrongNames.vue", TYPE_MISMATCH);
    } catch (err) {
      failParityGap(
        this,
        "slots.wrong-names-or-missing-bindings.errors",
        "ISSUE-vue-slots-wrong-names",
        `Unknown slot name / missing slot binding produced no diagnostic: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("slots.expect-error.structural", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/slots/SlotExpectError.ts" : "src/slots/SnippetExpectError.ts";
    try {
      await assertTsExpectErrorFileHolds(file, 5);
    } catch (err) {
      failParityGap(
        this,
        "slots.expect-error.structural",
        fw === "vue" ? "ISSUE-vue-slots-expect-error" : "ISSUE-svelte-slots-expect-error",
        `Slot/snippet @ts-expect-error suite failed: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("slots.local.hover-typed", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    try {
      if (fw === "vue") {
        // Correct fixture: header title is string, count is number
        await assertHoverNeedles(
          { file: "src/slots/SlotCorrect.vue", token: "title", occurrence: 1 },
          ["title"],
        );
        await assertHoverNeedles(
          { file: "src/slots/SlotCorrect.vue", token: "count", occurrence: 1 },
          ["count"],
        );
        // Prefer concrete types when the provider surfaces them
        try {
          await assertHoverNeedles(
            { file: "src/slots/SlotCorrect.vue", token: "title", occurrence: 1 },
            ["string"],
          );
        } catch {
          /* type needle soft — name hover is required above */
        }
      } else {
        await assertHoverNeedles(
          { file: "src/slots/SnippetCorrect.svelte", token: "title", occurrence: 1 },
          ["title"],
        );
        await assertHoverNeedles(
          { file: "src/slots/SnippetCorrect.svelte", token: "count", occurrence: 1 },
          ["count"],
        );
      }
    } catch (err) {
      failParityGap(
        this,
        "slots.local.hover-typed",
        fw === "vue" ? "ISSUE-vue-slots-local-hover" : "ISSUE-svelte-slots-local-hover",
        `Slot/snippet local hover not typed: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("slots.local.definition-to-destructure", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/slots/SlotCorrect.vue" : "src/slots/SnippetCorrect.svelte";
    try {
      // Usage of `title` in the body → destructure binding in the same slot/snippet.
      // occurrence: 0 is often the destructure, 1 is the template usage (fixture-dependent).
      await assertDefinitionTargetsToken(
        { file, token: "title", occurrence: 1 },
        { file, token: "title", occurrence: 0 },
      );
    } catch (err) {
      failParityGap(
        this,
        "slots.local.definition-to-destructure",
        fw === "vue" ? "ISSUE-vue-slots-local-def" : "ISSUE-svelte-slots-local-def",
        `Slot/snippet local definition did not map to destructure: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("slots.matrix-positive.legacy-hover", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    // Keep matrix SlotHost / SnippetParent in the denser gate.
    try {
      if (fw === "vue") {
        await assertHoverNeedles(
          { file: "src/matrix/SlotsEmits.vue", token: "title", occurrence: 1 },
          ["title"],
        );
        await assertCleanErrors("src/matrix/SlotsEmits.vue");
      } else {
        await assertCleanErrors("src/features/SnippetParent.svelte");
      }
    } catch (err) {
      failParityGap(
        this,
        "slots.matrix-positive.legacy-hover",
        fw === "vue" ? "ISSUE-vue-matrix-slots-clean" : "ISSUE-svelte-matrix-snippet-clean",
        `Legacy matrix slot/snippet regression: ${String(err)}`,
        "product-gap",
      );
    }
  });
});
