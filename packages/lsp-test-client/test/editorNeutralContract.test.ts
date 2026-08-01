/**
 * @ai-generated - Verifies the typed, editor-neutral LSP contract inventory,
 * anchor resolution, and fail-closed result validation.
 */
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import {
  createEditorNeutralContractInventory,
  executeEditorNeutralContractCase,
  resolveContractAnchor,
  type EditorNeutralContractDriver,
  type LspDiagnostic,
} from "../src/index.js";

function fakeDriver(diagnostics: readonly LspDiagnostic[]): EditorNeutralContractDriver {
  return {
    route: "tsserver",
    async diagnostics() {
      return diagnostics;
    },
    async hover() {
      return { contents: { kind: "markdown", value: "```ts\nconst local: number\n```" } };
    },
    async definition() {
      return [];
    },
    async completion() {
      return { items: [{ label: "toFixed" }] };
    },
    async rename() {
      return { changes: {} };
    },
    async attestProvider() {
      return { route: "tsserver", publicKind: "tsserver", startedKinds: ["tsserver"] };
    },
    async attestTopology() {
      return { managedFallbackStarted: false, sharedRelayAlive: false };
    },
  };
}

function fixtureSource(document: string): string {
  const fixtureRoot = fileURLToPath(new URL("../fixtures/editor-neutral/", import.meta.url));
  return readFileSync(resolve(fixtureRoot, document), "utf8");
}

describe("editor-neutral LSP contract inventory", () => {
  it("is a non-vacuous, exact 92-case cross-framework inventory", () => {
    const inventory = createEditorNeutralContractInventory();
    expect(inventory).toHaveLength(92);
    expect(new Set(inventory.map((testCase) => testCase.id)).size).toBe(92);

    const standard = inventory.filter((testCase) => testCase.surface === "standard-lsp");
    const custom = inventory.filter((testCase) => testCase.surface === "verter-custom-protocol");
    const topology = inventory.filter((testCase) => testCase.surface === "provider-topology");
    expect(standard).toHaveLength(90);
    expect(custom).toHaveLength(1);
    expect(topology).toHaveLength(1);

    for (const framework of ["vue", "svelte"] as const) {
      for (const language of ["ts", "js"] as const) {
        const carrierCases = standard.filter(
          (testCase) => testCase.framework === framework && testCase.language === language,
        );
        for (const feature of [
          "diagnostics-clean",
          "diagnostics-error",
          "hover",
          "definition",
          "completion",
          "rename",
          "direct-import-hover",
          "direct-import-definition",
          "barrel-import-hover",
          "barrel-import-definition",
        ] as const) {
          expect(
            carrierCases.some((testCase) => testCase.feature === feature),
            `${framework}-${language} missing ${feature}`,
          ).toBe(true);
        }
      }
      const laxPrefix = `${framework}-js-dom-event-policy-lax.`;
      const laxCases = standard.filter((testCase) => testCase.id.startsWith(laxPrefix));
      expect(laxCases).toHaveLength(4);
      expect(laxCases.map((testCase) => testCase.feature).sort()).toEqual([
        "completion",
        "definition",
        "diagnostics-clean",
        "hover",
      ]);
      // D7: the jsconfig-configured lax family mirrors the tsconfig one.
      const laxJsconfigPrefix = `${framework}-js-dom-event-policy-lax-jsconfig.`;
      const laxJsconfigCases = standard.filter((testCase) =>
        testCase.id.startsWith(laxJsconfigPrefix),
      );
      expect(laxJsconfigCases).toHaveLength(4);
      expect(laxJsconfigCases.map((testCase) => testCase.feature).sort()).toEqual([
        "completion",
        "definition",
        "diagnostics-clean",
        "hover",
      ]);
    }
    expect(standard.filter((testCase) => testCase.feature === "consumer-diagnostics")).toHaveLength(
      1,
    );
    expect(
      standard.filter((testCase) => testCase.feature.startsWith("plain-control-")),
    ).toHaveLength(6);
    expect(
      standard.filter((testCase) => /^plain-[jt]sx-pointer-event\./.test(testCase.id)),
    ).toHaveLength(4);
    for (const testCase of standard.filter((candidate) =>
      candidate.feature.includes("definition"),
    )) {
      expect(testCase.expectedDefinitionDocument, `${testCase.id} target document`).toBeTruthy();
      expect(
        Number(testCase.expectedDefinitionAnchor !== undefined) +
          Number(testCase.expectedDefinitionRange !== undefined),
        `${testCase.id} must own exactly one exact declaration range`,
      ).toBe(1);
    }
    for (const testCase of standard.filter((candidate) => candidate.feature === "rename")) {
      expect(testCase.expectedRenameAnchors, `${testCase.id} exact rename anchors`).toHaveLength(2);
    }
    for (const framework of ["vue", "svelte"] as const) {
      for (const language of ["js", "ts"] as const) {
        const prefix = `${framework}-${language}-dom-event.`;
        const eventCases = standard.filter((testCase) => testCase.id.startsWith(prefix));
        expect(eventCases).toHaveLength(3);
        expect(eventCases.find((testCase) => testCase.feature === "hover")).toMatchObject({
          framework,
          language,
          requiredHoverFragments: ["PointerEvent"],
        });
        expect(
          eventCases.find((testCase) => testCase.id.endsWith("invalid-member-consumed")),
        ).toMatchObject({ feature: "diagnostics-clean" });
        if (language === "js") {
          expect(
            eventCases.find((testCase) => testCase.id.endsWith("non-inference-boundary")),
          ).toMatchObject({ feature: "diagnostics-clean" });
          expect(eventCases.find((testCase) => testCase.feature === "hover")?.id).toContain(
            ".jsdoc.",
          );
        }
      }
    }
    expect(
      standard.find((testCase) => testCase.id === "svelte-ts-state-string.diagnostics.clean"),
    ).toMatchObject({
      framework: "svelte",
      language: "ts",
      feature: "diagnostics-clean",
      providers: ["tsserver", "tsgo", "shared-tsgo"],
    });
    expect(
      standard.filter((testCase) => /^svelte-classic-[jt]s-dom-event\./.test(testCase.id)),
    ).toEqual([
      expect.objectContaining({ language: "js", feature: "diagnostics-clean" }),
      expect.objectContaining({ language: "ts", feature: "diagnostics-clean" }),
    ]);
  });
});

