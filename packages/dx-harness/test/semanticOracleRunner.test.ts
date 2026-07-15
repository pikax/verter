import { describe, expect, it } from "vitest";

import { addFileAnchors, stripAnchors, type AnchorMap } from "../src/anchors.js";
import type {
  ErrorResponse,
  ProviderCapabilities,
  QueryInput,
  QueryResponse,
  QueryResult,
} from "../src/baseline/bridgeClient.js";
import { prepareOracleSource } from "../src/semantic-oracle/prepare.js";
import {
  OracleError,
  resolveLspPosition,
  runResolvedOracleQuery,
  runSemanticOracle,
  type OracleSourceContext,
  type OracleVerterClient,
  type OracleBridgeClient,
  type ResolvedOracleQuery,
} from "../src/semantic-oracle/runner.js";
import type { OracleBinding, SemanticOracle } from "../src/semantic-oracle/model.js";
import type { Probe } from "../src/scenario/index.js";

const CAPS: ProviderCapabilities = {
  provider: "tsgo",
  positionEncoding: "utf-8",
  diagnosticsPush: false,
  completionResolve: true,
};

function probe(over: Partial<Probe> = {}): Probe {
  return {
    id: "hover.probe",
    method: "hover",
    anchor: "count",
    mappingPolicy: "none",
    confidence: "high",
    dimension: "vueSemanticValidity",
    requiresSourceMap: false,
    requiredDrivers: ["rawLsp", "tsgo"],
    capabilityRequirements: [],
    ...over,
  };
}

/** A recording fake verter client returning canned raw LSP responses per method. */
class FakeVerter implements OracleVerterClient {
  positionEncoding: "utf-16" | "utf-8" | "utf-32" = "utf-16";
  readonly calls: { method: string; params: unknown }[] = [];
  constructor(private readonly responder: (method: string) => unknown) {}
  async sendRequest<T = unknown>(method: string, params?: unknown): Promise<T> {
    this.calls.push({ method, params });
    return this.responder(method) as T;
  }
}

/** A recording fake bridge returning a canned query response. */
class FakeBridge implements OracleBridgeClient {
  readonly queries: QueryInput[] = [];
  constructor(
    private readonly queryResponder: (input: QueryInput) => QueryResponse | ErrorResponse,
  ) {}
  async query(input: QueryInput): Promise<QueryResponse | ErrorResponse> {
    this.queries.push(input);
    return this.queryResponder(input);
  }
}

function hoverResult(contents: string): QueryResult {
  return { kind: "hover", hover: { contents } };
}
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

function vueContext(over: Partial<OracleSourceContext> = {}): OracleSourceContext {
  // A `.vue` script with a trailing anchor at the `count` usage.
  const raw = '<script setup lang="ts">\nconst count = 1\ncount // @dx-anchor count\n</script>\n';
  const { stripped, anchors } = stripAnchors(raw);
  const vueAnchors: AnchorMap = new Map();
  addFileAnchors(vueAnchors, "file:///Drawer.vue", { stripped, anchors });
  const oracle = prepareOracleSource("const count: number = 1\ncount // @dx-anchor count\n");
  return {
    vueUri: "file:///Drawer.vue",
    vueText: stripped,
    vueAnchors,
    oracleUri: "file:///count.ts",
    oraclePath: "/count.ts",
    oracleVersion: 1,
    oracle,
    ...over,
  };
}

function resolved(over: Partial<ResolvedOracleQuery> = {}): ResolvedOracleQuery {
  return {
    probe: probe(),
    binding: { probeId: "hover.probe", oracleAnchor: "count", requiredSnippets: ["number"] },
    vue: { uri: "file:///Drawer.vue", position: { line: 2, character: 0 } },
    oracle: { uri: "file:///count.ts", path: "/count.ts", version: 1, offset: 24 },
    ...over,
  };
}

describe("resolveLspPosition — encoding-correct `.vue` position from a UTF-16 anchor", () => {
  it("a UTF-16 server keeps the anchor column verbatim", () => {
    const pos = resolveLspPosition("const x = 1\ncount\n", { line: 1, character: 5 }, "utf-16");
    expect(pos).toEqual({ line: 1, character: 5 });
  });

  it("a UTF-8 server measures the column in bytes (a multibyte prefix widens it)", () => {
    // Line 1 is `π=` then the anchor at column 2 (UTF-16): `π` is 2 UTF-8 bytes, so
    // the byte column is 3, not 2.
    const pos = resolveLspPosition("x\nπ=y\n", { line: 1, character: 2 }, "utf-8");
    expect(pos).toEqual({ line: 1, character: 3 });
  });
});

