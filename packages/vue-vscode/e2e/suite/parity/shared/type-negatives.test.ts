/**
 * Intentional type-negative coverage (Vue + Svelte, equal bar).
 *
 * Two mechanisms:
 * 1. **Live error carriers** — markup/script that should surface TS2322 / TS2345 /
 *    TS2769 (or equivalent) diagnostics for wrong props, event handlers, attrs.
 * 2. **`@ts-expect-error` files** — must stay clean; unused expect-error (TS2578)
 *    means the surface went `any` / stopped typechecking (false green).
 *
 * Keep fixtures small; no multi-minute timeouts.
 */
import { FIXTURE_NAME } from "../../../helpers";
import {
  assertErrorCountAtLeast,
  assertHasErrorMatching,
  assertTsExpectErrorFileHolds,
  ensureParityReady,
  failParityGap,
} from "../../../lib/parityHarness";

function parityFramework(): "vue" | "svelte" | null {
  if (FIXTURE_NAME === "vue-parity") return "vue";
  if (FIXTURE_NAME === "svelte-parity") return "svelte";
  return null;
}

/** Common assignability / overload diagnostic codes + wording. */
const TYPE_MISMATCH =
  /2322|2345|2353|2769|2554|type|assignable|overload|not assignable|Argument of type|Property/i;

suite(`Type negatives (@ts-expect-error + live diagnostics) [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    const fw = parityFramework();
    if (!fw) {
      throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    }
    this.timeout(20_000);
    await ensureParityReady(fw === "vue" ? "src/App.vue" : "src/App.svelte");
  });

  test("type-neg.expect-error.props-and-handlers", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file =
      fw === "vue"
        ? "src/diagnostics/TypeNegExpectError.ts"
        : "src/diagnostics/TypeNegExpectError.ts";
    try {
      // Vue: 3 prop + 3 handler = 6; Svelte: + optional onPick = 6+
      await assertTsExpectErrorFileHolds(file, fw === "vue" ? 6 : 6);
    } catch (err) {
      failParityGap(
        this,
        "type-neg.expect-error.props-and-handlers",
        fw === "vue" ? "ISSUE-vue-type-neg-expect-error" : "ISSUE-svelte-type-neg-expect-error",
        `@ts-expect-error negatives failed (surface may be any, or expect-error unused): ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("type-neg.live.wrong-prop-types", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file =
      fw === "vue"
        ? "src/diagnostics/TypeNegWrongProps.vue"
        : "src/diagnostics/TypeNegWrongProps.svelte";
    try {
      await assertHasErrorMatching(file, TYPE_MISMATCH);
      // Multiple wrong props in the fixture — require more than a single soft warn.
      await assertErrorCountAtLeast(file, 1);
    } catch (err) {
      failParityGap(
        this,
        "type-neg.live.wrong-prop-types",
        fw === "vue" ? "ISSUE-vue-type-neg-props" : "ISSUE-svelte-type-neg-props",
        `Wrong prop types produced no type diagnostic: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("type-neg.live.wrong-event-handlers", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file =
      fw === "vue"
        ? "src/diagnostics/TypeNegWrongEvents.vue"
        : "src/diagnostics/TypeNegWrongEvents.svelte";
    try {
      // Wrong emit/callback payload and/or native click overload.
      await assertHasErrorMatching(file, TYPE_MISMATCH);
    } catch (err) {
      failParityGap(
        this,
        "type-neg.live.wrong-event-handlers",
        fw === "vue" ? "ISSUE-vue-type-neg-events" : "ISSUE-svelte-type-neg-events",
        `Wrong event/handler types produced no diagnostic (expect overload/assignability): ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("type-neg.live.wrong-directive-or-attr-types", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file =
      fw === "vue"
        ? "src/diagnostics/TypeNegWrongDirectives.vue"
        : "src/diagnostics/TypeNegWrongDirectives.svelte";
    try {
      await assertHasErrorMatching(file, TYPE_MISMATCH);
    } catch (err) {
      failParityGap(
        this,
        "type-neg.live.wrong-directive-or-attr-types",
        fw === "vue" ? "ISSUE-vue-type-neg-directives" : "ISSUE-svelte-type-neg-directives",
        `Wrong directive/attr types produced no diagnostic: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("type-neg.legacy.bad-prop-parent-still-errors", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    // Keep the original BadPropParent fixtures in the denser negative gate.
    const file =
      fw === "vue" ? "src/diagnostics/BadPropParent.vue" : "src/diagnostics/BadPropParent.svelte";
    try {
      await assertHasErrorMatching(file, /2322|2345|type|number|string|assignable/i);
    } catch (err) {
      failParityGap(
        this,
        "type-neg.legacy.bad-prop-parent-still-errors",
        fw === "vue" ? "ISSUE-vue-bad-prop-diagnostic" : "ISSUE-svelte-bad-prop-diagnostic",
        `BadPropParent regression: ${String(err)}`,
        "product-gap",
      );
    }
  });
});