describe("editor-neutral anchor resolution", () => {
  it("requires an exact occurrence and resolves through negotiated positions", () => {
    const source = "const local = 1\nlocal.toFixed()\n";
    expect(
      resolveContractAnchor(source, { text: "local.toFixed", occurrence: 0, token: "local" }),
    ).toEqual({ line: 1, character: 0 });
    expect(() =>
      resolveContractAnchor(source, { text: "missing", occurrence: 0, token: "missing" }),
    ).toThrow(/anchor text.*missing/i);
    expect(() => resolveContractAnchor(source, { text: "local", token: "local" })).toThrow(
      /occurrence/i,
    );
    expect(() =>
      resolveContractAnchor("return event", {
        text: "return event",
        occurrence: 0,
        token: "e",
      }),
    ).toThrow(/anchor token.*ambiguous/i);
  });

  it("resolves every committed inventory anchor unambiguously", () => {
    const fixtureRoot = fileURLToPath(new URL("../fixtures/editor-neutral/", import.meta.url));
    for (const testCase of createEditorNeutralContractInventory()) {
      if (testCase.anchor && testCase.document) {
        const source = readFileSync(resolve(fixtureRoot, testCase.document), "utf8");
        expect(() => resolveContractAnchor(source, testCase.anchor!), testCase.id).not.toThrow();
      }
      if (testCase.expectedDefinitionAnchor && testCase.expectedDefinitionDocument) {
        const source = readFileSync(
          resolve(fixtureRoot, testCase.expectedDefinitionDocument),
          "utf8",
        );
        expect(
          () => resolveContractAnchor(source, testCase.expectedDefinitionAnchor!),
          `${testCase.id} definition`,
        ).not.toThrow();
      }
      if (testCase.expectedRenameAnchors) {
        const source = readFileSync(resolve(fixtureRoot, testCase.document), "utf8");
        for (const anchor of testCase.expectedRenameAnchors) {
          expect(
            () => resolveContractAnchor(source, anchor),
            `${testCase.id} rename`,
          ).not.toThrow();
        }
      }
    }
  });
});