describe("runResolvedOracleQuery — verter `.vue` vs the `.ts` oracle gold standard", () => {
  it("a WRONG verter hover (boolean where the oracle says number) -> divergence", async () => {
    const verter = new FakeVerter(() => ({ contents: "(alias) count: boolean" }));
    const tsgo = new FakeBridge(() => queryResponse(hoverResult("(alias) count: number")));
    const out = await runResolvedOracleQuery(resolved(), { verter, tsgo });

    expect(out.map((o) => o.kind)).toEqual(["divergence"]);
    expect(out[0].probe.dimension).toBe("vueSemanticValidity");
    // Verter was queried for hover at the resolved `.vue` position.
    expect(verter.calls).toEqual([
      {
        method: "textDocument/hover",
        params: {
          textDocument: { uri: "file:///Drawer.vue" },
          position: { line: 2, character: 0 },
        },
      },
    ]);
    // The bridge was queried at the resolved oracle byte offset.
    expect(tsgo.queries[0]).toMatchObject({
      method: "hover",
      path: "/count.ts",
      offset: 24,
      version: 1,
    });
  });

  it("a CORRECT verter hover matching the oracle -> agreement", async () => {
    const verter = new FakeVerter(() => ({ contents: "(alias) count: number" }));
    const tsgo = new FakeBridge(() => queryResponse(hoverResult("(alias) count: number")));
    const out = await runResolvedOracleQuery(resolved(), { verter, tsgo });
    expect(out.map((o) => o.kind)).toEqual(["agreement"]);
  });

  it("a completion probe forwards the trigger character to verter AND the bridge", async () => {
    const verter = new FakeVerter(() => ({ items: [{ label: "title" }] }));
    const tsgo = new FakeBridge((input) => ({
      type: "query",
      method: input.method,
      uri: input.uri,
      version: input.version,
      result: { kind: "completion", items: [{ label: "title" }], isIncomplete: false },
      capabilities: CAPS,
    }));
    const out = await runResolvedOracleQuery(
      resolved({
        probe: probe({ id: "c", method: "completion", anchor: "count" }),
        binding: {
          probeId: "c",
          oracleAnchor: "count",
          requiredLabels: ["title"],
          triggerCharacter: ".",
        },
      }),
      { verter, tsgo },
    );
    expect(out.map((o) => o.kind)).toEqual(["agreement"]);
    expect(verter.calls[0]).toEqual({
      method: "textDocument/completion",
      params: {
        textDocument: { uri: "file:///Drawer.vue" },
        position: { line: 2, character: 0 },
        context: { triggerKind: 2, triggerCharacter: "." },
      },
    });
    expect(tsgo.queries[0]).toMatchObject({ method: "completion", triggerCharacter: "." });
  });

  it("a definition probe queries `textDocument/definition`", async () => {
    const verter = new FakeVerter(() => ({
      uri: "file:///Drawer.vue",
      range: { start: { line: 1, character: 6 }, end: { line: 1, character: 11 } },
    }));
    const tsgo = new FakeBridge(() =>
      queryResponse({ kind: "definition", locations: [{ path: "/count.ts", start: 6, end: 11 }] }),
    );
    const out = await runResolvedOracleQuery(
      resolved({
        probe: probe({ id: "d", method: "definition", anchor: "count" }),
        binding: {
          probeId: "d",
          oracleAnchor: "count",
          expected: {
            uri: "file:///Drawer.vue",
            range: { start: { line: 1, character: 6 }, end: { line: 1, character: 11 } },
          },
        },
      }),
      { verter, tsgo },
    );
    expect(out.map((o) => o.kind)).toEqual(["agreement"]);
    expect(verter.calls[0].method).toBe("textDocument/definition");
  });

  it("an artifactParity probe is a hard authoring error -> OracleError (dimension contract)", async () => {
    const verter = new FakeVerter(() => ({ contents: "x" }));
    const tsgo = new FakeBridge(() => queryResponse(hoverResult("x")));
    await expect(
      runResolvedOracleQuery(resolved({ probe: probe({ dimension: "artifactParity" }) }), {
        verter,
        tsgo,
      }),
    ).rejects.toThrow(OracleError);
  });

  it("a diagnostics probe is a documented deferral seam -> skipped with a push-delivery reason", async () => {
    const verter = new FakeVerter(() => null);
    const tsgo = new FakeBridge(() => queryResponse(hoverResult("x")));
    const out = await runResolvedOracleQuery(
      resolved({ probe: probe({ id: "diag", method: "diagnostics", anchor: "count" }) }),
      { verter, tsgo },
    );
    expect(out.map((o) => o.kind)).toEqual(["skipped"]);
    const only = out[0];
    if (only.kind !== "skipped") throw new Error("unreachable");
    // Not a bare locked-in skip: the reason documents WHY (diagnostics are push-
    // delivered, not a pull request) and WHERE they are driven (the diagnostics
    // collector) — the deliberate runner/collector boundary.
    expect(only.reason).toMatch(/push/i);
    expect(only.reason).toMatch(/collector/i);
    // Verter was never queried for an unsupported live method.
    expect(verter.calls).toEqual([]);
  });

  it("a definition probe with NO expected authored identity is a hard authoring error", async () => {
    const verter = new FakeVerter(() => ({
      uri: "file:///Drawer.vue",
      range: { start: { line: 1, character: 6 }, end: { line: 1, character: 11 } },
    }));
    const tsgo = new FakeBridge(() =>
      queryResponse({ kind: "definition", locations: [{ path: "/count.ts", start: 6, end: 11 }] }),
    );
    await expect(
      runResolvedOracleQuery(
        resolved({
          probe: probe({ id: "d", method: "definition", anchor: "count" }),
          // No `expected`: a definition oracle without an authored identity could
          // otherwise pass ANY `.vue` target as a false agreement.
          binding: { probeId: "d", oracleAnchor: "count" },
        }),
        { verter, tsgo },
      ),
    ).rejects.toThrow(OracleError);
    // The fault is raised before verter is ever queried — never a silent agreement.
    expect(verter.calls).toEqual([]);
  });

  it("with both providers, a baseline disagreement is recorded and verter is not failed", async () => {
    const verter = new FakeVerter(() => ({ contents: "(alias) count: boolean" }));
    const tsgo = new FakeBridge(() => queryResponse(hoverResult("(alias) count: number")));
    const tsserver = new FakeBridge(() => queryResponse(hoverResult("(alias) count: string")));
    const out = await runResolvedOracleQuery(resolved(), { verter, tsgo, tsserver });
    expect(out.map((o) => o.kind)).toEqual(["baselineDisagreement"]);
  });
});

