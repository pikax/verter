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

describe("editor-neutral LSP contract inventory", () => {
  it("is a non-vacuous, exact 73-case cross-framework inventory", () => {
    const inventory = createEditorNeutralContractInventory();
    expect(inventory).toHaveLength(73);
    expect(new Set(inventory.map((testCase) => testCase.id)).size).toBe(73);

    const standard = inventory.filter((testCase) => testCase.surface === "standard-lsp");
    const custom = inventory.filter((testCase) => testCase.surface === "verter-custom-protocol");
    const topology = inventory.filter((testCase) => testCase.surface === "provider-topology");
    expect(standard).toHaveLength(71);
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
    }
    expect(standard.filter((testCase) => testCase.feature === "consumer-diagnostics")).toHaveLength(
      1,
    );
    expect(
      standard.filter((testCase) => testCase.feature.startsWith("plain-control-")),
    ).toHaveLength(5);
    expect(
      standard.filter((testCase) => /^plain-[jt]sx-pointer-event\./.test(testCase.id)),
    ).toHaveLength(4);
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
      if (!testCase.anchor || !testCase.document) continue;
      const source = readFileSync(resolve(fixtureRoot, testCase.document), "utf8");
      expect(() => resolveContractAnchor(source, testCase.anchor!), testCase.id).not.toThrow();
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
});