describe("editor-neutral fail-closed validation", () => {
  it("rejects TS7026 instead of accepting a nominally clean diagnostic response", async () => {
    const cleanCase = createEditorNeutralContractInventory().find(
      (testCase) => testCase.id === "vue-ts.diagnostics.clean",
    );
    expect(cleanCase).toBeDefined();
    await expect(
      executeEditorNeutralContractCase(
        cleanCase!,
        fakeDriver([
          {
            range: {
              start: { line: 7, character: 2 },
              end: { line: 7, character: 8 },
            },
            message: "JSX element implicitly has type 'any'",
            code: 7026,
            severity: 1,
            source: "ts",
          },
        ]),
        new Map([[cleanCase!.document, "<template><div /></template>"]]),
      ),
    ).rejects.toThrow(/TS7026/);
  });

  it("rejects tsserver serving that carries no tsgo recommendation (tsgo-preferred flip)", async () => {
    const attestationCase = createEditorNeutralContractInventory().find(
      (testCase) => testCase.id === "verter.provider-attestation",
    );
    expect(attestationCase).toBeDefined();
    // The default fake driver attests tsserver WITHOUT a recommendation.
    await expect(
      executeEditorNeutralContractCase(attestationCase!, fakeDriver([]), new Map()),
    ).rejects.toThrow(/must recommend tsgo/i);
  });

  it("rejects a recommendation whose wording is not editor-agnostic", async () => {
    const attestationCase = createEditorNeutralContractInventory().find(
      (testCase) => testCase.id === "verter.provider-attestation",
    );
    const driver = fakeDriver([]);
    driver.attestProvider = async () => ({
      route: "tsserver",
      publicKind: "tsserver",
      startedKinds: ["tsserver"],
      recommendation: {
        preferred: "tsgo",
        reason: 'Set verter.typeProvider to "tsgo" in VS Code settings.',
        knownGaps: ["TS6133 quick fix unported."],
      },
    });
    await expect(
      executeEditorNeutralContractCase(attestationCase!, driver, new Map()),
    ).rejects.toThrow(/editor-agnostic/i);
  });

  it("rejects tsgo-family serving that nags with a recommendation", async () => {
    const attestationCase = createEditorNeutralContractInventory().find(
      (testCase) => testCase.id === "verter.provider-attestation",
    );
    const driver = fakeDriver([]);
    driver.route = "tsgo";
    driver.attestProvider = async () => ({
      route: "tsgo",
      publicKind: "tsgo",
      startedKinds: ["tsgo"],
      recommendation: {
        preferred: "tsgo",
        reason: "TSGO is recommended.",
        knownGaps: [],
      },
    });
    await expect(
      executeEditorNeutralContractCase(attestationCase!, driver, new Map()),
    ).rejects.toThrow(/no recommendation/i);
  });

  it("accepts tsserver serving that carries the honest tsgo recommendation", async () => {
    const attestationCase = createEditorNeutralContractInventory().find(
      (testCase) => testCase.id === "verter.provider-attestation",
    );
    const driver = fakeDriver([]);
    driver.attestProvider = async () => ({
      route: "tsserver",
      publicKind: "tsserver",
      startedKinds: ["tsserver"],
      recommendation: {
        preferred: "tsgo",
        reason:
          "This workspace is served by a tsserver-family TypeScript service. TSGO is recommended.",
        knownGaps: ["The 'remove unused declaration' quick fix (TS6133) is not yet available."],
      },
    });
    await expect(
      executeEditorNeutralContractCase(attestationCase!, driver, new Map()),
    ).resolves.toBeUndefined();
  });

  it("rejects a shared-tsgo topology that activated managed fallback", async () => {
    const topologyCase = createEditorNeutralContractInventory().find(
      (testCase) => testCase.surface === "provider-topology",
    );
    expect(topologyCase).toBeDefined();
    const driver = fakeDriver([]);
    driver.route = "shared-tsgo";
    driver.attestTopology = async () => ({
      managedFallbackStarted: true,
      sharedRelayAlive: true,
    });
    await expect(
      executeEditorNeutralContractCase(topologyCase!, driver, new Map()),
    ).rejects.toThrow(/managed fallback/i);
  });

  it("rejects a definition that reaches the right document at the wrong declaration range", async () => {
    const testCase = createEditorNeutralContractInventory().find(
      (candidate) => candidate.id === "plain-typescript.definition",
    );
    expect(testCase).toBeDefined();
    const source = fixtureSource(testCase!.document);
    const driver = fakeDriver([]);
    driver.definition = async () => ({
      uri: "file:///workspace/src/plain-control.ts",
      range: {
        start: { line: 1, character: 34 },
        end: { line: 1, character: 52 },
      },
    });

    await expect(
      executeEditorNeutralContractCase(testCase!, driver, new Map([[testCase!.document, source]])),
    ).rejects.toThrow(/declaration range/i);
  });

  it("requires the same exact local declaration on first and repeated requests", async () => {
    const testCase = createEditorNeutralContractInventory().find(
      (candidate) => candidate.id === "vue-ts.markup.definition",
    );
    expect(testCase).toBeDefined();
    const source = fixtureSource(testCase!.document);
    const driver = fakeDriver([]);
    let requests = 0;
    driver.definition = async () => {
      requests += 1;
      return {
        uri: "file:///workspace/src/vue/TypeScriptCase.vue",
        range: {
          start: { line: 6, character: 6 },
          end: { line: 6, character: 16 },
        },
      };
    };

    const evidence = await executeEditorNeutralContractCase(
      testCase!,
      driver,
      new Map([[testCase!.document, source]]),
    );
    expect(requests).toBe(2);
    expect(evidence?.localDefinitionDurationsMs).toEqual([expect.any(Number), expect.any(Number)]);
  });

  it("still exercises and records the repeated definition when the first target is wrong", async () => {
    const testCase = createEditorNeutralContractInventory().find(
      (candidate) => candidate.id === "vue-ts.markup.definition",
    );
    expect(testCase).toBeDefined();
    const source = fixtureSource(testCase!.document);
    const driver = fakeDriver([]);
    let requests = 0;
    driver.definition = async () => {
      requests += 1;
      return {
        uri: "file:///workspace/src/vue/TypeScriptCase.vue",
        range:
          requests === 1
            ? {
                start: { line: 0, character: 0 },
                end: { line: 0, character: 0 },
              }
            : {
                start: { line: 6, character: 6 },
                end: { line: 6, character: 16 },
              },
      };
    };

    await expect(
      executeEditorNeutralContractCase(testCase!, driver, new Map([[testCase!.document, source]])),
    ).rejects.toMatchObject({
      message: expect.stringMatching(/first.*wrong declaration range/i),
      evidence: {
        localDefinitionDurationsMs: [expect.any(Number), expect.any(Number)],
      },
    });
    expect(requests).toBe(2);
  });

  it("rejects rename edits whose replacement is not the requested new name", async () => {
    const testCase = createEditorNeutralContractInventory().find(
      (candidate) => candidate.id === "vue-ts.markup.rename",
    );
    expect(testCase).toBeDefined();
    const source = fixtureSource(testCase!.document);
    const driver = fakeDriver([]);
    driver.rename = async () => ({
      changes: {
        "file:///workspace/src/vue/TypeScriptCase.vue": [
          {
            newText: "wrong_name",
            range: {
              start: { line: 6, character: 6 },
              end: { line: 6, character: 16 },
            },
          },
          {
            newText: "wrong_name",
            range: {
              start: { line: 10, character: 28 },
              end: { line: 10, character: 38 },
            },
          },
        ],
      },
    });

    await expect(
      executeEditorNeutralContractCase(testCase!, driver, new Map([[testCase!.document, source]])),
    ).rejects.toThrow(/newText/i);
  });

  it("rejects rename edits that do not replace the original source token", async () => {
    const testCase = createEditorNeutralContractInventory().find(
      (candidate) => candidate.id === "vue-ts.markup.rename",
    );
    expect(testCase).toBeDefined();
    const source = fixtureSource(testCase!.document);
    const driver = fakeDriver([]);
    driver.rename = async () => ({
      changes: {
        "file:///workspace/src/vue/TypeScriptCase.vue": [
          {
            newText: testCase!.renameTo!,
            range: {
              start: { line: 6, character: 0 },
              end: { line: 6, character: 5 },
            },
          },
          {
            newText: testCase!.renameTo!,
            range: {
              start: { line: 10, character: 28 },
              end: { line: 10, character: 38 },
            },
          },
        ],
      },
    });

    await expect(
      executeEditorNeutralContractCase(testCase!, driver, new Map([[testCase!.document, source]])),
    ).rejects.toThrow(/original token/i);
  });
});

