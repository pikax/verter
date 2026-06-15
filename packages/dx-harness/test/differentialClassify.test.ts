import { describe, expect, it } from "vitest";

import {
  MapAbsentGateError,
  assertKnownGoodSourceMap,
  classifyProbe,
  completionDisagrees,
  definitionDisagrees,
  diagnosticsDisagrees,
  hoverDisagrees,
  type BaselineCompletion,
  type ClassifyProbeInput,
  type Divergence,
} from "../src/differential/index.js";
import type { ErrorResponse, Probe } from "../src/index.js";

function probe(over: Partial<Probe> = {}): Probe {
  return {
    id: "p1",
    method: "hover",
    anchor: "a1",
    mappingPolicy: "strict",
    confidence: "high",
    dimension: "artifactParity",
    requiresSourceMap: true,
    requiredDrivers: ["rawLsp", "tsgo", "tsserver"],
    capabilityRequirements: [],
    ...over,
  };
}

const AGREE: Divergence[] = [];
const DISAGREE: Divergence[] = [{ class: "typeLabelMismatch", detail: "differs" }];

/** A baseline disagree that signals the two providers differ. */
const providersDiffer = (): Divergence[] => DISAGREE;
/** A baseline disagree that signals the two providers match. */
const providersAgree = (): Divergence[] => AGREE;

function base<B>(over: Partial<ClassifyProbeInput<B>>): ClassifyProbeInput<B> {
  return {
    probe: probe(),
    sourceMapPresent: true,
    providers: {},
    compareVerter: () => AGREE,
    baselineDisagree: providersAgree,
    ...over,
  } as ClassifyProbeInput<B>;
}

describe("classifyProbe — baseline disagreement is first-class and never fails verter", () => {
  it("two baselines that disagree (no authoritative) -> baselineDisagreement, verter NOT compared", () => {
    let verterCompared = false;
    const out = classifyProbe<string>(
      base({
        providers: { tsgo: { ok: true, output: "g" }, tsserver: { ok: true, output: "s" } },
        baselineDisagree: providersDiffer,
        compareVerter: () => {
          verterCompared = true;
          return DISAGREE;
        },
      }),
    );
    expect(out.map((o) => o.kind)).toEqual(["baselineDisagreement"]);
    expect(verterCompared).toBe(false);
    const first = out[0];
    if (first.kind !== "baselineDisagreement") throw new Error("unreachable");
    expect(first.providers).toEqual(["tsgo", "tsserver"]);
  });

  it("a named authoritative provider -> compare verter ONLY against it, ignoring the disagreement", () => {
    const compared: string[] = [];
    const out = classifyProbe<string>(
      base({
        providers: { tsgo: { ok: true, output: "g" }, tsserver: { ok: true, output: "s" } },
        authoritativeProvider: "tsgo",
        baselineDisagree: providersDiffer,
        compareVerter: (provider) => {
          compared.push(provider);
          return AGREE;
        },
      }),
    );
    expect(compared).toEqual(["tsgo"]);
    expect(out.map((o) => o.kind)).toEqual(["agreement"]);
  });

  it("two baselines that agree -> compare verter against the primary provider", () => {
    const compared: string[] = [];
    const out = classifyProbe<string>(
      base({
        providers: { tsgo: { ok: true, output: "g" }, tsserver: { ok: true, output: "s" } },
        baselineDisagree: providersAgree,
        compareVerter: (provider) => {
          compared.push(provider);
          return DISAGREE;
        },
      }),
    );
    expect(compared).toEqual(["tsgo"]);
    expect(out.map((o) => o.kind)).toEqual(["divergence"]);
  });

  it("a single available provider -> compare verter against it", () => {
    const out = classifyProbe<string>(
      base({
        providers: { tsserver: { ok: true, output: "s" } },
        compareVerter: () => AGREE,
      }),
    );
    expect(out.map((o) => o.kind)).toEqual(["agreement"]);
    const only = out[0];
    if (only.kind !== "agreement") throw new Error("unreachable");
    expect(only.provider).toBe("tsserver");
  });
});

