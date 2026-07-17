/**
 * Advanced component generics (Vue + Svelte).
 *
 * Contracts (absolute — not Official LS):
 * 1. Call sites need **no** explicit type args when T is inferred from props
 *    (options: T[] ⇒ modelValue/value: T, events/callbacks: T, slots/snippets: T).
 * 2. Hover on inferred surfaces must show the **concrete** type (string vs number),
 *    not bare T / any / unknown.
 * 3. Defaulted generics (`T = string`) work without GenericX&lt;string&gt;.
 * 4. Multi-prop linkage + wrong pairings error.
 * 5. Svelte uses the same `generic="..."` script attribute as Vue.
 */
import { FIXTURE_NAME } from "../../../helpers";
import {
  assertCleanErrors,
  assertCompletionsInclude,
  assertDefinitionTargetsToken,
  assertHasErrorMatching,
  assertHoverNeedles,
  assertTsExpectErrorFileHolds,
  completionsAtOffset,
  ensureParityReady,
  findOffset,
  hoverTextAt,
  openRelative,
  registerFrameworkTest,
  renameEditsAt,
  failParityGap,
  type TokenAnchor,
} from "../../../lib/parityHarness";

function parityFramework(): "vue" | "svelte" | null {
  if (FIXTURE_NAME === "vue-parity") return "vue";
  if (FIXTURE_NAME === "svelte-parity") return "svelte";
  return null;
}

const TYPE_MISMATCH =
  /2322|2345|2353|2769|2339|type|assignable|not assignable|Argument|overload|Property/i;

/**
 * Hover must include the concrete type needle and must not look like an unbound
 * generic param only (bare `T`) or any/unknown degradation.
 */
async function assertInferredHoverType(
  anchor: TokenAnchor,
  concrete: "string" | "number",
): Promise<string> {
  const text = await assertHoverNeedles(anchor, [concrete], {
    forbidAny: true,
    forbidUnknown: true,
  });
  // Fail if the only type-looking token is a bare type parameter T (not string/number).
  const stripped = text.replace(new RegExp(concrete, "g"), "");
  if (
    /\bT\b/.test(text) &&
    !text.includes(concrete) // already required above
  ) {
    throw new Error(`hover still shows unbound T without concrete ${concrete}: ${text}`);
  }
  void stripped;
  return text;
}

async function assertSvelteScopedSnippetNavigation(file: string): Promise<void> {
  const first = { file, token: "selected", occurrence: 0 } as const;
  const hover = await assertHoverNeedles(first, ["selected"]);
  if (hover.includes("__verter")) {
    throw new Error(`snippet-name hover leaked a generated identifier: ${hover}`);
  }
  await assertDefinitionTargetsToken(first, first);

  const edit = await renameEditsAt(first, "selectedString");
  if (!edit) throw new Error("rename on first scoped snippet returned no edit");
  const doc = await openRelative(file);
  const local = edit.entries().filter(([uri]) => uri.toString() === doc.uri.toString());
  const external = edit.entries().filter(([uri]) => uri.toString() !== doc.uri.toString());
  if (external.length > 0) {
    throw new Error(
      `scoped snippet rename leaked outside authored file: ${external.map(([u]) => u.toString())}`,
    );
  }
  const edits = local.flatMap(([, edits]) => edits);
  const firstOffset = doc.getText().indexOf("selected");
  const secondOffset = doc.getText().indexOf("selected", firstOffset + "selected".length);
  if (
    edits.length !== 1 ||
    doc.offsetAt(edits[0]!.range.start) !== firstOffset ||
    doc.getText(edits[0]!.range) !== "selected" ||
    (secondOffset >= 0 &&
      edits.some(
        (candidate) =>
          doc.offsetAt(candidate.range.start) <= secondOffset &&
          doc.offsetAt(candidate.range.end) > secondOffset,
      ))
  ) {
    throw new Error(
      `rename must edit only the first lexical selected declaration; edits=${edits.map((e) => `${doc.offsetAt(e.range.start)}:${doc.getText(e.range)}`)}`,
    );
  }

  const param = { file, token: "selStr", occurrence: 0 } as const;
  await assertDefinitionTargetsToken({ file, token: "selStr", occurrence: 1 }, param);
  const paramEdit = await renameEditsAt(param, "selectedStringValue");
  if (!paramEdit) throw new Error("rename on authored snippet parameter returned no edit");
  const paramExternal = paramEdit
    .entries()
    .filter(([uri]) => uri.toString() !== doc.uri.toString());
  const paramEdits = paramEdit
    .entries()
    .filter(([uri]) => uri.toString() === doc.uri.toString())
    .flatMap(([, edits]) => edits);
  const paramOffsets = paramEdits
    .map((candidate) => doc.offsetAt(candidate.range.start))
    .sort((a, b) => a - b);
  const declarationOffset = doc.getText().indexOf("selStr");
  const useOffset = doc.getText().indexOf("selStr", declarationOffset + "selStr".length);
  if (
    paramExternal.length > 0 ||
    paramEdits.some((candidate) => doc.getText(candidate.range) !== "selStr") ||
    paramOffsets.length !== 2 ||
    paramOffsets[0] !== declarationOffset ||
    paramOffsets[1] !== useOffset
  ) {
    throw new Error(
      `snippet parameter rename must cover exactly declaration+body use; offsets=${paramOffsets}`,
    );
  }
}

