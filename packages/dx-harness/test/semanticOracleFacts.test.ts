import { describe, expect, it } from "vitest";

import type {
  DiagnosticsResponse,
  ErrorResponse,
  ProviderCapabilities,
  QueryResponse,
  QueryResult,
} from "../src/baseline/bridgeClient.js";
import {
  ORACLE_FAMILIES,
  ORACLE_QUERY_METHODS,
  isOracleFamily,
  isOracleQueryMethod,
} from "../src/semantic-oracle/model.js";
import {
  bridgeCompletionFact,
  bridgeDefinitionFact,
  bridgeDiagnosticsFact,
  bridgeHoverFact,
  verterCompletionFact,
  verterDefinitionFact,
  verterHoverFact,
} from "../src/semantic-oracle/facts.js";
import { prepareOracleSource, requireOracleByteOffset } from "../src/semantic-oracle/prepare.js";
import { AnchorError } from "../src/anchors.js";

const CAPS: ProviderCapabilities = {
  provider: "tsgo",
  positionEncoding: "utf-8",
  diagnosticsPush: false,
  completionResolve: true,
};

function queryResponse(result: QueryResult): QueryResponse {
  return {
    type: "query",
    method: "hover",
    uri: "file:///o.ts",
    version: 1,
    result,
    capabilities: CAPS,
  };
}

describe("oracle model — closed families and live query methods", () => {
  it("the eight required scenario families are the closed set", () => {
    expect([...ORACLE_FAMILIES]).toEqual([
      "defineProps",
      "defineEmits",
      "defineModel",
      "slots",
      "templateRef",
      "fallthroughAttrs",
      "autoImportShape",
      "eventArgs",
    ]);
    expect(isOracleFamily("defineProps")).toBe(true);
    expect(isOracleFamily("nope")).toBe(false);
  });

  it("the live runner drives the three semantic query methods only", () => {
    expect([...ORACLE_QUERY_METHODS]).toEqual(["completion", "hover", "definition"]);
    expect(isOracleQueryMethod("hover")).toBe(true);
    expect(isOracleQueryMethod("diagnostics")).toBe(false);
  });
});

describe("prepareOracleSource — anchors become UTF-8 byte offsets (the bridge's space)", () => {
  it("resolves a trailing anchor to the START of its line's target identifier", () => {
    // The formatter relocates the anchor past the inserted `;`; resolution lands on
    // the line's last identifier (`title`) rather than the dead position after `;`.
    const prepared = prepareOracleSource(
      "declare const title: string\nconst read = props.title; // @dx-anchor t\n",
    );
    const offset = requireOracleByteOffset(prepared, "t");
    expect(prepared.stripped.slice(offset, offset + 5)).toBe("title");
  });

  it("a non-ASCII character before the anchor shifts the BYTE offset past the UTF-16 index", () => {
    // Line 0 holds `π` (1 UTF-16 unit, 2 UTF-8 bytes); the own-line anchor keeps its
    // raw line-1 start, so its byte offset (13) exceeds the UTF-16 char index (12).
    const { byteOffsets } = prepareOracleSource("const π = 1\n// @dx-anchor a\nconst y = 2\n");
    expect(byteOffsets.get("a")).toBe(13);
  });

  it("a missing oracle anchor throws a typed AnchorError, never a silent skip", () => {
    const prepared = prepareOracleSource("const x = 1\n");
    expect(() => requireOracleByteOffset(prepared, "ghost")).toThrow(AnchorError);
  });
});

describe("bridge fact extraction — typed refusals pass through, wrong kinds throw", () => {
  it("a completion query response folds to a BaselineCompletion ProviderResult", () => {
    const result = bridgeCompletionFact(
      queryResponse({ kind: "completion", items: [{ label: "title" }], isIncomplete: false }),
    );
    expect(result).toEqual({
      ok: true,
      output: { items: [{ label: "title" }], isIncomplete: false },
    });
  });

  it("a hover query response folds to a NormalizedHover ProviderResult", () => {
    const result = bridgeHoverFact(queryResponse({ kind: "hover", hover: { contents: "string" } }));
    expect(result).toEqual({ ok: true, output: { contents: "string" } });
  });

  it("a definition query response folds to a location ProviderResult", () => {
    const result = bridgeDefinitionFact(
      queryResponse({ kind: "definition", locations: [{ path: "/o.ts", start: 1, end: 2 }] }),
    );
    expect(result).toEqual({ ok: true, output: [{ path: "/o.ts", start: 1, end: 2 }] });
  });

  it("a diagnostics response folds to a diagnostic ProviderResult", () => {
    const response: DiagnosticsResponse = {
      type: "diagnostics",
      uri: "file:///o.ts",
      version: 1,
      diagnostics: [{ message: "m", severity: "error", start: 1, end: 2, code: "2304" }],
      capabilities: CAPS,
    };
    const result = bridgeDiagnosticsFact(response);
    expect(result).toEqual({
      ok: true,
      output: [{ message: "m", severity: "error", start: 1, end: 2, code: "2304" }],
    });
  });

  it("a typed bridge error becomes an ok:false ProviderResult, never a throw", () => {
    const error: ErrorResponse = { type: "error", kind: "provider_error", message: "crashed" };
    expect(bridgeHoverFact(error)).toEqual({ ok: false, error });
    expect(bridgeCompletionFact(error)).toEqual({ ok: false, error });
    expect(bridgeDefinitionFact(error)).toEqual({ ok: false, error });
    expect(bridgeDiagnosticsFact(error)).toEqual({ ok: false, error });
  });

  it("a method/result-kind mismatch throws — never a silent wrong-kind fold", () => {
    expect(() =>
      bridgeHoverFact(queryResponse({ kind: "completion", items: [], isIncomplete: false })),
    ).toThrow();
  });
});

describe("verter fact normalization reuses the shared normalize/ layer", () => {
  it("verterHoverFact strips and normalizes raw hover contents", () => {
    expect(verterHoverFact({ contents: "  string  " })).toEqual({ contents: "string" });
    expect(verterHoverFact(null)).toBeNull();
  });

  it("verterCompletionFact folds a raw list into a canonical set", () => {
    const fact = verterCompletionFact([{ label: "b" }, { label: "a" }]);
    expect(fact.items.map((i) => i.label)).toEqual(["a", "b"]);
    expect(fact.noSuggestionsCollapse).toBe(false);
  });

  it("verterDefinitionFact preserves a precise line-0 target", () => {
    const targets = verterDefinitionFact({
      uri: "file:///x.vue",
      range: { start: { line: 0, character: 6 }, end: { line: 0, character: 11 } },
    });
    expect(targets).toHaveLength(1);
    expect(targets[0].range.start.line).toBe(0);
  });
});
