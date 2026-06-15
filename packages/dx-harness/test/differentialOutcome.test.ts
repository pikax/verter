import { describe, expect, it } from "vitest";

import type { Probe } from "../src/index.js";
import {
  DIVERGENCE_PRIORITY,
  agreement,
  baselineArtifactStale,
  baselineDisagreement,
  foldOutcome,
  mapAbsent,
  probeIdentity,
  rankingSignal,
  skipped,
  type Divergence,
} from "../src/differential/index.js";

function probe(overrides: Partial<Probe> = {}): Probe {
  return {
    id: "p1",
    method: "hover",
    anchor: "a1",
    mappingPolicy: "strict",
    confidence: "high",
    dimension: "artifactParity",
    requiresSourceMap: true,
    requiredDrivers: ["rawLsp", "tsgo"],
    capabilityRequirements: [],
    ...overrides,
  };
}

describe("probeIdentity — the outcome envelope", () => {
  it("carries probe identity, provider-free, with declared AND structurally-clamped confidence", () => {
    const id = probeIdentity(
      probe({
        confidence: "high",
        mappingPolicy: "nearestTokenLowConfidence",
        dimension: "vueSemanticValidity",
      }),
    );
    expect(id.probeId).toBe("p1");
    expect(id.method).toBe("hover");
    expect(id.mappingPolicy).toBe("nearestTokenLowConfidence");
    expect(id.dimension).toBe("vueSemanticValidity");
    // declared high, but a nearest-token policy structurally clamps to low.
    expect(id.confidence).toBe("high");
    expect(id.effectiveConfidence).toBe("low");
    // requiresSourceMap is carried on the envelope (it governs the map-absent path).
    expect(id.requiresSourceMap).toBe(true);
  });
});

describe("agreement — the explicit agreement builder", () => {
  it("carries probe identity + provider, omitting detail when not given", () => {
    const out = agreement(probe(), "tsgo");
    expect(out.kind).toBe("agreement");
    expect(out.provider).toBe("tsgo");
    expect(out.probe.probeId).toBe("p1");
    expect("detail" in out).toBe(false);
  });

  it("includes detail when given", () => {
    const out = agreement(probe(), "tsserver", "matched on type label");
    expect(out.detail).toBe("matched on type label");
  });
});

describe("rankingSignal — a baseline-side signal, never attributed to verter", () => {
  it("is its own outcome kind, not a verter agreement/divergence", () => {
    const findings: Divergence[] = [{ class: "rankingMismatch", detail: "baseline order differs" }];
    const out = rankingSignal(probe(), "tsgo", findings);
    expect(out.kind).toBe("rankingSignal");
    // It is NOT folded into a verter divergence/agreement.
    expect(out.kind).not.toBe("divergence");
    expect(out.kind).not.toBe("agreement");
    expect(out.findings).toHaveLength(1);
    // `provider` names the baseline whose ranking was observed (not a verter comparison).
    expect(out.provider).toBe("tsgo");
  });
});

describe("foldOutcome — divergences fold into one outcome, no finding dropped", () => {
  it("empty findings -> agreement carrying identity + provider", () => {
    const out = foldOutcome(probe(), "tsgo", []);
    expect(out.kind).toBe("agreement");
    if (out.kind !== "agreement") throw new Error("unreachable");
    expect(out.provider).toBe("tsgo");
    expect(out.probe.probeId).toBe("p1");
  });

  it("non-empty findings -> divergence whose primary class is the highest-priority finding", () => {
    const findings: Divergence[] = [
      { class: "wrongKind", detail: "kind differs" },
      { class: "noSuggestionsCollapse", detail: "verter empty" },
    ];
    const out = foldOutcome(probe(), "tsserver", findings);
    expect(out.kind).toBe("divergence");
    if (out.kind !== "divergence") throw new Error("unreachable");
    // noSuggestionsCollapse outranks wrongKind, so it becomes the primary class.
    expect(out.class).toBe("noSuggestionsCollapse");
    expect(DIVERGENCE_PRIORITY.noSuggestionsCollapse).toBeLessThan(DIVERGENCE_PRIORITY.wrongKind);
    // No finding is dropped.
    expect(out.findings).toHaveLength(2);
    expect(out.findings.map((f) => f.class).sort()).toEqual(["noSuggestionsCollapse", "wrongKind"]);
    expect(out.provider).toBe("tsserver");
  });
});

describe("non-divergence outcomes are data, never thrown failures", () => {
  it("mapAbsent records the probe/provider and does not throw", () => {
    const out = mapAbsent(probe(), "tsgo", { detail: "no map at v3", requestedVersion: 3 });
    expect(out.kind).toBe("mapAbsent");
    expect(out.probe.probeId).toBe("p1");
    expect(out.provider).toBe("tsgo");
    expect(out.requestedVersion).toBe(3);
  });

  it("baselineArtifactStale carries requested/have versions", () => {
    const out = baselineArtifactStale(probe(), "tsserver", {
      detail: "stale",
      requestedVersion: 5,
      haveVersion: 2,
    });
    expect(out.kind).toBe("baselineArtifactStale");
    expect(out.requestedVersion).toBe(5);
    expect(out.haveVersion).toBe(2);
  });

  it("baselineDisagreement names BOTH providers and never carries a verter failure", () => {
    const findings: Divergence[] = [{ class: "typeLabelMismatch", detail: "tsgo != tsserver" }];
    const out = baselineDisagreement(probe(), ["tsgo", "tsserver"], findings);
    expect(out.kind).toBe("baselineDisagreement");
    expect(out.providers).toEqual(["tsgo", "tsserver"]);
    expect(out.findings).toHaveLength(1);
    // It is not a verter agreement/divergence — it carries no single `provider`.
    expect("provider" in out).toBe(false);
  });

  it("skipped records a reason", () => {
    const out = skipped(probe(), { reason: "no baseline provider available" });
    expect(out.kind).toBe("skipped");
    expect(out.reason).toBe("no baseline provider available");
  });
});
