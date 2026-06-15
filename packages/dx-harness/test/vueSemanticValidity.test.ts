import { describe, expect, it } from "vitest";

import {
  classifyOracleCompletion,
  classifyOracleDefinition,
  classifyOracleDiagnostics,
  classifyOracleHover,
  classifyOracleProbe,
  compareOracleDiagnostics,
} from "../src/differential/vueSemanticValidity.js";
import type {
  NormalizedCompletionItem,
  NormalizedDiagnostic,
  NormalizedHover,
  NormalizedLocation,
} from "../src/baseline/bridgeClient.js";
import type {
  CanonicalCompletionList,
  CanonicalDefinitionTarget,
  CanonicalDiagnostic,
  CanonicalHover,
} from "../src/normalize/index.js";
import type { Probe } from "../src/scenario/index.js";

// A vueSemanticValidity probe: a direct Vue-surface probe (mappingPolicy `none`,
// requiresSourceMap false) — the oracle compares verter-on-`.vue` against
// tsgo-on-the-`.ts` gold standard, never through verter's emitted artifact.
function probe(over: Partial<Probe> = {}): Probe {
  return {
    id: "p1",
    method: "hover",
    anchor: "a1",
    mappingPolicy: "none",
    confidence: "high",
    dimension: "vueSemanticValidity",
    requiresSourceMap: false,
    requiredDrivers: ["rawLsp", "tsgo", "tsserver"],
    capabilityRequirements: [],
    ...over,
  };
}

function completionList(labels: readonly string[]): CanonicalCompletionList {
  return {
    items: labels.map((label) => ({ label })),
    isIncomplete: false,
    noSuggestionsCollapse: labels.length === 0,
  };
}

function baselineCompletion(items: readonly NormalizedCompletionItem[]): {
  items: readonly NormalizedCompletionItem[];
  isIncomplete: boolean;
} {
  return { items, isIncomplete: false };
}

describe("classifyOracleHover — verter type label vs the `.ts` oracle gold standard", () => {
  it("a semantically WRONG verter hover (boolean where the oracle says string) -> divergence", () => {
    const verter: CanonicalHover = { contents: "```ts\n(property) title: boolean\n```" };
    const oracle: NormalizedHover = { contents: "```ts\n(property) Props.title: string\n```" };
    const out = classifyOracleHover({
      probe: probe(),
      verter,
      providers: { tsgo: { ok: true, output: oracle } },
      requiredSnippets: ["string"],
    });
    expect(out.map((o) => o.kind)).toEqual(["divergence"]);
    const only = out[0];
    if (only.kind !== "divergence") throw new Error("unreachable");
    // The dimension flows through from the probe — this is the vueSemanticValidity rail.
    expect(only.probe.dimension).toBe("vueSemanticValidity");
    // The intended type token is absent from verter's hover.
    expect(only.findings.some((f) => f.class === "missingSnippet")).toBe(true);
  });

  it("a CORRECT verter hover (label + snippet match) -> agreement, no false divergence", () => {
    const same = "```ts\n(parameter) e: MouseEvent\n```";
    const out = classifyOracleHover({
      probe: probe(),
      verter: { contents: same },
      providers: { tsgo: { ok: true, output: { contents: same } } },
      requiredSnippets: ["MouseEvent"],
    });
    expect(out.map((o) => o.kind)).toEqual(["agreement"]);
    const only = out[0];
    if (only.kind !== "agreement") throw new Error("unreachable");
    expect(only.probe.dimension).toBe("vueSemanticValidity");
  });

  it("native MouseEvent handler-arg mismatch (verter says Event) is caught", () => {
    const out = classifyOracleHover({
      probe: probe({ id: "click.event" }),
      verter: { contents: "(parameter) e: Event" },
      providers: { tsgo: { ok: true, output: { contents: "(parameter) e: MouseEvent" } } },
      requiredSnippets: ["MouseEvent"],
    });
    expect(out.map((o) => o.kind)).toEqual(["divergence"]);
    const only = out[0];
    if (only.kind !== "divergence") throw new Error("unreachable");
    // Both the full-label mismatch AND the missing intended type token fire.
    expect(only.findings.some((f) => f.class === "typeLabelMismatch")).toBe(true);
    expect(only.findings.some((f) => f.class === "missingSnippet")).toBe(true);
  });

  it("docs-only churn under a matching type label -> agreement (unstable docs stripped)", () => {
    const out = classifyOracleHover({
      probe: probe(),
      verter: { contents: "```ts\nconst count: number\n```\n\nThe live counter." },
      providers: { tsgo: { ok: true, output: { contents: "```ts\nconst count: number\n```" } } },
    });
    expect(out.map((o) => o.kind)).toEqual(["agreement"]);
  });
});

