/**
 * JS / @ts-check carriers (Vue + Svelte): wrong prop types must still fail.
 * Absolute TS/JSDoc contracts — not Official LS.
 */
import { FIXTURE_NAME } from "../../../helpers";
import {
  assertCleanErrors,
  assertHasErrorMatching,
  assertHoverNeedles,
  ensureParityReady,
  failParityGap,
} from "../../../lib/parityHarness";

function parityFramework(): "vue" | "svelte" | null {
  if (FIXTURE_NAME === "vue-parity") return "vue";
  if (FIXTURE_NAME === "svelte-parity") return "svelte";
  return null;
}

const TYPE_MISMATCH = /2322|2345|type|number|string|assignable|not assignable/i;

suite(`JS language surface [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    const fw = parityFramework();
    if (!fw) {
      throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    }
    this.timeout(20_000);
    await ensureParityReady(fw === "vue" ? "src/App.vue" : "src/App.svelte");
  });

  test("js.daily.clean-and-hover", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/JsDaily.vue" : "src/JsDaily.svelte";
    try {
      await assertCleanErrors(file);
      await assertHoverNeedles(
        { file, token: fw === "vue" ? "jsDaily" : "jsDaily", occurrence: 0 },
        fw === "vue" ? ["jsDaily"] : ["jsDaily"],
      );
    } catch (err) {
      // Soft name: try markup occurrence
      try {
        await assertHoverNeedles({ file, token: "label", occurrence: 0 }, ["label"]);
      } catch (inner) {
        failParityGap(
          this,
          "js.daily.clean-and-hover",
          fw === "vue" ? "ISSUE-vue-js-daily" : "ISSUE-svelte-js-daily",
          `JS daily surface failed: ${String(err)}; ${String(inner)}`,
          "product-gap",
        );
      }
    }
  });

  test("js.wrong-prop-type.errors", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/js/JsWrongProp.vue" : "src/js/JsWrongProp.svelte";
    try {
      await assertHasErrorMatching(file, TYPE_MISMATCH);
    } catch (err) {
      failParityGap(
        this,
        "js.wrong-prop-type.errors",
        fw === "vue" ? "ISSUE-vue-js-wrong-prop" : "ISSUE-svelte-js-wrong-prop",
        `JS/@ts-check wrong prop must type-error: ${String(err)}`,
        "product-gap",
      );
    }
  });
});
