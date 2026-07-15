import { describe, expect, it } from "vitest";

import {
  CONFIDENCE_LEVELS,
  MAPPING_POLICIES,
  OPERATIONAL_METHODS,
  PROBE_METHODS,
  REQUIRED_DRIVERS,
  REQUIRED_DRIVER_LABELS,
  SEMANTIC_DIMENSIONS,
  confidenceWithinCeiling,
  effectiveConfidence,
  isConfidence,
  isMappingPolicy,
  isOperationalMethod,
  isProbeMethod,
  isRequiredDriver,
  isSemanticDimension,
  mappingPolicyConfidenceCeiling,
  mappingPolicyRequiresSourceMap,
  methodSupportsDimension,
} from "../src/scenario/index.js";

describe("scenario enumerations", () => {
  it("encodes EXACTLY the probe-method set, no more, no less", () => {
    expect([...PROBE_METHODS]).toEqual([
      "completion",
      "hover",
      "definition",
      "diagnostics",
      "codeAction",
      "autoImport",
      "churn",
      "latency",
      "log",
      "recovery",
    ]);
    // Negative: a near-miss method outside the known probe-method set is rejected.
    expect(isProbeMethod("references")).toBe(false);
    expect(isProbeMethod("completion")).toBe(true);
    expect(isProbeMethod(5)).toBe(false);
    expect(isProbeMethod(undefined)).toBe(false);
  });

  it("encodes EXACTLY the mapping policies", () => {
    expect([...MAPPING_POLICIES]).toEqual([
      "strict",
      "memberBoundaryFallback",
      "nearestTokenLowConfidence",
      "none",
    ]);
    expect(isMappingPolicy("strict")).toBe(true);
    expect(isMappingPolicy("loose")).toBe(false);
  });

  it("encodes EXACTLY the confidence levels", () => {
    expect([...CONFIDENCE_LEVELS]).toEqual(["high", "medium", "low"]);
    expect(isConfidence("medium")).toBe(true);
    expect(isConfidence("maybe")).toBe(false);
  });

  it("encodes EXACTLY the semantic dimensions", () => {
    expect([...SEMANTIC_DIMENSIONS]).toEqual(["artifactParity", "vueSemanticValidity"]);
    expect(isSemanticDimension("artifactParity")).toBe(true);
    expect(isSemanticDimension("performance")).toBe(false);
  });

  it("encodes the canonical required-driver identifiers and documents the label mapping", () => {
    expect([...REQUIRED_DRIVERS]).toEqual([
      "rawLsp",
      "extensionHost",
      "rustComplement",
      "tsgo",
      "tsserver",
      "volar",
    ]);
    // Every canonical id is accepted…
    for (const driver of REQUIRED_DRIVERS) {
      expect(isRequiredDriver(driver)).toBe(true);
    }
    // …and a genuinely-unknown editor/driver (or non-string) is rejected.
    expect(isRequiredDriver("emacs")).toBe(false);
    expect(isRequiredDriver("sublime")).toBe(false);
    expect(isRequiredDriver(5)).toBe(false);
    // The identifier→label mapping is first-class DATA, not a doc-comment note.
    expect(REQUIRED_DRIVER_LABELS.rawLsp).toBe("raw-LSP");
    expect(REQUIRED_DRIVER_LABELS.extensionHost).toBe("extension-host");
    expect(REQUIRED_DRIVER_LABELS.rustComplement).toBe("Rust complement");
    expect(REQUIRED_DRIVER_LABELS.tsgo).toBe("tsgo");
    expect(REQUIRED_DRIVER_LABELS.tsserver).toBe("tsserver");
    expect(REQUIRED_DRIVER_LABELS.volar).toBe("Volar");
  });
});

describe("structural confidence relationship", () => {
  it("derives the confidence ceiling each mapping policy structurally permits", () => {
    expect(mappingPolicyConfidenceCeiling("strict")).toBe("high");
    expect(mappingPolicyConfidenceCeiling("memberBoundaryFallback")).toBe("medium");
    expect(mappingPolicyConfidenceCeiling("nearestTokenLowConfidence")).toBe("low");
    expect(mappingPolicyConfidenceCeiling("none")).toBe("high");
  });

  it("treats `nearestTokenLowConfidence` as STRUCTURALLY low, not free-text", () => {
    // A nearest-token mapping cannot be declared high/medium confidence.
    expect(confidenceWithinCeiling("high", "nearestTokenLowConfidence")).toBe(false);
    expect(confidenceWithinCeiling("medium", "nearestTokenLowConfidence")).toBe(false);
    expect(confidenceWithinCeiling("low", "nearestTokenLowConfidence")).toBe(true);
    // Strict mapping permits the full range up to high.
    expect(confidenceWithinCeiling("high", "strict")).toBe(true);
    expect(confidenceWithinCeiling("low", "strict")).toBe(true);
  });

  it("derives an effective confidence by clamping the declared value to the ceiling", () => {
    // A high-confidence claim on a nearest-token probe is clamped DOWN to low.
    expect(
      effectiveConfidence({ confidence: "high", mappingPolicy: "nearestTokenLowConfidence" }),
    ).toBe("low");
    expect(
      effectiveConfidence({ confidence: "medium", mappingPolicy: "memberBoundaryFallback" }),
    ).toBe("medium");
    // A strict probe keeps its declared confidence (no clamp).
    expect(effectiveConfidence({ confidence: "high", mappingPolicy: "strict" })).toBe("high");
    // A low declaration is never raised by a permissive ceiling.
    expect(effectiveConfidence({ confidence: "low", mappingPolicy: "strict" })).toBe("low");
  });
});

describe("requiresSourceMap relationship", () => {
  it("requires a source map for every mapping policy EXCEPT `none`", () => {
    expect(mappingPolicyRequiresSourceMap("strict")).toBe(true);
    expect(mappingPolicyRequiresSourceMap("memberBoundaryFallback")).toBe(true);
    expect(mappingPolicyRequiresSourceMap("nearestTokenLowConfidence")).toBe(true);
    // A direct Vue-surface probe (`none`) continues without a map.
    expect(mappingPolicyRequiresSourceMap("none")).toBe(false);
  });
});

describe("method↔dimension relationship", () => {
  it("forbids vueSemanticValidity for the operational signal methods", () => {
    expect([...OPERATIONAL_METHODS]).toEqual(["churn", "latency", "log", "recovery"]);
    for (const method of OPERATIONAL_METHODS) {
      expect(isOperationalMethod(method)).toBe(true);
      // Operational signals carry no curated-oracle / Vue-surface semantic claim.
      expect(methodSupportsDimension(method, "vueSemanticValidity")).toBe(false);
      // …but they ARE verter-internal artifact-parity operational probes.
      expect(methodSupportsDimension(method, "artifactParity")).toBe(true);
    }
  });

  it("permits BOTH dimensions for the semantic query methods", () => {
    expect(isOperationalMethod("hover")).toBe(false);
    for (const method of [
      "completion",
      "hover",
      "definition",
      "diagnostics",
      "autoImport",
    ] as const) {
      expect(methodSupportsDimension(method, "artifactParity")).toBe(true);
      expect(methodSupportsDimension(method, "vueSemanticValidity")).toBe(true);
    }
  });
});