describe("classifyOracleCompletion — label set / required-label presence vs the oracle", () => {
  it("an ORDER-ONLY completion difference -> agreement (order-insensitive)", () => {
    const out = classifyOracleCompletion({
      probe: probe({ method: "completion" }),
      verter: completionList(["title", "count"]),
      providers: {
        tsgo: { ok: true, output: baselineCompletion([{ label: "count" }, { label: "title" }]) },
      },
    });
    expect(out.map((o) => o.kind)).toEqual(["agreement"]);
  });

  it("a required label absent from verter -> missingLabel divergence", () => {
    const out = classifyOracleCompletion({
      probe: probe({ method: "completion" }),
      verter: completionList(["count"]),
      providers: { tsgo: { ok: true, output: baselineCompletion([{ label: "count" }]) } },
      requiredLabels: ["title"],
    });
    expect(out.map((o) => o.kind)).toEqual(["divergence"]);
    const only = out[0];
    if (only.kind !== "divergence") throw new Error("unreachable");
    expect(only.class).toBe("missingLabel");
  });

  it("No-Suggestions collapse: verter empty where the oracle is non-empty -> divergence", () => {
    const out = classifyOracleCompletion({
      probe: probe({ method: "completion" }),
      verter: completionList([]),
      providers: { tsgo: { ok: true, output: baselineCompletion([{ label: "title" }]) } },
    });
    const only = out[0];
    if (only.kind !== "divergence") throw new Error("unreachable");
    expect(only.class).toBe("noSuggestionsCollapse");
  });

  it("a label/kind/insert match whose import-source detail differs -> divergence", () => {
    // The completion exists in both with the same label, kind, and insert text, but
    // verter resolves it from the wrong module — the import-source `detail` differs.
    const out = classifyOracleCompletion({
      probe: probe({ method: "completion" }),
      verter: {
        items: [
          {
            label: "Drawer",
            kind: "Variable",
            insertText: "Drawer",
            detail: "(alias) Drawer\nimport Drawer from './wrong'",
          },
        ],
        isIncomplete: false,
        noSuggestionsCollapse: false,
      },
      providers: {
        tsgo: {
          ok: true,
          output: baselineCompletion([
            {
              label: "Drawer",
              kind: "Variable",
              insertText: "Drawer",
              detail: "(alias) Drawer\nimport Drawer from './right'",
            },
          ]),
        },
      },
    });
    expect(out.map((o) => o.kind)).toEqual(["divergence"]);
    const only = out[0];
    if (only.kind !== "divergence") throw new Error("unreachable");
    expect(only.findings.some((f) => f.class === "typeLabelMismatch")).toBe(true);
  });

  it("a label/kind/insert match whose import-source detail also matches -> agreement", () => {
    const detail = "(alias) ref\nimport { ref } from 'vue'";
    const out = classifyOracleCompletion({
      probe: probe({ method: "completion" }),
      verter: {
        items: [{ label: "ref", kind: "Function", insertText: "ref", detail }],
        isIncomplete: false,
        noSuggestionsCollapse: false,
      },
      providers: {
        tsgo: {
          ok: true,
          output: baselineCompletion([
            { label: "ref", kind: "Function", insertText: "ref", detail },
          ]),
        },
      },
    });
    expect(out.map((o) => o.kind)).toEqual(["agreement"]);
  });
});

