/**
 * Mixed Vue + Svelte workspace: both carriers open and stay usable.
 */
import { FIXTURE_NAME } from "../../../helpers";
import {
  assertCleanErrors,
  assertHasErrorMatching,
  assertHoverNeedles,
  ensureParityReady,
  openRelative,
  failParityGap,
} from "../../../lib/parityHarness";

function onlyMixed(ctx: Mocha.Context): void {
  if (FIXTURE_NAME !== "mixed-parity")
    throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
}

suite(`Mixed Vue+Svelte workspace [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    this.timeout(60_000);
    onlyMixed(this);
    await ensureParityReady("src/App.vue");
  });

  test("mixed.vue.entry.clean", async function () {
    onlyMixed(this);
    try {
      await assertCleanErrors("src/App.vue");
    } catch (err) {
      failParityGap(
        this,
        "mixed.vue.entry.clean",
        "ISSUE-mixed-vue-entry",
        `Mixed workspace Vue entry not clean: ${String(err)}`,
      );
    }
  });

  test("mixed.svelte.child.opens", async function () {
    onlyMixed(this);
    try {
      const doc = await openRelative("src/SvelteChild.svelte");
      if (doc.languageId !== "svelte") {
        throw new Error(`expected languageId svelte, got ${doc.languageId}`);
      }
      await assertCleanErrors("src/SvelteChild.svelte");
    } catch (err) {
      failParityGap(
        this,
        "mixed.svelte.child.opens",
        "ISSUE-mixed-svelte-child",
        `Mixed workspace Svelte child failed: ${String(err)}`,
      );
    }
  });

  test("mixed.vue.hover.local", async function () {
    onlyMixed(this);
    try {
      await assertHoverNeedles({ file: "src/App.vue", token: "mixedLabel", occurrence: 1 }, [
        "mixedLabel",
      ]);
    } catch (err) {
      failParityGap(
        this,
        "mixed.vue.hover.local",
        "ISSUE-mixed-vue-hover",
        `Mixed Vue hover failed: ${String(err)}`,
      );
    }
  });

  test("mixed.svelte.root.hover", async function () {
    onlyMixed(this);
    try {
      await openRelative("src/MixedRoot.svelte");
      await assertHoverNeedles(
        { file: "src/MixedRoot.svelte", token: "mixedRoot", occurrence: 1 },
        ["mixedRoot"],
      );
    } catch (err) {
      failParityGap(
        this,
        "mixed.svelte.root.hover",
        "ISSUE-mixed-svelte-hover",
        `Mixed Svelte root hover failed: ${String(err)}`,
      );
    }
  });

  test("mixed.cross-import.vue-imports-svelte", async function () {
    onlyMixed(this);
    try {
      // App.vue imports SvelteChild.svelte — must remain error-clean if supported.
      await assertCleanErrors("src/App.vue");
    } catch (err) {
      failParityGap(
        this,
        "mixed.cross-import.vue-imports-svelte",
        "ISSUE-mixed-cross-import",
        `Vue importing Svelte is not error-clean: ${String(err)}`,
      );
    }
  });

  test("mixed.cross-import.wrong-prop-types", async function () {
    onlyMixed(this);
    try {
      // Absolute contract: wrong title/label types fail in mixed workspace (both carriers).
      await assertHasErrorMatching(
        "src/MixedWrongProp.vue",
        /2322|2345|type|string|number|assignable/i,
      );
    } catch (err) {
      failParityGap(
        this,
        "mixed.cross-import.wrong-prop-types",
        "ISSUE-mixed-wrong-prop-types",
        `Mixed Vue/Svelte wrong props must type-error: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("mixed.child.prop-hover", async function () {
    onlyMixed(this);
    try {
      await assertHoverNeedles({ file: "src/App.vue", token: "mixedLabel", occurrence: 2 }, [
        "mixedLabel",
      ]);
    } catch (err) {
      failParityGap(
        this,
        "mixed.child.prop-hover",
        "ISSUE-mixed-child-prop-hover",
        `Hover on prop binding to cross-framework child failed: ${String(err)}`,
        "product-gap",
      );
    }
  });
});
