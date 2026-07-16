/**
 * Svelte runes and control-flow markup locals.
 */
import { FIXTURE_NAME } from "../../../helpers";
import {
  assertCleanErrors,
  assertHoverNeedles,
  ensureParityReady,
  failParityGap,
} from "../../../lib/parityHarness";

function onlySvelteParity(ctx: Mocha.Context): void {
  if (FIXTURE_NAME !== "svelte-parity")
    throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
}

suite(`Svelte runes and control flow [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    this.timeout(60_000);
    onlySvelteParity(this);
    await ensureParityReady("src/App.svelte");
  });

  test("svelte.runes.props-destructure-hover", async function () {
    onlySvelteParity(this);
    try {
      await assertHoverNeedles(
        { file: "src/components/PropChild.svelte", token: "contractProp", occurrence: 0 },
        ["string"],
      );
    } catch (err) {
      failParityGap(
        this,
        "svelte.runes.props-destructure-hover",
        "ISSUE-svelte-props-destructure-hover",
        `$props() destructured field hover not typed: ${String(err)}`,
      );
    }
  });

  test("svelte.runes.state-hover", async function () {
    onlySvelteParity(this);
    try {
      await assertHoverNeedles(
        { file: "src/runes/RunesSurface.svelte", token: "count", occurrence: 2 },
        ["number"],
      );
    } catch (err) {
      failParityGap(
        this,
        "svelte.runes.state-hover",
        "ISSUE-svelte-state-hover",
        `$state binding hover in markup not typed: ${String(err)}`,
      );
    }
  });

  test("svelte.runes.derived-hover", async function () {
    onlySvelteParity(this);
    try {
      await assertHoverNeedles(
        { file: "src/runes/RunesSurface.svelte", token: "doubled", occurrence: 1 },
        ["number"],
      );
    } catch (err) {
      failParityGap(
        this,
        "svelte.runes.derived-hover",
        "ISSUE-svelte-derived-hover",
        `$derived binding hover in markup not typed: ${String(err)}`,
      );
    }
  });

  test("svelte.control.each-item-hover", async function () {
    onlySvelteParity(this);
    try {
      await assertHoverNeedles(
        { file: "src/control/ControlFlow.svelte", token: "user", occurrence: 2 },
        ["name"],
      );
    } catch (err) {
      failParityGap(
        this,
        "svelte.control.each-item-hover",
        "ISSUE-svelte-each-hover",
        `{#each} item hover not typed: ${String(err)}`,
      );
    }
  });

  test("svelte.control.if-branch", async function () {
    onlySvelteParity(this);
    try {
      await assertHoverNeedles(
        { file: "src/control/ControlFlow.svelte", token: "selected", occurrence: 2 },
        ["name"],
      );
    } catch (err) {
      failParityGap(
        this,
        "svelte.control.if-branch",
        "ISSUE-svelte-if-narrowing",
        `{#if} narrowed binding hover incomplete: ${String(err)}`,
      );
    }
  });

  test("svelte.runes.bindable.parent-clean", async function () {
    onlySvelteParity(this);
    try {
      await assertCleanErrors("src/features/BindableParent.svelte");
      await assertHoverNeedles(
        { file: "src/features/BindableChild.svelte", token: "value", occurrence: 0 },
        ["value"],
      );
    } catch (err) {
      failParityGap(
        this,
        "svelte.runes.bindable.parent-clean",
        "ISSUE-svelte-bindable",
        `$bindable surface failed: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("svelte.runes.effect-clean", async function () {
    onlySvelteParity(this);
    try {
      await assertCleanErrors("src/features/EffectCase.svelte");
    } catch (err) {
      failParityGap(
        this,
        "svelte.runes.effect-clean",
        "ISSUE-svelte-effect-clean",
        `$effect case not clean: ${String(err)}`,
        "product-gap",
      );
    }
  });
});
