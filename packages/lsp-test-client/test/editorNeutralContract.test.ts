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
  it("is a non-vacuous, exact 82-case cross-framework inventory", () => {
    const inventory = createEditorNeutralContractInventory();
    expect(inventory).toHaveLength(82);
    expect(new Set(inventory.map((testCase) => testCase.id)).size).toBe(82);

    const standard = inventory.filter((testCase) => testCase.surface === "standard-lsp");
    const custom = inventory.filter((testCase) => testCase.surface === "verter-custom-protocol");
    const topology = inventory.filter((testCase) => testCase.surface === "provider-topology");
    expect(standard).toHaveLength(80);
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