describe("classifyOracleDefinition — expected Vue identity, never line===0", () => {
  it("verter resolves a precise line-0 target matching the expected identity -> agreement", () => {
    const verter: readonly CanonicalDefinitionTarget[] = [
      {
        uri: "file:///Drawer.vue",
        range: { start: { line: 0, character: 6 }, end: { line: 0, character: 11 } },
      },
    ];
    const oracleLocs: readonly NormalizedLocation[] = [{ path: "/Drawer.ts", start: 6, end: 11 }];
    const out = classifyOracleDefinition({
      probe: probe({ method: "definition" }),
      verter,
      providers: { tsgo: { ok: true, output: oracleLocs } },
      expected: {
        uri: "file:///Drawer.vue",
        range: { start: { line: 0, character: 6 }, end: { line: 0, character: 11 } },
      },
    });
    // A precise line-0 range that matches the expected identity is NOT a failure.
    expect(out.map((o) => o.kind)).toEqual(["agreement"]);
  });

  it("verter produces no definition for an expected symbol -> wrongTarget divergence", () => {
    const out = classifyOracleDefinition({
      probe: probe({ method: "definition" }),
      verter: [],
      providers: { tsgo: { ok: true, output: [{ path: "/Drawer.ts", start: 6, end: 11 }] } },
      expected: { uri: "file:///Drawer.vue" },
    });
    const only = out[0];
    if (only.kind !== "divergence") throw new Error("unreachable");
    expect(only.class).toBe("wrongTarget");
  });

  it("verter resolves ONLY into a generated artifact with no mapping back -> unmappedGenerated", () => {
    const out = classifyOracleDefinition({
      probe: probe({ method: "definition" }),
      verter: [
        {
          uri: "file:///Drawer.vue.tsx",
          range: { start: { line: 4, character: 2 }, end: { line: 4, character: 7 } },
          fromGenerated: true,
        },
      ],
      providers: { tsgo: { ok: true, output: [{ path: "/Drawer.ts", start: 6, end: 11 }] } },
      expected: { uri: "file:///Drawer.vue" },
    });
    const only = out[0];
    if (only.kind !== "divergence") throw new Error("unreachable");
    expect(only.class).toBe("unmappedGenerated");
  });
});

describe("classifyOracle* — baseline disagreement is recorded, verter is NOT failed", () => {
  it("tsgo and tsserver disagree on the `.ts` hover -> baselineDisagreement, no verter comparison", () => {
    const out = classifyOracleHover({
      probe: probe(),
      // verter is WRONG, but the two baselines disagree, so verter is never compared.
      verter: { contents: "(parameter) e: Event" },
      providers: {
        tsgo: { ok: true, output: { contents: "(parameter) e: MouseEvent" } },
        tsserver: { ok: true, output: { contents: "(parameter) e: PointerEvent" } },
      },
      requiredSnippets: ["MouseEvent"],
    });
    expect(out.map((o) => o.kind)).toEqual(["baselineDisagreement"]);
  });

  it("a named authoritative provider compares verter only against it, ignoring the disagreement", () => {
    const out = classifyOracleHover({
      probe: probe(),
      verter: { contents: "(parameter) e: MouseEvent" },
      providers: {
        tsgo: { ok: true, output: { contents: "(parameter) e: MouseEvent" } },
        tsserver: { ok: true, output: { contents: "(parameter) e: PointerEvent" } },
      },
      authoritativeProvider: "tsgo",
      requiredSnippets: ["MouseEvent"],
    });
    expect(out.map((o) => o.kind)).toEqual(["agreement"]);
  });

  it("a bridge provider_error refusal -> skipped, never a verter failure", () => {
    const out = classifyOracleHover({
      probe: probe(),
      verter: { contents: "x" },
      providers: {
        tsgo: {
          ok: false,
          error: { type: "error", kind: "provider_error", message: "tsgo crashed" },
        },
      },
    });
    expect(out.map((o) => o.kind)).toEqual(["skipped"]);
  });
});

