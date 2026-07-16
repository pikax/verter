/**
 * Ecosystem path-alias smokes: @/, $lib, #imports (Nuxt-style).
 */
import { FIXTURE_NAME } from "../../../helpers";
import {
  assertCleanErrors,
  assertDefinitionTargetsFile,
  assertHoverNeedles,
  ensureParityReady,
  failParityGap,
} from "../../../lib/parityHarness";

function onlyEco(ctx: Mocha.Context): void {
  if (FIXTURE_NAME !== "ecosystem-parity")
    throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
}

suite(`Ecosystem paths [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    this.timeout(60_000);
    onlyEco(this);
    await ensureParityReady("src/App.vue");
  });

  test("eco.vue.alias-at.clean", async function () {
    onlyEco(this);
    try {
      await assertCleanErrors("src/App.vue");
    } catch (err) {
      failParityGap(this, "eco.vue.alias-at.clean", "ISSUE-eco-vue-alias", String(err));
    }
  });

  test("eco.vue.alias-at.definition", async function () {
    onlyEco(this);
    try {
      await assertDefinitionTargetsFile(
        { file: "src/App.vue", token: "libHello", occurrence: 1 },
        "src/lib/helper.ts",
      );
    } catch (err) {
      failParityGap(this, "eco.vue.alias-at.definition", "ISSUE-eco-vue-alias-def", String(err));
    }
  });

  test("eco.vue.hash-imports.hover", async function () {
    onlyEco(this);
    try {
      await assertHoverNeedles({ file: "src/App.vue", token: "useNuxtStyleFlag", occurrence: 1 }, [
        "useNuxtStyleFlag",
      ]);
    } catch (err) {
      failParityGap(this, "eco.vue.hash-imports.hover", "ISSUE-eco-hash-imports", String(err));
    }
  });

  test("eco.svelte.lib-alias.clean", async function () {
    onlyEco(this);
    try {
      await assertCleanErrors("src/KitPage.svelte");
    } catch (err) {
      failParityGap(this, "eco.svelte.lib-alias.clean", "ISSUE-eco-svelte-lib", String(err));
    }
  });

  test("eco.svelte.lib-alias.definition", async function () {
    onlyEco(this);
    try {
      await assertDefinitionTargetsFile(
        { file: "src/KitPage.svelte", token: "libHello", occurrence: 1 },
        "src/lib/helper.ts",
      );
    } catch (err) {
      failParityGap(
        this,
        "eco.svelte.lib-alias.definition",
        "ISSUE-eco-svelte-lib-def",
        String(err),
      );
    }
  });

  test("eco.svelte.lib-alias.hover", async function () {
    onlyEco(this);
    try {
      await assertHoverNeedles({ file: "src/KitPage.svelte", token: "msg", occurrence: 1 }, [
        "msg",
      ]);
    } catch (err) {
      failParityGap(this, "eco.svelte.lib-alias.hover", "ISSUE-eco-svelte-lib-hover", String(err));
    }
  });

  test("eco.nuxt-like.pages-index.clean", async function () {
    onlyEco(this);
    try {
      await assertCleanErrors("src/pages/index.vue");
      await assertDefinitionTargetsFile(
        { file: "src/pages/index.vue", token: "libHello", occurrence: 1 },
        "src/lib/helper.ts",
      );
    } catch (err) {
      failParityGap(
        this,
        "eco.nuxt-like.pages-index.clean",
        "ISSUE-eco-nuxt-pages",
        `Nuxt-like pages/ typed surface failed: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("eco.nuxt-like.composable-via-hash-imports", async function () {
    onlyEco(this);
    try {
      // useEcoLabel is re-exported from #imports
      await assertHoverNeedles(
        { file: "src/nuxt-style/imports.ts", token: "useEcoLabel", occurrence: 0 },
        ["useEcoLabel"],
      );
    } catch (err) {
      failParityGap(
        this,
        "eco.nuxt-like.composable-via-hash-imports",
        "ISSUE-eco-nuxt-composable",
        `#imports composable export failed: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("eco.kit-like.routes-page.clean", async function () {
    onlyEco(this);
    try {
      await assertCleanErrors("src/routes/+page.svelte");
      await assertDefinitionTargetsFile(
        { file: "src/routes/+page.svelte", token: "libHello", occurrence: 1 },
        "src/lib/helper.ts",
      );
    } catch (err) {
      failParityGap(
        this,
        "eco.kit-like.routes-page.clean",
        "ISSUE-eco-kit-routes",
        `SvelteKit-like routes/+page $lib surface failed: ${String(err)}`,
        "product-gap",
      );
    }
  });
});
