import { describe, expect, it } from "vitest";

import {
  classifyDefinition,
  classifyHoverText,
  classifyMemberCompletion,
  classifyReferences,
  declaredTypeText,
  extractCodeFences,
  isBareImportReprint,
  quickInfoPrefix,
  verterNativeFingerprint,
  type HoverProbeContract,
} from "./tsAnswer";

/**
 * Verbatim payloads produced by Verter's OWN hover formatters.
 *
 * Each is assembled exactly the way the Rust producer assembles it: the
 * formatters push markdown blocks into a `Vec<String>` and emit
 * `lines.join("\n\n")`. Reproducing that join here is what makes these
 * fixtures faithful rather than approximate — a discriminator that rejects a
 * paraphrase but accepts the real payload would be worthless.
 */
const native = {
  /** `format_binding_hover` — hover.rs:1650. `const count = ref(0)`. */
  refBinding: [
    "```typescript\nconst count\n```",
    "*(ref — needs `.value`)*",
    "Initialized via `ref()` (from `vue`)",
  ].join("\n\n"),

  /** `format_binding_hover` — a computed with an authored annotation. */
  computedBinding: [
    "```typescript\nconst label: string\n```",
    "*(computed — needs `.value`, read-only)*",
    "Initialized via `computed()` (from `vue`)",
  ].join("\n\n"),

  /**
   * `format_binding_hover` ADVERSARIAL CASE — authored annotation, reactivity
   * kind `None`, `is_reactive == false`, initializer `Other`. Every optional
   * line is suppressed, so the payload is a lone `typescript` fence carrying a
   * declaration with a type position: byte-identical in SHAPE to a real
   * quickinfo hover. This is the exact payload that makes "a declaration with a
   * type" a false-green rule.
   */
  bareAnnotatedBinding: "```typescript\nconst total: Money\n```",

  /** `format_binding_hover` with no annotation and no markers at all. */
  bareUnannotatedBinding: "```typescript\nconst payload\n```",

  /** `format_import_hover` — hover.rs:1730, with a Vue API classification. */
  vueImport: ["```typescript\nimport { ref } from 'vue'\n```", "Vue API: `Ref`"].join("\n\n"),

  /** `format_import_hover` — a plain import with no Vue API classification. */
  plainImport: "```typescript\nimport { formatMoney } from './money'\n```",

  /** `format_import_hover` — a type-only import. */
  typeImport: "```typescript\nimport type { Money } from './money'\n```",

  /** `format_macro_hover` — hover.rs:1753. */
  macro: ["```typescript\nconst props = defineProps()\n```", "Type-based: `<Props>`"].join("\n\n"),

  /** Custom-block hover — hover.rs:288. */
  customBlock: "**`<i18n>`** — Custom block.",

  /** Slot outlet hover — hover.rs:896. */
  slotOutlet: '**`<slot>`** outlet — **"footer"**',
} as const;

/** Verbatim quickinfo payloads as `tsserver`/`tsgo` render them. */
const typescript = {
  property: "```typescript\n(property) Invoice.total: Money\n```",
  alias:
    "```typescript\n(alias) function formatMoney(value: Money): string\nimport formatMoney\n```",
  method:
    "```typescript\n(method) Array<Invoice>.map<string>(callbackfn: (value: Invoice) => string): string[]\n```",
  parameter: "```typescript\n(parameter) value: Money\n```",
  localVar: "```typescript\n(local var) index: number\n```",
  refValue: "```typescript\n(property) Ref<number, number>.value: number\n```",
  /** An inferred local: `const count = ref(0)` with NO authored annotation. */
  inferredLocal: "```typescript\nconst count: Ref<number, number>\n```",
  /** Quickinfo carrying JSDoc that quotes one of Verter's own strings. */
  aliasWithAdversarialJsDoc: [
    "```typescript\n(alias) const useStore: () => Store\nimport useStore\n```",
    "---",
    "Vue API: `inject` is used internally.",
  ].join("\n\n"),
} as const;

const memberProbe: HoverProbeContract = { probeClass: "member", identifier: "total" };
const aliasProbe: HoverProbeContract = { probeClass: "alias", identifier: "formatMoney" };
const provenInferredLocal: HoverProbeContract = {
  probeClass: "inferred-local",
  identifier: "count",
  declarationHasNoAuthoredAnnotation: true,
};