describe("compareOracleDiagnostics — cross-file identity/category, default-range collapse", () => {
  it("a verter-only false-red the oracle does not emit -> verterOnly", () => {
    const verter: readonly CanonicalDiagnostic[] = [
      {
        range: { start: { line: 2, character: 1 }, end: { line: 2, character: 5 } },
        severity: "Error",
        code: "2322",
        message: "not assignable",
      },
    ];
    const baseline: readonly NormalizedDiagnostic[] = [];
    const found = compareOracleDiagnostics(verter, baseline);
    expect(found.map((d) => d.class)).toEqual(["verterOnly"]);
  });

  it("an oracle diagnostic verter misses -> baselineOnly", () => {
    const found = compareOracleDiagnostics(
      [],
      [{ message: "x", severity: "error", start: 1, end: 2, code: "2304" }],
    );
    expect(found.map((d) => d.class)).toEqual(["baselineOnly"]);
  });

  it("with no authored .vue span, the cross-file `.ts` range is not comparable -> agreement", () => {
    const found = compareOracleDiagnostics(
      [
        {
          range: { start: { line: 9, character: 0 }, end: { line: 9, character: 4 } },
          severity: "Error",
          code: "2304",
          message: "cannot find name",
        },
      ],
      [{ message: "cannot find name", severity: "error", start: 1, end: 5, code: "2304" }],
    );
    // The baseline range is in `.ts` byte space with no shared coordinate to the `.vue`,
    // so with no authored span supplied the range cannot be checked — identity+category
    // match alone is agreement. (When an authored span IS supplied it IS checked, below.)
    expect(found).toEqual([]);
  });

  it("with an authored .vue span, a verter diagnostic at the WRONG non-default range -> rangeMismatch", () => {
    const found = compareOracleDiagnostics(
      [
        {
          range: { start: { line: 7, character: 2 }, end: { line: 7, character: 8 } },
          severity: "Error",
          code: "2322",
          message: "m",
        },
      ],
      [{ message: "m", severity: "error", start: 3, end: 7, code: "2322" }],
      {
        knownSourceSpans: {
          "2322": { start: { line: 4, character: 2 }, end: { line: 4, character: 8 } },
        },
      },
    );
    // Same code+category, but verter's authored `.vue` range is not the expected span and
    // is not the (0,0) default collapse — a genuine range divergence.
    expect(found.map((d) => d.class)).toEqual(["rangeMismatch"]);
  });

  it("with an authored .vue span, a verter diagnostic AT the expected range -> agreement", () => {
    const span = { start: { line: 4, character: 2 }, end: { line: 4, character: 8 } };
    const found = compareOracleDiagnostics(
      [{ range: span, severity: "Error", code: "2322", message: "m" }],
      [{ message: "m", severity: "error", start: 3, end: 7, code: "2322" }],
      { knownSourceSpans: { "2322": span } },
    );
    expect(found).toEqual([]);
  });

  it("verter collapsed a real diagnostic to the (0,0) default while the known span is elsewhere -> defaultRange", () => {
    const found = compareOracleDiagnostics(
      [
        {
          range: { start: { line: 0, character: 0 }, end: { line: 0, character: 0 } },
          severity: "Error",
          code: "2322",
          message: "m",
        },
      ],
      [{ message: "m", severity: "error", start: 3, end: 7, code: "2322" }],
      {
        knownSourceSpans: {
          "2322": { start: { line: 4, character: 2 }, end: { line: 4, character: 8 } },
        },
      },
    );
    expect(found.map((d) => d.class)).toEqual(["defaultRange"]);
  });

  it("a severity/category difference at the same identity -> severityMismatch", () => {
    const found = compareOracleDiagnostics(
      [
        {
          range: { start: { line: 1, character: 0 }, end: { line: 1, character: 4 } },
          severity: "Warning",
          code: "2304",
          message: "m",
        },
      ],
      [{ message: "m", severity: "error", start: 1, end: 2, code: "2304" }],
    );
    expect(found.map((d) => d.class)).toEqual(["severityMismatch"]);
  });
});

describe("classifyOracleDiagnostics + classifyOracleProbe dispatch", () => {
  it("classifyOracleDiagnostics records baseline disagreement on the shared `.ts` byte space", () => {
    const out = classifyOracleDiagnostics({
      probe: probe({ method: "diagnostics" }),
      verter: [],
      providers: {
        tsgo: {
          ok: true,
          output: [{ message: "m", severity: "error", start: 1, end: 2, code: "2304" }],
        },
        tsserver: {
          ok: true,
          output: [{ message: "m", severity: "error", start: 1, end: 2, code: "2552" }],
        },
      },
    });
    expect(out.map((o) => o.kind)).toEqual(["baselineDisagreement"]);
  });

  it("classifyOracleProbe dispatches on method and carries the vueSemanticValidity dimension", () => {
    const out = classifyOracleProbe({
      method: "completion",
      probe: probe({ method: "completion" }),
      verter: completionList(["title"]),
      providers: { tsgo: { ok: true, output: baselineCompletion([{ label: "title" }]) } },
    });
    expect(out.map((o) => o.kind)).toEqual(["agreement"]);
    expect(out[0].probe.dimension).toBe("vueSemanticValidity");
  });
});