describe("runSemanticOracle — resolves every binding from a `.vue` scenario + `.ts` oracle", () => {
  const oracle: SemanticOracle = {
    family: "defineModel",
    oracleFile: "define-model.ts",
    scenarioId: "model-scenario",
    bindings: [
      {
        probeId: "hover.probe",
        oracleAnchor: "count",
        requiredSnippets: ["number"],
      } satisfies OracleBinding,
    ],
  };

  it("produces a vueSemanticValidity outcome per binding from resolved anchors", async () => {
    const verter = new FakeVerter(() => ({ contents: "(alias) count: number" }));
    const tsgo = new FakeBridge(() => queryResponse(hoverResult("(alias) count: number")));
    const probes = new Map<string, Probe>([["hover.probe", probe()]]);
    const out = await runSemanticOracle(oracle, probes, vueContext(), { verter, tsgo });
    expect(out.map((o) => o.kind)).toEqual(["agreement"]);
    expect(out[0].probe.dimension).toBe("vueSemanticValidity");
    // The oracle `.ts` byte offset was resolved (the trailing `count` usage on line 1).
    expect(tsgo.queries[0].path).toBe("/count.ts");
    expect(tsgo.queries[0].offset).toBeGreaterThan(0);
  });

  it("a binding referencing an unknown probe id is a hard authoring error", async () => {
    const verter = new FakeVerter(() => ({ contents: "x" }));
    const tsgo = new FakeBridge(() => queryResponse(hoverResult("x")));
    const probes = new Map<string, Probe>();
    await expect(runSemanticOracle(oracle, probes, vueContext(), { verter, tsgo })).rejects.toThrow(
      OracleError,
    );
  });
});