describe("classifyHoverText — rejects Verter-native hovers", () => {
  it("rejects a native ref-binding hover for every probe class", () => {
    for (const probe of [memberProbe, aliasProbe, provenInferredLocal]) {
      const verdict = classifyHoverText(native.refBinding, { ...probe, identifier: "count" });
      expect(verdict.verdict, `probe ${probe.probeClass}`).toBe("verter-native");
      expect(verdict.marker).toBe("*(ref — needs `.value`)*");
    }
  });

  it("rejects a native computed-binding hover", () => {
    const verdict = classifyHoverText(native.computedBinding, {
      ...provenInferredLocal,
      identifier: "label",
    });
    expect(verdict.verdict).toBe("verter-native");
  });

  it("rejects the ADVERSARIAL bare annotated binding for member and alias probes", () => {
    // `const total: Money` is shaped exactly like a real quickinfo hover. A
    // discriminator that accepts "a declaration with a type position" reports
    // green here while the TypeScript engine is entirely absent.
    expect(classifyHoverText(native.bareAnnotatedBinding, memberProbe).verdict).toBe(
      "indeterminate",
    );
    expect(classifyHoverText(native.bareAnnotatedBinding, aliasProbe).verdict).toBe(
      "indeterminate",
    );
  });

  it("rejects the adversarial bare annotated binding for an UNPROVEN inferred-local probe", () => {
    const unproven: HoverProbeContract = { probeClass: "inferred-local", identifier: "total" };
    const verdict = classifyHoverText(native.bareAnnotatedBinding, unproven);
    expect(verdict.verdict).toBe("indeterminate");
    expect(verdict.reason).toContain("not proven annotation-free");
  });

  it("rejects a native unannotated binding on the inferred-local rail", () => {
    // The native formatter prints only the AUTHORED annotation, so with none
    // present there is no type position at all — which is exactly what makes
    // the inferred-local rail sound.
    const verdict = classifyHoverText(native.bareUnannotatedBinding, {
      probeClass: "inferred-local",
      identifier: "payload",
      declarationHasNoAuthoredAnnotation: true,
    });
    expect(verdict.verdict).toBe("indeterminate");
  });

  it("rejects a native import hover, with and without a Vue API line", () => {
    expect(classifyHoverText(native.vueImport, aliasProbe).verdict).toBe("verter-native");
    const plain = classifyHoverText(native.plainImport, aliasProbe);
    expect(plain.verdict).toBe("verter-native");
    expect(plain.reason).toContain("bare import re-print");
    expect(classifyHoverText(native.typeImport, aliasProbe).verdict).toBe("verter-native");
  });

  it("rejects a native macro hover", () => {
    const verdict = classifyHoverText(native.macro, {
      probeClass: "inferred-local",
      identifier: "props",
      declarationHasNoAuthoredAnnotation: true,
    });
    expect(verdict.verdict).toBe("verter-native");
  });

  it("rejects native block and slot documentation hovers", () => {
    expect(classifyHoverText(native.customBlock, memberProbe).verdict).toBe("verter-native");
    expect(classifyHoverText(native.slotOutlet, memberProbe).verdict).toBe("verter-native");
  });

  it("reports an empty hover as empty, not as a failure to match", () => {
    expect(classifyHoverText("", memberProbe).verdict).toBe("empty");
    expect(classifyHoverText("   \n  ", memberProbe).verdict).toBe("empty");
  });
});

describe("classifyHoverText — accepts real TypeScript answers", () => {
  it("accepts every parenthesised quickinfo kind", () => {
    const cases: Array<[string, string]> = [
      [typescript.property, "(property)"],
      [typescript.alias, "(alias)"],
      [typescript.method, "(method)"],
      [typescript.parameter, "(parameter)"],
      [typescript.localVar, "(local var)"],
      [typescript.refValue, "(property)"],
    ];
    for (const [payload, marker] of cases) {
      const verdict = classifyHoverText(payload, memberProbe);
      expect(verdict.verdict, payload).toBe("typescript");
      expect(verdict.marker).toBe(marker);
    }
  });

  it("accepts an inferred local when the probe is proven annotation-free", () => {
    const verdict = classifyHoverText(typescript.inferredLocal, provenInferredLocal);
    expect(verdict.verdict).toBe("typescript");
    expect(verdict.reason).toContain("necessarily inferred");
  });

  it("credits TypeScript even when JSDoc quotes a Verter-native string", () => {
    // The exclusive rail is evaluated first precisely so documentation text
    // cannot demote a genuine engine answer.
    const verdict = classifyHoverText(typescript.aliasWithAdversarialJsDoc, aliasProbe);
    expect(verdict.verdict).toBe("typescript");
    expect(verdict.marker).toBe("(alias)");
  });
});

