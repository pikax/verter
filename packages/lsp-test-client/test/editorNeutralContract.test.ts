/**
 * @ai-generated - Verifies the typed, editor-neutral LSP contract inventory,
 * anchor resolution, and fail-closed result validation.
 */
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
  it("is a non-vacuous, exact 43-case cross-framework inventory", () => {
    const inventory = createEditorNeutralContractInventory();
    expect(inventory).toHaveLength(43);
    expect(new Set(inventory.map((testCase) => testCase.id)).size).toBe(43);

    const standard = inventory.filter((testCase) => testCase.surface === "standard-lsp");
    const custom = inventory.filter((testCase) => testCase.surface === "verter-custom-protocol");
    const topology = inventory.filter((testCase) => testCase.surface === "provider-topology");
    expect(standard).toHaveLength(41);
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
    }
    expect(standard.filter((testCase) => testCase.feature === "consumer-diagnostics")).toHaveLength(
      1,
    );
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