/**
 * Hover strings CAPTURED VERBATIM from the three real provider routes against the
 * committed `editor-neutral` fixture. They are the evidence the hover assertions
 * are tuned against: a synthetic string could be written to make any predicate
 * look correct, so the discrimination below is proven on what the system actually
 * emits.
 */
const UNTYPED_EMIT_FALLBACK_TEXT = "$emit: (event: string, ...args: unknown[]) => void";

const CAPTURED = {
  /**
   * `vue-js.direct-import.hover`, tsserver — a CORRECT Vue component hover. It
   * contains `any` only as one of `DefineComponent`'s own trailing default type
   * ARGUMENTS (`…, ComponentProvideOptions, true, {}, any>>`), never as an
   * annotation. Every route emits this same framework tail.
   */
  correctVueComponent:
    '```typescript\n(const) const JavaScriptCase: __OmitNew<DefineComponent<ExtractPropTypes<{\n    label: {\n        type: StringConstructor;\n        required: true;\n    };\n}>, {}, {}, {}, {}, ComponentOptionsMixin, ComponentOptionsMixin, {}, string, PublicProps, ToResolvedProps<ExtractPropTypes<{\n    label: {\n        type: StringConstructor;\n        required: true;\n    };\n}>, {}>, {}, {}, {}, {}, string, ComponentProvideOptions, true, {}, any>> & (new (props?: import("vue").PublicProps & {\n    label: string;\n}) => {\n    $props: import("vue").PublicProps & {\n        label: string;\n    };\n    $emit: (event: string, ...args: unknown[]) => void;\n    $data: {};\n    $attrs: import("vue").HTMLAttributes;\n    $refs: {};\n})\n```',
  /**
   * `vue-ts.markup.hover`, shared-tsgo — a REAL, currently-live degradation: the
   * script local resolves to `any` on that route while tsserver and tsgo both
   * resolve it correctly. This is the failure the predicate must keep catching.
   */
  degradedLocal: "```typescript\nconst vueTsLocal: any\n```",
  /** `vue-ts.markup.hover`, tsserver — the correct answer for a TS `const 1`. */
  correctLocal: "```typescript\n(const) const vueTsLocal: 1\n```",
} as const;

function hoverDriver(value: string): EditorNeutralContractDriver {
  const driver = fakeDriver([]);
  driver.hover = async () => (value === "" ? null : { contents: { kind: "markdown", value } });
  return driver;
}

async function runHoverCase(id: string, hover: string): Promise<Error | null> {
  const testCase = createEditorNeutralContractInventory().find((candidate) => candidate.id === id);
  expect(testCase, `inventory must contain ${id}`).toBeDefined();
  const sources = new Map([[testCase!.document, fixtureSource(testCase!.document)]]);
  try {
    await executeEditorNeutralContractCase(testCase!, hoverDriver(hover), sources);
    return null;
  } catch (error) {
    return error as Error;
  }
}

