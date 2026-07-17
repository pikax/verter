/**
 * Template-bound named handlers follow each framework's public contract:
 * TypeScript setup/runes handlers are contextually typed, unannotated
 * JavaScript remains unannotated, and authored JSDoc remains authoritative.
 */
import { strict as assert } from "node:assert";
import * as path from "node:path";
import * as vscode from "vscode";

import { FIXTURE_NAME } from "../../../helpers";
import {
  assertCleanErrors,
  assertCompletionsInclude,
  assertHoverNeedles,
  assertTsExpectErrorFileHolds,
  definitionsAt,
  ensureParityReady,
  failParityGap,
  hoverTextAt,
  registerFrameworkTest,
} from "../../../lib/parityHarness";

type EventFixtureKind = "js" | "js-lax" | "ts";

function framework(): "vue" | "svelte" | null {
  if (FIXTURE_NAME === "vue-parity") return "vue";
  if (FIXTURE_NAME === "svelte-parity") return "svelte";
  return null;
}

function expectedEventType(): "PointerEvent" | "MouseEvent" {
  return framework() === "svelte" ? "MouseEvent" : "PointerEvent";
}

function expectedEventMembers(): readonly string[] {
  return framework() === "svelte" ? ["clientX", "button"] : ["clientX", "pointerType"];
}

function fixtureFile(kind: EventFixtureKind, invalid = false): string {
  const fw = framework();
  if (!fw) throw new Error("TEST_DEFECT: DOM event suite loaded for wrong fixture");
  return `src/${kind}/DomEventHandler${invalid ? "Invalid" : ""}.${fw}`;
}

function jsDocFixtureFile(): string {
  const fw = framework();
  if (!fw) throw new Error("TEST_DEFECT: DOM event suite loaded for wrong fixture");
  return `src/js/JSDocEventHandler.${fw}`;
}

async function assertConcreteHover(file: string): Promise<void> {
  await assertCleanErrors(file);
  await assertHoverNeedles(
    { file, token: "domEvent", occurrence: 0 },
    ["domEvent", expectedEventType()],
    { forbidAny: true, forbidUnknown: true },
  );
}

async function assertEventCompletions(file: string): Promise<void> {
  assert.equal(
    process.env.VERTER_E2E_PROVIDER_ONLY_COMPLETIONS,
    "1",
    "TEST_DEFECT: completion contract must disable Verter-owned suggestions",
  );
  await assertCompletionsInclude(
    {
      file,
      token: "domEvent.",
      occurrence: 0,
      caretOffset: "domEvent.".length,
    },
    expectedEventMembers(),
    ".",
  );
}

async function assertDomDefinition(file: string): Promise<void> {
  const locations = await definitionsAt({ file, token: "clientX", occurrence: 0 });
  const domDefinition = locations.find(
    (location) => path.basename(location.uri.fsPath).toLowerCase() === "lib.dom.d.ts",
  );
  assert.ok(domDefinition, "clientX definition did not reach lib.dom.d.ts");
  const target = await vscode.workspace.openTextDocument(domDefinition.uri);
  assert.match(target.getText(domDefinition.range), /clientX/);
}

async function assertUnannotatedJsRemainsAny(kind: "js" | "js-lax"): Promise<void> {
  const file = fixtureFile(kind);
  const hover = await hoverTextAt({ file, token: "domEvent", occurrence: 0 });
  assert.match(hover, /:\s*any\b/, `unannotated JavaScript must remain any: ${hover}`);
  assert.doesNotMatch(
    hover,
    /\b(?:PointerEvent|MouseEvent)\b/,
    `unannotated JavaScript was over-inferred from template usage: ${hover}`,
  );
  if (kind === "js") {
    await assertTsExpectErrorFileHolds(file);
  } else {
    await assertCleanErrors(file);
  }
}

async function assertCheckedJsInvalidMemberFollowsAny(): Promise<void> {
  await assertTsExpectErrorFileHolds(fixtureFile("js", true));
}

async function assertClassicOrLegacyBoundary(kind: "js" | "ts"): Promise<void> {
  const fw = framework();
  if (!fw) throw new Error("TEST_DEFECT: DOM event boundary loaded for wrong fixture");
  const file = `src/boundary/ClassicEventHandler${kind === "ts" ? "Ts" : ""}.${fw}`;
  await assertTsExpectErrorFileHolds(file);
  const hover = await hoverTextAt({ file, token: "domEvent", occurrence: 0 });
  assert.match(hover, /:\s*any\b/, `classic/legacy handler must remain uncontextual: ${hover}`);
  assert.doesNotMatch(
    hover,
    /\b(?:PointerEvent|MouseEvent)\b/,
    `classic/legacy handler was over-inferred from template usage: ${hover}`,
  );
}

function productGap(
  context: Mocha.Context,
  id: string,
  issue: string,
  kind: string,
  capability: string,
  error: unknown,
): never {
  failParityGap(
    context,
    id,
    issue,
    `${framework()} ${kind} DOM event ${capability} failed: ${String(error)}`,
    "provider-gap",
  );
}

