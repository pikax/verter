/**
 * Svelte daily IDE surface: script ↔ markup, child props, basic diagnostics.
 */
import { FIXTURE_NAME } from "../../../helpers";
import {
  assertCleanErrors,
  assertCompletionsInclude,
  assertDefinitionTargetsToken,
  assertHasErrorMatching,
  assertHoverNeedles,
  assertReferenceCountAtLeast,
  ensureParityReady,
  failParityGap,
  type TokenAnchor,
} from "../../../lib/parityHarness";

function onlySvelteParity(ctx: Mocha.Context): void {
  if (FIXTURE_NAME !== "svelte-parity")
    throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
}

suite(`Svelte daily surface [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    this.timeout(60_000);
    onlySvelteParity(this);
    await ensureParityReady("src/App.svelte");
  });

  test("svelte.clean-diagnostics.daily", async function () {
    onlySvelteParity(this);
    await assertCleanErrors("src/DailyBinding.svelte");
    await assertCleanErrors("src/JsDaily.svelte");
  });

  test("svelte.definition.markup-to-script", async function () {
    onlySvelteParity(this);
    // Occurrences: 0 decl, 1–2 script body, 3–4 markup.
    const markup: TokenAnchor = {
      file: "src/DailyBinding.svelte",
      token: "dailyValue",
      occurrence: 3,
    };
    const decl: TokenAnchor = {
      file: "src/DailyBinding.svelte",
      token: "dailyValue",
      occurrence: 0,
    };
    try {
      await assertDefinitionTargetsToken(markup, decl);
    } catch (err) {
      failParityGap(
        this,
        "svelte.definition.markup-to-script",
        "ISSUE-svelte-markup-definition",
        `Markup→script definition failed: ${String(err)}`,
      );
    }
  });

  test("svelte.hover.typed-markup", async function () {
    onlySvelteParity(this);
    try {
      await assertHoverNeedles(
        { file: "src/DailyBinding.svelte", token: "dailyValue", occurrence: 3 },
        ["dailyValue"],
      );
    } catch (err) {
      failParityGap(
        this,
        "svelte.hover.typed-markup",
        "ISSUE-svelte-markup-hover",
        `Markup hover not typed: ${String(err)}`,
      );
    }
  });

  test("svelte.completion.markup-locals", async function () {
    onlySvelteParity(this);
    try {
      await assertCompletionsInclude(
        { file: "src/DailyBinding.svelte", token: "dailyValue", occurrence: 3, caretOffset: 0 },
        ["dailyValue"],
      );
    } catch (err) {
      failParityGap(
        this,
        "svelte.completion.markup-locals",
        "ISSUE-svelte-markup-completion",
        `Markup local completions missing: ${String(err)}`,
      );
    }
  });

  test("svelte.references.script-and-markup", async function () {
    onlySvelteParity(this);
    await assertReferenceCountAtLeast(
      { file: "src/DailyBinding.svelte", token: "dailyValue", occurrence: 0 },
      3,
    );
  });

  test("svelte.definition.child-prop-attr", async function () {
    onlySvelteParity(this);
    try {
      await assertDefinitionTargetsToken(
        { file: "src/components/PropParent.svelte", token: "contractProp", occurrence: 1 },
        { file: "src/components/PropChild.svelte", token: "contractProp", occurrence: 0 },
      );
    } catch (err) {
      failParityGap(
        this,
        "svelte.definition.child-prop-attr",
        "ISSUE-svelte-prop-attr-definition",
        `Prop attribute definition did not reach $props field: ${String(err)}`,
      );
    }
  });

  test("svelte.hover.child-prop-attr", async function () {
    onlySvelteParity(this);
    try {
      await assertHoverNeedles(
        { file: "src/components/PropParent.svelte", token: "contractProp", occurrence: 1 },
        ["string"],
        { forbidUnknown: true },
      );
    } catch (err) {
      failParityGap(
        this,
        "svelte.hover.child-prop-attr",
        "ISSUE-svelte-prop-attr-hover",
        `Child prop attribute hover not typed: ${String(err)}`,
      );
    }
  });

  test("svelte.completion.child-props", async function () {
    onlySvelteParity(this);
    try {
      await assertCompletionsInclude(
        {
          file: "src/components/PropParent.svelte",
          token: "PropChild",
          occurrence: 1,
          caretOffset: 9,
        },
        ["contractProp"],
      );
    } catch (err) {
      failParityGap(
        this,
        "svelte.completion.child-props",
        "ISSUE-svelte-prop-completion",
        `Child prop completions incomplete: ${String(err)}`,
      );
    }
  });

  test("svelte.diagnostics.bad-prop-type", async function () {
    onlySvelteParity(this);
    try {
      await assertHasErrorMatching(
        "src/diagnostics/BadPropParent.svelte",
        /2322|2345|type|number|string/i,
      );
    } catch (err) {
      failParityGap(
        this,
        "svelte.diagnostics.bad-prop-type",
        "ISSUE-svelte-bad-prop-diagnostic",
        `Wrong prop type did not produce a diagnostic: ${String(err)}`,
      );
    }
  });
});
