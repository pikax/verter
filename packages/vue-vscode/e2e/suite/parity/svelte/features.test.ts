/**
 * Svelte extended surfaces: bind:value, $bindable, #await, snippets, $effect.
 */
import { FIXTURE_NAME } from "../../../helpers";
import {
  assertCleanErrors,
  assertDefinitionTargetsToken,
  assertHoverNeedles,
  ensureParityReady,
  failParityGap,
} from "../../../lib/parityHarness";

function onlySvelteParity(ctx: Mocha.Context): void {
  if (FIXTURE_NAME !== "svelte-parity")
    throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
}

suite(`Svelte extended features [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    this.timeout(60_000);
    onlySvelteParity(this);
    await ensureParityReady("src/App.svelte");
  });

  test("svelte.feature.bind-value.definition", async function () {
    onlySvelteParity(this);
    try {
      await assertDefinitionTargetsToken(
        { file: "src/features/BindValue.svelte", token: "name", occurrence: 1 },
        { file: "src/features/BindValue.svelte", token: "name", occurrence: 0 },
      );
    } catch (err) {
      failParityGap(
        this,
        "svelte.feature.bind-value.definition",
        "ISSUE-svelte-bind-value",
        `bind:value definition failed: ${String(err)}`,
      );
    }
  });

  test("svelte.feature.bind-value.hover", async function () {
    onlySvelteParity(this);
    try {
      await assertHoverNeedles(
        { file: "src/features/BindValue.svelte", token: "name", occurrence: 2 },
        ["name"],
      );
    } catch (err) {
      failParityGap(
        this,
        "svelte.feature.bind-value.hover",
        "ISSUE-svelte-bind-value-hover",
        `bind:value markup hover failed: ${String(err)}`,
      );
    }
  });

  test("svelte.feature.bindable.clean", async function () {
    onlySvelteParity(this);
    try {
      await assertCleanErrors("src/features/BindableParent.svelte");
    } catch (err) {
      failParityGap(
        this,
        "svelte.feature.bindable.clean",
        "ISSUE-svelte-bindable",
        `$bindable parent/child surface not clean: ${String(err)}`,
      );
    }
  });

  test("svelte.feature.await.then-hover", async function () {
    onlySvelteParity(this);
    try {
      await assertHoverNeedles(
        { file: "src/features/AwaitCase.svelte", token: "result", occurrence: 1 },
        ["label"],
      );
    } catch (err) {
      failParityGap(
        this,
        "svelte.feature.await.then-hover",
        "ISSUE-svelte-await",
        `{#await} then-local hover incomplete: ${String(err)}`,
      );
    }
  });

  test("svelte.feature.snippet.param-hover", async function () {
    onlySvelteParity(this);
    try {
      await assertHoverNeedles(
        { file: "src/features/SnippetParent.svelte", token: "name", occurrence: 1 },
        ["name"],
      );
    } catch (err) {
      failParityGap(
        this,
        "svelte.feature.snippet.param-hover",
        "ISSUE-svelte-snippet",
        `Snippet parameter hover incomplete: ${String(err)}`,
      );
    }
  });

  test("svelte.feature.effect.clean", async function () {
    onlySvelteParity(this);
    try {
      await assertCleanErrors("src/features/EffectCase.svelte");
      await assertHoverNeedles(
        { file: "src/features/EffectCase.svelte", token: "count", occurrence: 2 },
        ["number", "count"],
      );
    } catch (err) {
      failParityGap(
        this,
        "svelte.feature.effect.clean",
        "ISSUE-svelte-effect",
        `$effect surface incomplete: ${String(err)}`,
      );
    }
  });
});