describe("classifyProbe — map-absent and stale-artifact are recorded, never verter failures", () => {
  it("a requiresSourceMap probe with no source map -> mapAbsent per present provider, no comparison", () => {
    let compared = false;
    const out = classifyProbe<string>(
      base({
        sourceMapPresent: false,
        providers: { tsgo: { ok: true, output: "g" }, tsserver: { ok: true, output: "s" } },
        compareVerter: () => {
          compared = true;
          return DISAGREE;
        },
      }),
    );
    expect(out.every((o) => o.kind === "mapAbsent")).toBe(true);
    expect(out).toHaveLength(2);
    expect(compared).toBe(false);
  });

  it("a mappingPolicy:none probe (requiresSourceMap:false) proceeds without a map", () => {
    const out = classifyProbe<string>(
      base({
        probe: probe({ mappingPolicy: "none", requiresSourceMap: false }),
        sourceMapPresent: false,
        providers: { tsgo: { ok: true, output: "g" } },
        compareVerter: () => AGREE,
      }),
    );
    expect(out.map((o) => o.kind)).toEqual(["agreement"]);
  });

  it("a bridge compiled_code_map_absent error -> mapAbsent outcome", () => {
    const error: ErrorResponse = {
      type: "error",
      kind: "compiled_code_map_absent",
      message: "no map",
      uri: "file:///App.vue",
      requestedVersion: 3,
    };
    const out = classifyProbe<string>(base({ providers: { tsgo: { ok: false, error } } }));
    expect(out.map((o) => o.kind)).toEqual(["mapAbsent"]);
  });

  it("a bridge baseline_artifact_stale error -> baselineArtifactStale, NOT a verter failure", () => {
    const error: ErrorResponse = {
      type: "error",
      kind: "baseline_artifact_stale",
      message: "stale",
      uri: "file:///App.vue",
      requestedVersion: 5,
      haveVersion: 2,
    };
    const out = classifyProbe<string>(base({ providers: { tsserver: { ok: false, error } } }));
    expect(out).toHaveLength(1);
    const only = out[0];
    if (only.kind !== "baselineArtifactStale") throw new Error("unreachable");
    expect(only.requestedVersion).toBe(5);
    expect(only.haveVersion).toBe(2);
  });

  it("no baseline provider at all -> skipped", () => {
    const out = classifyProbe<string>(base({ providers: {} }));
    expect(out.map((o) => o.kind)).toEqual(["skipped"]);
  });
});

describe("assertKnownGoodSourceMap — the hard known-good gate fails immediately", () => {
  it("throws a typed gate error when the map is absent", () => {
    expect(() => assertKnownGoodSourceMap(false, "App.vue@v1")).toThrow(MapAbsentGateError);
  });
  it("does not throw when the map is present", () => {
    expect(() => assertKnownGoodSourceMap(true, "App.vue@v1")).not.toThrow();
  });
});

describe("classifyProbe — baseline-vs-baseline parity uses the forward field set", () => {
  it("two baselines sharing labels but differing on kind -> baselineDisagreement, verter NOT compared", () => {
    let verterCompared = false;
    const out = classifyProbe<BaselineCompletion>({
      probe: probe({ method: "completion" }),
      sourceMapPresent: true,
      providers: {
        tsgo: {
          ok: true,
          output: { items: [{ label: "a", kind: "Function" }], isIncomplete: false },
        },
        tsserver: {
          ok: true,
          output: { items: [{ label: "a", kind: "Variable" }], isIncomplete: false },
        },
      },
      compareVerter: () => {
        verterCompared = true;
        return AGREE;
      },
      baselineDisagree: (tsgo, tsserver) => completionDisagrees(tsgo, tsserver),
    });
    expect(out.map((o) => o.kind)).toEqual(["baselineDisagreement"]);
    expect(verterCompared).toBe(false);
  });
});