suite(`DOM event handler contracts [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    this.timeout(60_000);
    if (!framework()) {
      throw new Error("TEST_DEFECT: DOM event suite loaded for wrong fixture");
    }
    await ensureParityReady(fixtureFile("ts"));
  });

  test("shared.js.dom-event.unannotated-checked-remains-any", async function () {
    try {
      await assertUnannotatedJsRemainsAny("js");
    } catch (error) {
      productGap(
        this,
        this.test!.title,
        "ISSUE-js-dom-event-non-inference",
        "checked JS",
        "non-inference",
        error,
      );
    }
  });

  test("shared.js.dom-event.checked-diagnostics-follow-config", async function () {
    try {
      await assertCheckedJsInvalidMemberFollowsAny();
    } catch (error) {
      productGap(
        this,
        this.test!.title,
        "ISSUE-js-dom-event-config",
        "checked JS",
        "diagnostic policy",
        error,
      );
    }
  });

  test("shared.js-lax.dom-event.unannotated-remains-any", async function () {
    try {
      await assertUnannotatedJsRemainsAny("js-lax");
    } catch (error) {
      productGap(
        this,
        this.test!.title,
        "ISSUE-js-lax-dom-event-non-inference",
        "lax JS",
        "non-inference",
        error,
      );
    }
  });

  test("shared.js-lax.dom-event.diagnostics-follow-config", async function () {
    try {
      await assertCleanErrors(fixtureFile("js-lax", true));
    } catch (error) {
      productGap(
        this,
        this.test!.title,
        "ISSUE-js-lax-dom-event-config",
        "lax JS",
        "checkJs=false diagnostic policy",
        error,
      );
    }
  });

  test("shared.js-jsdoc.dom-event.parameter-hover-concrete", async function () {
    try {
      await assertConcreteHover(jsDocFixtureFile());
    } catch (error) {
      productGap(
        this,
        this.test!.title,
        "ISSUE-js-dom-event-jsdoc",
        "authored JSDoc",
        "hover",
        error,
      );
    }
  });

  test("shared.js-jsdoc.dom-event.member-completion", async function () {
    try {
      await assertEventCompletions(jsDocFixtureFile());
    } catch (error) {
      productGap(
        this,
        this.test!.title,
        "ISSUE-js-dom-event-jsdoc",
        "authored JSDoc",
        "completion",
        error,
      );
    }
  });

  test("shared.js-jsdoc.dom-event.member-definition", async function () {
    try {
      await assertDomDefinition(jsDocFixtureFile());
    } catch (error) {
      productGap(
        this,
        this.test!.title,
        "ISSUE-js-dom-event-jsdoc",
        "authored JSDoc",
        "definition",
        error,
      );
    }
  });

  test("shared.ts.dom-event.parameter-hover-concrete", async function () {
    try {
      await assertConcreteHover(fixtureFile("ts"));
    } catch (error) {
      productGap(this, this.test!.title, "ISSUE-ts-dom-event-hover", "TS", "hover", error);
    }
  });

  test("shared.ts.dom-event.member-completion", async function () {
    try {
      await assertEventCompletions(fixtureFile("ts"));
    } catch (error) {
      productGap(
        this,
        this.test!.title,
        "ISSUE-ts-dom-event-completion",
        "TS",
        "completion",
        error,
      );
    }
  });

  test("shared.ts.dom-event.member-definition", async function () {
    try {
      await assertDomDefinition(fixtureFile("ts"));
    } catch (error) {
      productGap(
        this,
        this.test!.title,
        "ISSUE-ts-dom-event-definition",
        "TS",
        "definition",
        error,
      );
    }
  });

  test("shared.ts.dom-event.invalid-member-expect-error-consumed", async function () {
    try {
      await assertTsExpectErrorFileHolds(fixtureFile("ts", true));
    } catch (error) {
      productGap(
        this,
        this.test!.title,
        "ISSUE-ts-dom-event-invalid-member",
        "TS",
        "invalid-member expect-error",
        error,
      );
    }
  });

  registerFrameworkTest("svelte", "svelte.ts.dom-event.button-current-target", async function () {
    try {
      const file = fixtureFile("ts");
      await assertHoverNeedles(
        { file, token: "currentTarget", occurrence: 0 },
        ["currentTarget", "HTMLButtonElement"],
        { forbidAny: true, forbidUnknown: true },
      );
      await assertCompletionsInclude(
        {
          file,
          token: "domEvent.currentTarget.",
          occurrence: 0,
          caretOffset: "domEvent.currentTarget.".length,
        },
        ["disabled", "formAction"],
        ".",
      );
    } catch (error) {
      productGap(
        this,
        this.test!.title,
        "ISSUE-svelte-dom-event-current-target",
        "TS",
        "button currentTarget",
        error,
      );
    }
  });

  registerFrameworkTest("svelte", "svelte.intrinsic.button-type-literal-clean", async function () {
    try {
      await assertCleanErrors("src/diagnostics/ButtonTypeLiteral.svelte");
    } catch (error) {
      productGap(
        this,
        this.test!.title,
        "ISSUE-svelte-button-type-literal",
        "intrinsic",
        "button type literal",
        error,
      );
    }
  });

  test("shared.js.dom-event.classic-or-legacy-not-contextual", async function () {
    try {
      await assertClassicOrLegacyBoundary("js");
    } catch (error) {
      productGap(
        this,
        this.test!.title,
        "ISSUE-dom-event-over-inference-boundary",
        "JS",
        "classic/legacy boundary",
        error,
      );
    }
  });

  test("shared.ts.dom-event.classic-or-legacy-not-contextual", async function () {
    try {
      await assertClassicOrLegacyBoundary("ts");
    } catch (error) {
      productGap(
        this,
        this.test!.title,
        "ISSUE-dom-event-over-inference-boundary",
        "TS",
        "classic/legacy boundary",
        error,
      );
    }
  });
});