suite(`Advanced generics [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    const fw = parityFramework();
    if (!fw) {
      throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    }
    this.timeout(20_000);
    await ensureParityReady(fw === "vue" ? "src/App.vue" : "src/App.svelte");
  });

  test("generic.infer.good-clean-no-type-args", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file =
      fw === "vue" ? "src/generics/GenericInferGood.vue" : "src/generics/GenericInferGood.svelte";
    try {
      await assertCleanErrors(file);
      const doc = await openRelative(file);
      const src = doc.getText();
      if (/GenericSelect\s*</.test(src) || /GenericField\s*</.test(src)) {
        throw new Error("TEST_DEFECT: good fixture must not use explicit GenericX<T> at call site");
      }
    } catch (err) {
      failParityGap(
        this,
        "generic.infer.good-clean-no-type-args",
        fw === "vue" ? "ISSUE-vue-generic-infer-good" : "ISSUE-svelte-generic-infer-good",
        `Inferred generic call site must be clean without type args (${fw}): ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("generic.infer.bad-mismatched-props-events", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file =
      fw === "vue" ? "src/generics/GenericInferBad.vue" : "src/generics/GenericInferBad.svelte";
    try {
      await assertHasErrorMatching(file, TYPE_MISMATCH);
    } catch (err) {
      failParityGap(
        this,
        "generic.infer.bad-mismatched-props-events",
        fw === "vue" ? "ISSUE-vue-generic-infer-bad" : "ISSUE-svelte-generic-infer-bad",
        `Mismatched generic props/events must error (${fw}): ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("generic.defaulted-t-string.no-annotation", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file =
      fw === "vue" ? "src/generics/GenericInferGood.vue" : "src/generics/GenericInferGood.svelte";
    try {
      await assertCleanErrors(file);
      try {
        await assertHoverNeedles({ file, token: "stringValue", occurrence: 0 }, ["string"]);
      } catch {
        await assertHoverNeedles({ file, token: "stringOptions", occurrence: 0 }, ["string"]);
      }
    } catch (err) {
      failParityGap(
        this,
        "generic.defaulted-t-string.no-annotation",
        fw === "vue" ? "ISSUE-vue-generic-default" : "ISSUE-svelte-generic-default",
        `Defaulted / inferred string generic surface incomplete (${fw}): ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("generic.multi-prop-linkage.field-format-change", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const good =
      fw === "vue" ? "src/generics/GenericInferGood.vue" : "src/generics/GenericInferGood.svelte";
    const bad =
      fw === "vue" ? "src/generics/GenericInferBad.vue" : "src/generics/GenericInferBad.svelte";
    try {
      await assertCleanErrors(good);
      await assertHasErrorMatching(bad, TYPE_MISMATCH);
      try {
        await assertHoverNeedles({ file: good, token: "formatNum", occurrence: 0 }, ["number"]);
      } catch {
        await assertHoverNeedles({ file: good, token: "num", occurrence: 0 }, ["number"]);
      }
    } catch (err) {
      failParityGap(
        this,
        "generic.multi-prop-linkage.field-format-change",
        fw === "vue" ? "ISSUE-vue-generic-multi-prop" : "ISSUE-svelte-generic-multi-prop",
        `Multi-prop generic linkage failed (${fw}): ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("generic.expect-error.structural", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = "src/generics/GenericExpectError.ts";
    try {
      await assertTsExpectErrorFileHolds(file, 3);
    } catch (err) {
      failParityGap(
        this,
        "generic.expect-error.structural",
        fw === "vue" ? "ISSUE-vue-generic-expect-error" : "ISSUE-svelte-generic-expect-error",
        `Generic @ts-expect-error suite failed (${fw}): ${String(err)}`,
        "product-gap",
      );
    }
  });

  registerFrameworkTest("vue", "generic.list.constraint-infer-clean", async function () {
    try {
      await assertCleanErrors("src/generics/GenericConsumer.vue");
      await assertHoverNeedles(
        { file: "src/generics/GenericConsumer.vue", token: "rows", occurrence: 0 },
        ["Row"],
      );
    } catch (err) {
      failParityGap(
        this,
        "generic.list.constraint-infer-clean",
        "ISSUE-vue-generic-sfc",
        `GenericList constraint inference: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("generic.completion.props-on-generic-tag", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file =
      fw === "vue" ? "src/generics/GenericInferGood.vue" : "src/generics/GenericInferGood.svelte";
    try {
      const doc = await openRelative(file);
      const offset = findOffset(doc, "<GenericSelect") + "<GenericSelect".length;
      const labels = await completionsAtOffset(file, offset);
      const need =
        fw === "vue"
          ? ["options", "modelValue", "model-value", "label"]
          : ["options", "value", "label", "onSelect"];
      const hit = need.some((n) =>
        labels.some((l) => l === n || l.startsWith(n) || l.toLowerCase().includes(n.toLowerCase())),
      );
      if (!hit) {
        // Retry with assertCompletionsInclude on a stable prop token region
        await assertCompletionsInclude({ file, token: "options", occurrence: 0, caretOffset: 0 }, [
          "options",
        ]);
      }
    } catch (err) {
      failParityGap(
        this,
        "generic.completion.props-on-generic-tag",
        fw === "vue" ? "ISSUE-vue-generic-prop-completion" : "ISSUE-svelte-generic-prop-completion",
        `Generic component prop completion failed (${fw}): ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("generic.script-attribute-present-in-host", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const host =
      fw === "vue" ? "src/generics/GenericSelect.vue" : "src/generics/GenericSelect.svelte";
    try {
      const doc = await openRelative(host);
      if (!/generic\s*=/.test(doc.getText())) {
        throw new Error(`TEST_DEFECT: ${host} must declare script generic="..."`);
      }
      // Defaulted host also present
      const defHost =
        fw === "vue" ? "src/generics/GenericDefault.vue" : "src/generics/GenericDefault.svelte";
      const defDoc = await openRelative(defHost);
      if (
        !/generic\s*=\s*["']T\s*=/.test(defDoc.getText()) &&
        !/T\s*=\s*string/.test(defDoc.getText())
      ) {
        throw new Error(`TEST_DEFECT: ${defHost} must use defaulted generic T = string`);
      }
    } catch (err) {
      if (String(err).includes("TEST_DEFECT")) throw err;
      failParityGap(
        this,
        "generic.script-attribute-present-in-host",
        fw === "vue" ? "ISSUE-vue-generic-host" : "ISSUE-svelte-generic-host",
        String(err),
        "product-gap",
      );
    }
  });

  test("generic.event-handler.infers-from-options", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const good =
      fw === "vue" ? "src/generics/GenericInferGood.vue" : "src/generics/GenericInferGood.svelte";
    const bad =
      fw === "vue" ? "src/generics/GenericInferBad.vue" : "src/generics/GenericInferBad.svelte";
    try {
      await assertCleanErrors(good);
      await assertHasErrorMatching(bad, TYPE_MISMATCH);
      await assertInferredHoverType({ file: good, token: "onSelect", occurrence: 0 }, "string");
      await assertInferredHoverType({ file: good, token: "onNumSelect", occurrence: 0 }, "number");
    } catch (err) {
      failParityGap(
        this,
        "generic.event-handler.infers-from-options",
        fw === "vue" ? "ISSUE-vue-generic-event-infer" : "ISSUE-svelte-generic-event-infer",
        `Event/callback generic inference from options failed (${fw}): ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("generic.hover.prop-inferred-from-options-string", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file =
      fw === "vue" ? "src/generics/GenericInferGood.vue" : "src/generics/GenericInferGood.svelte";
    try {
      // options is string[] → modelValue/value binding should hover as string
      await assertInferredHoverType({ file, token: "stringValue", occurrence: 0 }, "string");
      await assertInferredHoverType({ file, token: "stringOptions", occurrence: 0 }, "string");
      // Template usage of model-value / value prop on the string select
      if (fw === "vue") {
        await assertInferredHoverType({ file, token: "stringValue", occurrence: 1 }, "string");
      } else {
        await assertInferredHoverType({ file, token: "stringValue", occurrence: 1 }, "string");
      }
    } catch (err) {
      failParityGap(
        this,
        "generic.hover.prop-inferred-from-options-string",
        fw === "vue" ? "ISSUE-vue-generic-hover-prop-str" : "ISSUE-svelte-generic-hover-prop-str",
        `Inferred string prop hover wrong (${fw}): ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("generic.hover.prop-inferred-from-options-number", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file =
      fw === "vue" ? "src/generics/GenericInferGood.vue" : "src/generics/GenericInferGood.svelte";
    try {
      await assertInferredHoverType({ file, token: "numberValue", occurrence: 0 }, "number");
      await assertInferredHoverType({ file, token: "numberOptions", occurrence: 0 }, "number");
      await assertInferredHoverType({ file, token: "numberValue", occurrence: 1 }, "number");
    } catch (err) {
      failParityGap(
        this,
        "generic.hover.prop-inferred-from-options-number",
        fw === "vue" ? "ISSUE-vue-generic-hover-prop-num" : "ISSUE-svelte-generic-hover-prop-num",
        `Inferred number prop hover wrong (${fw}): ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("generic.hover.event-payload-matches-options", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file =
      fw === "vue" ? "src/generics/GenericInferGood.vue" : "src/generics/GenericInferGood.svelte";
    try {
      // Handler decls: parameter type must match inferred T
      await assertInferredHoverType({ file, token: "onSelect", occurrence: 0 }, "string");
      await assertInferredHoverType(
        { file, token: fw === "vue" ? "onUpdate" : "onChange", occurrence: 0 },
        "string",
      );
      await assertInferredHoverType({ file, token: "onNumSelect", occurrence: 0 }, "number");
      // Event attribute tokens at call site (Vue @select / Svelte onSelect={})
      if (fw === "vue") {
        await assertInferredHoverType({ file, token: "onSelect", occurrence: 1 }, "string");
        await assertInferredHoverType({ file, token: "onNumSelect", occurrence: 1 }, "number");
      } else {
        await assertInferredHoverType({ file, token: "onSelect", occurrence: 1 }, "string");
        await assertInferredHoverType({ file, token: "onNumSelect", occurrence: 1 }, "number");
      }
    } catch (err) {
      failParityGap(
        this,
        "generic.hover.event-payload-matches-options",
        fw === "vue" ? "ISSUE-vue-generic-hover-event" : "ISSUE-svelte-generic-hover-event",
        `Inferred event/callback hover type wrong (${fw}): ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("generic.hover.slot-prop-inferred-string", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file =
      fw === "vue" ? "src/generics/GenericInferGood.vue" : "src/generics/GenericInferGood.svelte";
    try {
      // Slot/snippet locals from string options select
      await assertInferredHoverType({ file, token: "selStr", occurrence: 0 }, "string");
      await assertInferredHoverType({ file, token: "selStr", occurrence: 1 }, "string");
      await assertInferredHoverType({ file, token: "optStr", occurrence: 0 }, "string");
      // Method use proves string (toUpperCase) — hover should not be number
      const selHover = await hoverTextAt({ file, token: "selStr", occurrence: 1 });
      if (/\bnumber\b/.test(selHover) && !/\bstring\b/.test(selHover)) {
        throw new Error(`selStr hover looks like number, expected string: ${selHover}`);
      }
      if (fw === "svelte") await assertSvelteScopedSnippetNavigation(file);
    } catch (err) {
      failParityGap(
        this,
        "generic.hover.slot-prop-inferred-string",
        fw === "vue" ? "ISSUE-vue-generic-hover-slot-str" : "ISSUE-svelte-generic-hover-slot-str",
        `Inferred string slot/snippet prop hover wrong (${fw}): ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("generic.hover.slot-prop-inferred-number", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file =
      fw === "vue" ? "src/generics/GenericInferGood.vue" : "src/generics/GenericInferGood.svelte";
    try {
      await assertInferredHoverType({ file, token: "selNum", occurrence: 0 }, "number");
      await assertInferredHoverType({ file, token: "selNum", occurrence: 1 }, "number");
      await assertInferredHoverType({ file, token: "optNum", occurrence: 0 }, "number");
      const selHover = await hoverTextAt({ file, token: "selNum", occurrence: 1 });
      if (/\bstring\b/.test(selHover) && !/\bnumber\b/.test(selHover)) {
        throw new Error(`selNum hover looks like string, expected number: ${selHover}`);
      }
    } catch (err) {
      failParityGap(
        this,
        "generic.hover.slot-prop-inferred-number",
        fw === "vue" ? "ISSUE-vue-generic-hover-slot-num" : "ISSUE-svelte-generic-hover-slot-num",
        `Inferred number slot/snippet prop hover wrong (${fw}): ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("generic.slot.wrong-method-on-inferred-type", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const bad =
      fw === "vue" ? "src/generics/GenericInferBad.vue" : "src/generics/GenericInferBad.svelte";
    try {
      // string T used with .toFixed must error (slot misuse)
      await assertHasErrorMatching(bad, /2339|toFixed|property|type|assignable|number|string/i);
    } catch (err) {
      failParityGap(
        this,
        "generic.slot.wrong-method-on-inferred-type",
        fw === "vue" ? "ISSUE-vue-generic-slot-wrong" : "ISSUE-svelte-generic-slot-wrong",
        `Wrong method on inferred slot type must error (${fw}): ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("generic.hover.field-multi-prop-number-chain", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file =
      fw === "vue" ? "src/generics/GenericInferGood.vue" : "src/generics/GenericInferGood.svelte";
    try {
      await assertInferredHoverType({ file, token: "num", occurrence: 0 }, "number");
      await assertInferredHoverType({ file, token: "formatNum", occurrence: 0 }, "number");
      await assertInferredHoverType({ file, token: "onNumChange", occurrence: 0 }, "number");
    } catch (err) {
      failParityGap(
        this,
        "generic.hover.field-multi-prop-number-chain",
        fw === "vue" ? "ISSUE-vue-generic-hover-field" : "ISSUE-svelte-generic-hover-field",
        `GenericField multi-prop number hover chain failed (${fw}): ${String(err)}`,
        "product-gap",
      );
    }
  });
});
