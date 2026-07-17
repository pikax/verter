/**
 * Vue daily IDE surface: script ↔ markup bindings, child props, basic diagnostics.
 */
import { strict as assert } from "node:assert";
import { FIXTURE_NAME, waitForFileReady } from "../../../helpers";
import * as vscode from "vscode";
import {
  assertCleanErrors,
  assertCompletionsInclude,
  assertDefinitionTargetsToken,
  assertHasErrorMatching,
  assertHoverNeedles,
  assertReferenceCountAtLeast,
  ensureParityReady,
  failParityGap,
  openRelative,
  pollUntil,
  settledDiagnostics,
  type TokenAnchor,
} from "../../../lib/parityHarness";

const ENTRY = "src/App.vue";

function onlyVueParity(ctx: Mocha.Context): void {
  if (FIXTURE_NAME !== "vue-parity") {
    throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
  }
}

suite(`Vue daily surface [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    this.timeout(60_000);
    onlyVueParity(this);
    await ensureParityReady(ENTRY);
  });

  test("vue.clean-diagnostics.daily", async function () {
    onlyVueParity(this);
    await assertCleanErrors("src/DailyBinding.vue");
    await assertCleanErrors("src/JsDaily.vue");
  });

  test("vue.diagnostics.jsx-environment-isolated-from-react-children", async function () {
    onlyVueParity(this);
    for (const file of [
      "src/diagnostics/JsxEnvironmentTs.vue",
      "src/diagnostics/JsxEnvironmentJs.vue",
    ]) {
      const doc = await openRelative(file);
      await waitForFileReady(doc);
      await assertCleanErrors(file);
    }
  });

  test("vue.definition.markup-to-script", async function () {
    onlyVueParity(this);
    // Occurrences in DailyBinding.vue: 0 decl, 1–2 script body, 3–4 template.
    const markup: TokenAnchor = {
      file: "src/DailyBinding.vue",
      token: "dailyValue",
      occurrence: 3,
    };
    const decl: TokenAnchor = {
      file: "src/DailyBinding.vue",
      token: "dailyValue",
      occurrence: 0,
    };
    try {
      await assertDefinitionTargetsToken(markup, decl);
    } catch (err) {
      failParityGap(
        this,
        "vue.definition.markup-to-script",
        "ISSUE-vue-markup-definition",
        `Markup→script definition failed: ${String(err)}`,
      );
    }
  });

  test("vue.hover.typed-markup", async function () {
    onlyVueParity(this);
    await assertHoverNeedles({ file: "src/DailyBinding.vue", token: "dailyValue", occurrence: 3 }, [
      "dailyValue",
    ]);
  });

  test("vue.completion.markup-locals", async function () {
    onlyVueParity(this);
    // Caret inside the mustache identifier; completions should include locals.
    try {
      await assertCompletionsInclude(
        { file: "src/DailyBinding.vue", token: "dailyValue", occurrence: 3, caretOffset: 0 },
        ["dailyValue"],
      );
    } catch (err) {
      // Some providers complete only after a partial prefix; retry mid-token.
      try {
        await assertCompletionsInclude(
          { file: "src/DailyBinding.vue", token: "dailyValue", occurrence: 3, caretOffset: 3 },
          ["dailyValue"],
        );
      } catch (inner) {
        failParityGap(
          this,
          "vue.completion.markup-locals",
          "ISSUE-vue-markup-completion",
          `Markup local completions missing: ${String(err)}; retry: ${String(inner)}`,
        );
      }
    }
  });

  test("vue.references.script-and-markup", async function () {
    onlyVueParity(this);
    // declaration + renderDaily body uses + two template uses ≥ 3
    await assertReferenceCountAtLeast(
      { file: "src/DailyBinding.vue", token: "dailyValue", occurrence: 0 },
      3,
    );
  });

  test("vue.definition.child-prop-attr", async function () {
    onlyVueParity(this);
    try {
      await assertDefinitionTargetsToken(
        { file: "src/components/PropParent.vue", token: "contract-prop", occurrence: 0 },
        { file: "src/components/PropChild.vue", token: "contractProp", occurrence: 0 },
      );
    } catch (err) {
      failParityGap(
        this,
        "vue.definition.child-prop-attr",
        "ISSUE-vue-prop-attr-definition",
        `Prop attribute definition did not reach child defineProps field: ${String(err)}`,
      );
    }
  });

  test("vue.hover.child-prop-attr", async function () {
    onlyVueParity(this);
    // Provider may show the prop type as `string` or a literal like `"from-parent"`.
    await assertHoverNeedles(
      { file: "src/components/PropParent.vue", token: "contract-prop", occurrence: 0 },
      ["contract-prop"],
      { forbidUnknown: true },
    );
  });

  test("vue.completion.child-props", async function () {
    onlyVueParity(this);
    const doc = await openRelative("src/components/PropParent.vue");
    const idx = doc.getText().indexOf("<PropChild ");
    if (idx < 0) throw new Error("missing <PropChild usage");
    const pos = doc.positionAt(idx + "<PropChild ".length);
    const list = await pollUntil(
      "prop attr completions",
      async () =>
        (await vscode.commands.executeCommand<vscode.CompletionList>(
          "vscode.executeCompletionItemProvider",
          doc.uri,
          pos,
        )) ?? { items: [], isIncomplete: false },
      (result) => (result.items?.length ?? 0) > 0,
    );
    const labels = (list.items ?? []).map((item) =>
      typeof item.label === "string" ? item.label : item.label.label,
    );
    const hasContract = labels.some((l) => l === "contractProp" || l === "contract-prop");
    const hasOptional = labels.some((l) => l === "optionalFlag" || l === "optional-flag");
    if (!hasContract || !hasOptional) {
      throw new Error(
        `child prop completions missing contractProp/optionalFlag; got: ${labels.slice(0, 40).join(", ")}`,
      );
    }
  });

  test("vue.diagnostics.bad-prop-type", async function () {
    onlyVueParity(this);
    try {
      await assertHasErrorMatching(
        "src/diagnostics/BadPropParent.vue",
        /2322|2345|type|number|string/i,
      );
    } catch (err) {
      failParityGap(
        this,
        "vue.diagnostics.bad-prop-type",
        "ISSUE-vue-bad-prop-diagnostic",
        `Wrong child prop type produced no diagnostic: ${String(err)}`,
      );
    }
  });

  test("vue.diagnostics.unresolved-markup-name", async function () {
    onlyVueParity(this);
    try {
      await assertHasErrorMatching(
        "src/diagnostics/UnresolvedMarkup.vue",
        /2304|totallyMissingName|Cannot find name/i,
      );
    } catch (err) {
      failParityGap(
        this,
        "vue.diagnostics.unresolved-markup-name",
        "ISSUE-vue-markup-unresolved",
        `Markup-region unresolved identifier did not surface a mapped diagnostic: ${String(err)}`,
        "architecture",
      );
    }
  });

  test("vue.diagnostics.unused-script-template-css-binding", async function () {
    onlyVueParity(this);
    const file = "src/diagnostics/UnusedBindings.vue";
    const doc = await openRelative(file);
    const diagnostics = await settledDiagnostics(file);
    const errors = diagnostics.filter(
      (diagnostic) => diagnostic.severity === vscode.DiagnosticSeverity.Error,
    );
    assert.deepEqual(
      errors.map(
        (diagnostic) =>
          `${diagnostic.source ?? "unknown"}:${String(diagnostic.code)}:${diagnostic.message}`,
      ),
      [],
      `${file} must remain error-clean`,
    );

    const unused = diagnostics.filter((diagnostic) => {
      const code =
        typeof diagnostic.code === "object" && diagnostic.code && "value" in diagnostic.code
          ? String(diagnostic.code.value)
          : String(diagnostic.code ?? "");
      return code === "6133";
    });
    assert.equal(
      unused.length,
      1,
      `expected exactly one genuinely unused binding; got ${unused.map((diagnostic) => `${String(diagnostic.code)}:${diagnostic.message}`).join(" | ")}`,
    );
    const unusedText = doc.getText(unused[0].range);
    assert.equal(
      unusedText,
      "trulyUnused",
      `the unused diagnostic must map to the authored script binding, got ${JSON.stringify(unusedText)}`,
    );
    assert.ok(
      unused[0].tags?.includes(vscode.DiagnosticTag.Unnecessary),
      "the unused binding diagnostic must carry the Unnecessary tag for editor fading",
    );

    for (const liveBinding of ["templateOnly", "cssOnly"]) {
      assert.ok(
        !unused.some((diagnostic) => doc.getText(diagnostic.range).includes(liveBinding)),
        `${liveBinding} is live through ${liveBinding === "templateOnly" ? "template projection" : "CSS v-bind()"} and must not be reported unused`,
      );
    }
  });
});