describe("baseline disagreement helpers compare two provider outputs directly", () => {
  it("completionDisagrees on a label-set difference, agrees on an equal set modulo order", () => {
    expect(
      completionDisagrees(
        { items: [{ label: "a" }, { label: "b" }], isIncomplete: false },
        { items: [{ label: "b" }, { label: "a" }], isIncomplete: false },
      ),
    ).toEqual([]);
    expect(
      completionDisagrees(
        { items: [{ label: "a" }], isIncomplete: false },
        { items: [{ label: "a" }, { label: "c" }], isIncomplete: false },
      ),
    ).not.toEqual([]);
  });

  it("hoverDisagrees compares stripped type labels", () => {
    expect(
      hoverDisagrees({ contents: "const x: string" }, { contents: "const x: string\n\ndocs" }),
    ).toEqual([]);
    expect(
      hoverDisagrees({ contents: "const x: string" }, { contents: "const x: number" }),
    ).not.toEqual([]);
  });

  it("completionDisagrees flags a kind difference on an otherwise-equal label set", () => {
    // Same labels, different kind — the forward comparator's field set includes kind,
    // so the baseline-vs-baseline equivalence must too (else verter is compared against
    // tsgo while tsgo and tsserver actually disagree).
    expect(
      completionDisagrees(
        { items: [{ label: "a", kind: "Function" }], isIncomplete: false },
        { items: [{ label: "a", kind: "Variable" }], isIncomplete: false },
      ),
    ).not.toEqual([]);
  });

  it("diagnosticsDisagrees flags a severity difference at the same code+range", () => {
    expect(
      diagnosticsDisagrees(
        [{ message: "m", severity: "error", start: 1, end: 2, code: "2304" }],
        [{ message: "m", severity: "warning", start: 1, end: 2, code: "2304" }],
      ),
    ).not.toEqual([]);
  });

  it("diagnosticsDisagrees treats two no-code messages differing only by CRLF/LF as equal", () => {
    // The baseline-vs-baseline identity must reuse the forward comparator's
    // `diagnosticIdentityKey`, which `normalizeEol`s a no-code message. Two providers
    // emitting the same message with CRLF vs LF line endings are NOT a disagreement.
    expect(
      diagnosticsDisagrees(
        [{ message: "line one\r\nline two", severity: "error", start: 1, end: 2 }],
        [{ message: "line one\nline two", severity: "error", start: 1, end: 2 }],
      ),
    ).toEqual([]);
    // A genuine no-code message difference still disagrees.
    expect(
      diagnosticsDisagrees(
        [{ message: "real difference A", severity: "error", start: 1, end: 2 }],
        [{ message: "real difference B", severity: "error", start: 1, end: 2 }],
      ),
    ).not.toEqual([]);
  });

  it("diagnosticsDisagrees namespaces the identity key so a code cannot collide with a message", () => {
    // A diagnostic with code "X" and a different diagnostic whose no-code message is "X"
    // are distinct. The shared `diagnosticIdentityKey` namespaces them (`code:X` vs
    // `msg:X`), so the two providers disagree — an unnamespaced `code ?? message` key
    // would wrongly treat them as the same diagnostic and report agreement.
    expect(
      diagnosticsDisagrees(
        [{ message: "ignored message", severity: "error", start: 1, end: 2, code: "X" }],
        [{ message: "X", severity: "error", start: 1, end: 2 }],
      ),
    ).not.toEqual([]);
  });

  it("definitionDisagrees / diagnosticsDisagrees compare native location/code sets", () => {
    expect(
      definitionDisagrees(
        [{ path: "a.ts", start: 1, end: 2 }],
        [{ path: "a.ts", start: 1, end: 2 }],
      ),
    ).toEqual([]);
    expect(
      definitionDisagrees(
        [{ path: "a.ts", start: 1, end: 2 }],
        [{ path: "a.ts", start: 9, end: 9 }],
      ),
    ).not.toEqual([]);
    expect(
      diagnosticsDisagrees(
        [{ message: "m", severity: "error", start: 1, end: 2, code: "2304" }],
        [{ message: "m", severity: "error", start: 1, end: 2, code: "2304" }],
      ),
    ).toEqual([]);
    expect(
      diagnosticsDisagrees(
        [{ message: "m", severity: "error", start: 1, end: 2, code: "2304" }],
        [{ message: "m", severity: "error", start: 1, end: 2, code: "2552" }],
      ),
    ).not.toEqual([]);
  });
});