describe("primitives", () => {
  it("extracts fences and ignores non-TypeScript ones", () => {
    expect(extractCodeFences("```typescript\na\n```")).toEqual(["a"]);
    expect(extractCodeFences("```html\n<div>\n```")).toEqual([]);
    expect(extractCodeFences("no fence at all")).toEqual([]);
  });

  it("only accepts a quickinfo prefix at the head of the first fence line", () => {
    expect(quickInfoPrefix("(property) A.b: string")).toBe("property");
    expect(quickInfoPrefix("const x: (property) => void")).toBeUndefined();
    expect(quickInfoPrefix("(not a real kind) x")).toBeUndefined();
  });

  it("identifies a bare import re-print but not an alias quickinfo", () => {
    expect(isBareImportReprint("import { a } from 'b'")).toBe(true);
    expect(isBareImportReprint("import type { a } from 'b'")).toBe(true);
    expect(isBareImportReprint("(alias) const a: number\nimport a")).toBe(false);
  });

  it("reads a declared type only when a type position is present", () => {
    expect(declaredTypeText("const total: Money", "total")).toBe("Money");
    expect(declaredTypeText("const total", "total")).toBeUndefined();
    expect(declaredTypeText("const other: Money", "total")).toBeUndefined();
  });

  it("finds native fingerprints", () => {
    expect(verterNativeFingerprint(native.macro)).toBe("Type-based: `<");
    expect(verterNativeFingerprint(typescript.property)).toBeUndefined();
  });
});

describe("non-hover operations never claim a TypeScript answer", () => {
  // A control run with `verter.typeProvider = off` — no engine at ALL — still
  // produced cross-file definitions into `.ts` files, foreign member
  // completions, and cross-file references, because Verter answers those
  // natively. Any classifier that read those as engine evidence would report
  // TypeScript as present on a run where it was disabled. These three therefore
  // cannot express that verdict, and this suite pins that.
  const outcomes = [
    classifyDefinition({ targetPaths: ["/w/src/money.ts"], sourcePath: "/w/src/App.vue" }),
    classifyDefinition({
      targetPaths: ["/w/node_modules/x/index.d.ts"],
      sourcePath: "/w/src/App.vue",
    }),
    classifyDefinition({ targetPaths: ["/w/src/App.vue"], sourcePath: "/w/src/App.vue" }),
    classifyDefinition({ targetPaths: [], sourcePath: "/w/src/App.vue" }),
    classifyMemberCompletion([{ label: "currencyCode" }], "const a = 1;"),
    classifyMemberCompletion([], "const a = 1;"),
    classifyReferences({
      locationPaths: ["/w/src/App.vue", "/w/src/money.ts"],
      sourcePath: "/w/src/App.vue",
    }),
    classifyReferences({ locationPaths: [], sourcePath: "/w/src/App.vue" }),
  ];

  it("never emits the `typescript` verdict from a structural payload", () => {
    for (const outcome of outcomes) {
      expect(outcome.verdict).not.toBe("typescript");
      expect(["resolved", "unresolved", "empty"]).toContain(outcome.verdict);
    }
  });

  it("still distinguishes resolved from unresolved and empty", () => {
    expect(
      classifyDefinition({ targetPaths: ["/w/src/money.ts"], sourcePath: "/w/src/App.vue" })
        .verdict,
    ).toBe("resolved");
    expect(
      classifyDefinition({ targetPaths: ["/w/src/App.vue"], sourcePath: "/w/src/App.vue" }).verdict,
    ).toBe("unresolved");
    expect(classifyDefinition({ targetPaths: [], sourcePath: "/w/src/App.vue" }).verdict).toBe(
      "empty",
    );

    const source = "const invoice = load();\ninvoice.total;";
    expect(
      classifyMemberCompletion([{ label: "total" }, { label: "invoice" }], source).verdict,
    ).toBe("unresolved");
    expect(
      classifyMemberCompletion([{ label: "total" }, { label: "currencyCode" }], source).verdict,
    ).toBe("resolved");
    expect(classifyMemberCompletion([], source).verdict).toBe("empty");

    expect(
      classifyReferences({
        locationPaths: ["/w/src/App.vue", "/w/src/money.ts"],
        sourcePath: "/w/src/App.vue",
      }).marker,
    ).toBe("cross-file");
    expect(
      classifyReferences({ locationPaths: ["/w/src/App.vue"], sourcePath: "/w/src/App.vue" })
        .marker,
    ).toBe("same-file");
    expect(classifyReferences({ locationPaths: [], sourcePath: "/w/src/App.vue" }).verdict).toBe(
      "empty",
    );
  });
});