describe("editor-neutral hover assertions discriminate on real provider output", () => {
  it("accepts the correct TS-const local and rejects every degraded or wrong answer", async () => {
    // The correct answer passes. `const vueTsLocal = 1` in TypeScript keeps the
    // literal type, so this — not `number` — is what a healthy route emits.
    expect(await runHoverCase("vue-ts.markup.hover", CAPTURED.correctLocal)).toBeNull();

    // An EMPTY hover fails: the case's whole purpose is that the local resolves.
    expect((await runHoverCase("vue-ts.markup.hover", ""))?.message).toMatch(/hover was empty/);

    // The REAL shared-tsgo degradation fails. This is the property the repair had
    // to preserve: a passing predicate must not be reachable by `any`.
    expect((await runHoverCase("vue-ts.markup.hover", CAPTURED.degradedLocal))?.message).toMatch(
      /any/i,
    );

    // A hover that resolved a DIFFERENT symbol fails — the teeth the previous
    // `["number"]` fragment lacked, since `toFixed`'s own signature mentions
    // `number` and would have satisfied it while proving nothing about the local.
    expect(
      (
        await runHoverCase(
          "vue-ts.markup.hover",
          "```typescript\n(method) Number.toFixed(fractionDigits?: number): string\n```",
        )
      )?.message,
    ).toMatch(/missing "vueTsLocal: 1"/);
  });

  it("holds the same discrimination for the plain-TypeScript control", async () => {
    expect(
      await runHoverCase(
        "plain-typescript.hover",
        "```typescript\n(const) const plainControlNumber: 1\n```",
      ),
    ).toBeNull();
    expect((await runHoverCase("plain-typescript.hover", ""))?.message).toMatch(/hover was empty/);
    expect(
      (
        await runHoverCase(
          "plain-typescript.hover",
          "```typescript\n(const) const plainControlNumber: any\n```",
        )
      )?.message,
    ).toMatch(/any/i);
    // A `number`-mentioning hover on the wrong symbol no longer satisfies it.
    expect(
      (
        await runHoverCase(
          "plain-typescript.hover",
          "```typescript\n(method) Number.toFixed(fractionDigits?: number): string\n```",
        )
      )?.message,
    ).toMatch(/missing "plainControlNumber: 1"/);
  });

  it("passes a correct Vue component hover while still catching an annotation `any`", async () => {
    // A CORRECT component hover carries Vue's own `…, {}, any>>` default type
    // arguments. The bare `/\bany\b/` this replaced fired here, so the case could
    // never pass and asserted nothing about Verter.
    expect(CAPTURED.correctVueComponent).toMatch(/\bany\b/);
    expect(
      await runHoverCase("vue-js.direct-import.hover", CAPTURED.correctVueComponent),
    ).toBeNull();
    expect(
      await runHoverCase("vue-js.barrel-import.hover", CAPTURED.correctVueComponent),
    ).toBeNull();

    // A degraded component binding — `any` in ANNOTATION position — is still
    // rejected. This is the same real shape as the shared-tsgo local degradation.
    const degradedComponent = "```typescript\n(const) const JavaScriptCase: any\n```";
    expect((await runHoverCase("vue-js.direct-import.hover", degradedComponent))?.message).toMatch(
      /any/i,
    );

    // A degraded PROP inside an otherwise well-formed component type is rejected
    // too — the annotation rule reaches inside the printed object surface.
    const degradedProp = CAPTURED.correctVueComponent.replace("label: string;", "label: any;");
    expect(degradedProp).not.toBe(CAPTURED.correctVueComponent);
    expect((await runHoverCase("vue-js.direct-import.hover", degradedProp))?.message).toMatch(
      /any/i,
    );

    // An empty component hover still fails.
    expect((await runHoverCase("vue-js.direct-import.hover", ""))?.message).toMatch(
      /hover was empty/,
    );
  });

  it("accepts Verter's untyped-emit `unknown[]` fallback but not an `unknown` value type", async () => {
    // The SAME defect as the bare `any`: a component with no `defineEmits` gets
    // Verter's deliberate `$emit: (event: string, ...args: unknown[]) => void`,
    // so a bare `/\bunknown\b/` fired on every correct hover. It is present in the
    // captured output, which is why the predicate had to be narrowed too.
    expect(CAPTURED.correctVueComponent).toMatch(/\bunknown\b/);
    expect(CAPTURED.correctVueComponent).toContain("...args: unknown[]");
    // The load-bearing half: a hover carrying ONLY that fallback `unknown` passes.
    expect(
      await runHoverCase("vue-js.direct-import.hover", CAPTURED.correctVueComponent),
    ).toBeNull();

    // A prop degraded to the top type IS still caught: the exemption covers the
    // $emit fallback SIGNATURE, nothing else in the printed type.
    const degradedProp = CAPTURED.correctVueComponent.replace("label: string;", "label: unknown;");
    expect(degradedProp).not.toBe(CAPTURED.correctVueComponent);
    expect((await runHoverCase("vue-js.direct-import.hover", degradedProp))?.message).toMatch(
      /unknown/i,
    );

    // …and so is a component binding degraded to `unknown`.
    expect(
      (
        await runHoverCase(
          "vue-js.direct-import.hover",
          "```typescript\n(const) const JavaScriptCase: unknown\n```",
        )
      )?.message,
    ).toMatch(/unknown/i);
  });

  // Every degraded props shape a prose-shape predicate lets through. Each keeps the
  // required name `label`, so the required half cannot catch them: the forbidden
  // half is the only guard, and it must reject all of them.
  const DEGRADED_PROP_TYPES = [
    // What a predicate exempting the `unknown[]` TYPE FORM admits.
    "unknown[]",
    // What a predicate exempting `...ident:` admits — the `unknown` sits behind a
    // generic, so no annotation-position rule sees it.
    "Array<unknown>",
    // What a rest-parameter-shaped exemption admits: a PROP whose type is itself a
    // function with a rest parameter. Structurally identical to the contractual
    // emit fallback, but on the wrong member.
    "(...args: unknown[]) => void",
    // The plain top type, for completeness.
    "unknown",
  ] as const;

  for (const degraded of DEGRADED_PROP_TYPES) {
    it(`rejects a prop degraded to \`${degraded}\``, async () => {
      const hover = CAPTURED.correctVueComponent.replace("label: string;", `label: ${degraded};`);
      expect(hover, "the mutation must actually apply").not.toBe(CAPTURED.correctVueComponent);
      // The required half cannot save us: the prop NAME survives.
      expect(hover).toContain("label");
      // The CONTRACTUAL emit fallback is still present, so the exemption is
      // genuinely exercised rather than removed by the mutation.
      expect(hover).toContain("...args: unknown[]");
      expect((await runHoverCase("vue-js.direct-import.hover", hover))?.message).toMatch(
        /unknown/i,
      );
    });
  }

  // `$emit`-SHAPED text that is NOT the exact contractual member. An exemption
  // expressed as a pattern rather than exact text swallows each of these — deleting
  // the only `unknown` before the strict rule runs — so the degraded hover greens.
  // Each keeps the required `label`, so the required half cannot catch them either.
  const EMIT_SHAPED_DEGRADATIONS = [
    {
      name: "a degraded emit parameter",
      // `$emit:\s*\([^)]*\)\s*=>\s*void` matches this whole member.
      emit: "$emit: (event: unknown) => void",
    },
    {
      name: "an emit-shaped member nested inside a degraded prop",
      emit: "$emit: (event: string, ...args: unknown[]) => void",
      prop: "label: { $emit: (event: unknown) => void }",
    },
    {
      name: "a nested callback whose paren truncates a pattern match",
      // `[^)]*` stops at the INNER paren, so a pattern match covers the leading
      // half and carries the only `unknown` away with it.
      emit: "$emit: (event: string, cb: (value: unknown) => void) => void",
    },
    {
      name: "a second, degraded emit member beside the valid one",
      // A global strip removes BOTH; first-occurrence-only removal leaves this one.
      emit: "$emit: (event: string, ...args: unknown[]) => void;\n    $emit: (event: unknown) => void",
    },
  ] as const;

  for (const shape of EMIT_SHAPED_DEGRADATIONS) {
    it(`rejects ${shape.name}`, async () => {
      let hover = CAPTURED.correctVueComponent.replace(
        "$emit: (event: string, ...args: unknown[]) => void",
        shape.emit,
      );
      if ("prop" in shape && shape.prop) {
        hover = hover.replace("label: string;", `${shape.prop};`);
      }
      expect(hover, "the mutation must actually apply").not.toBe(CAPTURED.correctVueComponent);
      // The required half cannot save us: the prop NAME survives.
      expect(hover).toContain("label");
      const error = await runHoverCase("vue-js.direct-import.hover", hover);
      expect(error, `${shape.name} must not be accepted`).not.toBeNull();
      expect(error?.message).toMatch(/unknown/i);
    });
  }

  it("rejects the contractual bytes appearing at a NESTED structural position", async () => {
    // Exact bytes are not sufficient on their own: a substring match removes them
    // wherever they appear. Here the contractual member sits inside the `label`
    // PROP type — one brace deeper than its real position — while the actual
    // `$emit` member carries no `unknown` at all. Excising by text alone deletes
    // the only evidence and the degraded props surface greens.
    const hover = CAPTURED.correctVueComponent
      .replace("label: string;", `label: { ${UNTYPED_EMIT_FALLBACK_TEXT} };`)
      .replace(UNTYPED_EMIT_FALLBACK_TEXT + ";", "$emit: (event: string) => void;");
    expect(hover, "the mutation must actually apply").not.toBe(CAPTURED.correctVueComponent);
    // Exactly one occurrence remains, and it is the NESTED one.
    expect(hover.split(UNTYPED_EMIT_FALLBACK_TEXT)).toHaveLength(2);
    expect(hover).toContain(`label: { ${UNTYPED_EMIT_FALLBACK_TEXT} }`);
    // The required half cannot save us: the prop NAME survives.
    expect(hover).toContain("label");

    const error = await runHoverCase("vue-js.direct-import.hover", hover);
    expect(error, "contractual bytes at a nested position must not be excised").not.toBeNull();
    expect(error?.message).toMatch(/unknown/i);
  });

  // Peerhood is same-depth AND same-body. These inputs isolate the difference: in
  // each, the contractual bytes sit at the SAME brace depth as a `$props:` member
  // but inside a DIFFERENT object body, and they carry the hover's only `unknown`.
  // A depth-only rule excises them and the hover greens.
  const EQUAL_DEPTH_DIFFERENT_BODY = [
    {
      name: "an intersection whose second body holds the member",
      type: `{ $props: { label: string } } & { label: string; ${UNTYPED_EMIT_FALLBACK_TEXT} }`,
    },
    {
      name: "a union whose second body holds the member",
      type: `{ $props: { label: string } } | { label: string; ${UNTYPED_EMIT_FALLBACK_TEXT} }`,
    },
  ] as const;

  for (const shape of EQUAL_DEPTH_DIFFERENT_BODY) {
    it(`rejects the contractual bytes in ${shape.name}`, async () => {
      const hover = `\`\`\`typescript\n(const) const JavaScriptCase: ${shape.type}\n\`\`\``;
      expect(hover).toContain("label");
      const error = await runHoverCase("vue-js.direct-import.hover", hover);
      expect(error, "equal depth in a different body is not peerhood").not.toBeNull();
      expect(error?.message).toMatch(/unknown/i);
    });
  }

  it("still excises the member when it is a true peer of $props", async () => {
    // The positive control for the two rejections above: without it, a rule that
    // rejected everything would look equally correct.
    const hover =
      "```typescript\n(const) const JavaScriptCase: " +
      `{ $props: { label: string }; ${UNTYPED_EMIT_FALLBACK_TEXT}; $data: {} }\n\`\`\``;
    expect(await runHoverCase("vue-js.direct-import.hover", hover)).toBeNull();
  });

  it("rejects a string-literal `$props:` posing as a peer sibling", async () => {
    // `sep: "$props:"` is valid TypeScript that QuickInfo prints verbatim. Collecting
    // sibling candidates by raw substring treats it as a depth-1 member, manufactures
    // peerhood for the `$emit` beside it, and removes the hover's only `unknown` —
    // while the GENUINE `$props` sits one brace deeper inside `nested`.
    const hover =
      "```typescript\n(const) const JavaScriptCase: " +
      "{ nested: { $props: { label: string } } } & " +
      `{ label: string; sep: "$props:"; ${UNTYPED_EMIT_FALLBACK_TEXT} }\n\`\`\``;
    expect(hover).toContain('sep: "$props:"');
    // The real sibling is nested, so no legitimate peer exists at the member's depth.
    expect(hover).toContain("{ nested: { $props:");

    const error = await runHoverCase("vue-js.direct-import.hover", hover);
    expect(error, "a quoted `$props:` must not authorise excision").not.toBeNull();
    expect(error?.message).toMatch(/unknown/i);
  });

  it("rejects a TEMPLATE-LITERAL `$props:` posing as a peer sibling", async () => {
    // The double-quoted impostor's twin. It used to slip through because backticks
    // were excluded from literal tracking to avoid confusing them with the markdown
    // fence — an ambiguity that only existed while the whole markdown value was
    // analysed. Confined to fence CONTENT, a backtick can only be a template literal.
    const hover =
      "```typescript\n(const) const JavaScriptCase: " +
      "{ nested: { $props: { label: string } } } & " +
      `{ label: string; sep: \`$props:\`; ${UNTYPED_EMIT_FALLBACK_TEXT} }\n\`\`\``;
    expect(hover).toContain("sep: `$props:`");

    const error = await runHoverCase("vue-js.direct-import.hover", hover);
    expect(error, "a template-literal `$props:` must not authorise excision").not.toBeNull();
    expect(error?.message).toMatch(/unknown/i);
  });

  it("accepts a correct hover carrying a template-literal type with a brace", async () => {
    // The retired FALSE-REJECT residual: `sep: \`}\`` used to have its brace counted,
    // shifting depths so a true peer no longer looked like one and the exemption was
    // missed. Asserting acceptance is what proves the hole is closed rather than
    // merely re-routed — a rule that rejected everything would satisfy the impostor
    // tests above but fail here.
    const hover =
      "```typescript\n(const) const JavaScriptCase: " +
      `{ $props: { label: string }; sep: \`}\`; ${UNTYPED_EMIT_FALLBACK_TEXT}; $data: {} }\n\`\`\``;
    expect(hover).toContain("sep: `}`");
    expect(await runHoverCase("vue-js.direct-import.hover", hover)).toBeNull();
  });

  it("ignores prose outside the fenced code block", async () => {
    // JSDoc follows the type in a real hover. Prose is not TypeScript: an apostrophe
    // is not a string delimiter and a documented `$props:` is not a member. Both
    // directions are asserted — a good hover is not corrupted by prose, and prose
    // cannot supply the sibling that authorises an excision.
    const good =
      "```typescript\n(const) const JavaScriptCase: " +
      `{ $props: { label: string }; ${UNTYPED_EMIT_FALLBACK_TEXT}; $data: {} }\n\`\`\`` +
      "\n\nDoesn't emit typed events; it won't narrow. See the `label` prop.";
    expect(await runHoverCase("vue-js.direct-import.hover", good)).toBeNull();

    const proseSibling =
      "```typescript\n(const) const JavaScriptCase: " +
      "{ nested: { $props: { label: string } } } & " +
      `{ label: string; ${UNTYPED_EMIT_FALLBACK_TEXT} }\n\`\`\`` +
      "\n\nThe $props: member is documented here.";
    const error = await runHoverCase("vue-js.direct-import.hover", proseSibling);
    expect(error, "a prose mention is not a member").not.toBeNull();
    expect(error?.message).toMatch(/unknown/i);
  });

  it("does not excise contractual bytes quoted inside a string literal", async () => {
    // The symmetric case: the member itself appears only inside a literal, so there
    // is no real member to exempt and the degraded `label` must still be caught.
    const hover =
      "```typescript\n(const) const JavaScriptCase: " +
      `{ $props: { label: string }; doc: "${UNTYPED_EMIT_FALLBACK_TEXT}"; label: unknown }\n\`\`\``;
    const error = await runHoverCase("vue-js.direct-import.hover", hover);
    expect(error, "quoted contractual bytes are not a member").not.toBeNull();
    expect(error?.message).toMatch(/unknown/i);
    // Both implementations REJECT this (the degraded `label: unknown` trips either
    // way), so the outcome alone proves nothing. The discriminator is WHETHER the
    // exemption fired: treating quoted bytes as a member excises them and reports no
    // unapplied exemption, while the filter leaves it unapplied and says so.
    expect(
      error?.message,
      "the quoted occurrence must not have been consumed as the exemption",
    ).toMatch(/declared exemption\(s\) not excised/);
  });

  it("does not let a brace inside a STRING LITERAL type equalise depths", async () => {
    // `sep: "}"` is ordinary TypeScript that QuickInfo prints verbatim, so this is
    // reachable from real output rather than adversarial. Counting that brace drops
    // the running depth and makes the NESTED member look like a top-level peer,
    // licensing an excision that hides the degradation.
    const hover =
      "```typescript\n(const) const JavaScriptCase: " +
      `{ $props: { label: string }; sep: "}"; label: { ${UNTYPED_EMIT_FALLBACK_TEXT} } }\n\`\`\``;
    expect(hover).toContain('sep: "}"');
    const error = await runHoverCase("vue-js.direct-import.hover", hover);
    expect(error, "a brace inside a string literal must not affect depth").not.toBeNull();
    expect(error?.message).toMatch(/unknown/i);
  });

  it("strips nothing when the contractual member is not present verbatim", async () => {
    // Exactness is the guard, so a re-rendered fallback is NOT excised and trips the
    // strict rule. That is deliberate: a loud, fixable false-reject beats a silently
    // widened exemption. The failure must SAY so, naming the constant to update.
    const reRendered = CAPTURED.correctVueComponent.replace(
      "$emit: (event: string, ...args: unknown[]) => void",
      "$emit: (event: string, ...payload: unknown[]) => void",
    );
    expect(reRendered).not.toBe(CAPTURED.correctVueComponent);
    const error = await runHoverCase("vue-js.direct-import.hover", reRendered);
    expect(error, "a non-verbatim rendering must fail closed, not be excised").not.toBeNull();
    expect(
      error?.message,
      "the failure must name the unmatched exemption so it is fixable",
    ).toMatch(/declared exemption\(s\) not excised/);
  });

  it("rejects Verter's positional wildcard erasure through the REQUIRED half", async () => {
    // Degradation is not always annotation-position: Verter's own wildcard shape is
    // positional (`DefineComponent<{}, {}, any>`, crates/verter_tsc/src/checker.rs),
    // which is indistinguishable BY POSITION from the framework's own trailing
    // default type arguments. It is caught by the other half of the same case —
    // erasing the props surface erases the prop NAMES the case requires — so this
    // pins WHICH half does the work rather than leaving the guard overclaimed.
    const wildcardErasure =
      "```typescript\n(const) const JavaScriptCase: DefineComponent<{}, {}, any>\n```";
    const error = await runHoverCase("vue-js.direct-import.hover", wildcardErasure);
    expect(error, "positional wildcard erasure must not be accepted").not.toBeNull();
    expect(error?.message).toMatch(/missing "label"/);
  });
});
