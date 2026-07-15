import { describe, expect, it } from "vitest";

import type { Scenario } from "../src/scenario/index.js";
import { validateScenario } from "../src/scenario/index.js";

/** A fully-valid scenario; tests clone-and-mutate it to assert each rule. */
function validScenario(): Scenario {
  return {
    id: "minimal-member-access",
    fixture: "minimal-member-access",
    entryFile: "App.vue",
    anchors: ["memberAccess", "overlayClick", "importSite"],
    setup: [{ kind: "insert", anchor: "importSite", text: "import { Foo } from './foo'\n" }],
    script: [{ kind: "insert", anchor: "memberAccess", text: ".bar", burst: true }],
    probes: [
      {
        id: "member-completion",
        method: "completion",
        anchor: "memberAccess",
        mappingPolicy: "strict",
        confidence: "high",
        dimension: "artifactParity",
        requiresSourceMap: true,
        requiredDrivers: ["rawLsp", "tsgo"],
        capabilityRequirements: ["positionEncoding"],
      },
      {
        id: "overlay-hover",
        method: "hover",
        anchor: "overlayClick",
        mappingPolicy: "none",
        confidence: "high",
        dimension: "vueSemanticValidity",
        requiresSourceMap: false,
        requiredDrivers: ["rawLsp"],
        capabilityRequirements: [],
      },
    ],
    invariants: [
      {
        id: "overlay-surface",
        anchor: "overlayClick",
        method: "hover",
        assertion: "contains",
        value: "@click",
      },
    ],
    baselines: { tsgo: "required", tsserver: "requiredForCi", volar: "optional" },
    thresholds: {
      latency: { p95Ms: 200 },
      steadyStateCompileDelta: 0,
      recovery: { maxRecoveryMs: 1000 },
      flakeWindows: 2,
    },
  };
}

/** Codes present in a validation result, for concise assertions. */
function codes(input: unknown): string[] {
  return validateScenario(input).errors.map((e) => e.code);
}

describe("validateScenario — a well-formed scenario", () => {
  it("accepts a fully-valid scenario with ZERO errors", () => {
    const result = validateScenario(validScenario());
    expect(result.ok).toBe(true);
    expect(result.errors).toEqual([]);
  });

  it("does NOT flag the valid scenario with any of the cross-field rules", () => {
    const found = codes(validScenario());
    expect(found).not.toContain("source_map_required_but_mapping_policy_none");
    expect(found).not.toContain("mapping_policy_requires_source_map");
    expect(found).not.toContain("method_dimension_structurally_impossible");
    expect(found).not.toContain("confidence_exceeds_mapping_policy_ceiling");
  });
});

describe("validateScenario — duplicate probe ids", () => {
  it("flags two probes that share an id", () => {
    const s = validScenario();
    const dup = { ...s.probes[1], id: s.probes[0].id };
    const mutated = { ...s, probes: [...s.probes, dup] };
    const result = validateScenario(mutated);
    expect(result.ok).toBe(false);
    expect(result.errors.map((e) => e.code)).toContain("duplicate_probe_id");
    // The message names the offending id, not a bare blob.
    const err = result.errors.find((e) => e.code === "duplicate_probe_id");
    expect(err?.message).toContain(s.probes[0].id);
  });
});

describe("validateScenario — anchor membership", () => {
  it("flags a probe.anchor not declared in scenario.anchors", () => {
    const s = validScenario();
    const mutated = { ...s, probes: [{ ...s.probes[0], anchor: "ghostAnchor" }, s.probes[1]] };
    expect(codes(mutated)).toContain("probe_anchor_undeclared");
    // The valid sibling probe does not falsely trip the rule.
    expect(codes(validScenario())).not.toContain("probe_anchor_undeclared");
  });

  it("flags an edit-step anchor not declared in scenario.anchors", () => {
    const s = validScenario();
    const mutated = { ...s, script: [{ kind: "insert", anchor: "ghost", text: "x" }] };
    expect(codes(mutated)).toContain("edit_step_anchor_undeclared");
  });

  it("flags an invariant anchor not declared in scenario.anchors", () => {
    const s = validScenario();
    const mutated = { ...s, invariants: [{ ...s.invariants[0], anchor: "ghost" }] };
    expect(codes(mutated)).toContain("invariant_anchor_undeclared");
  });
});

describe("validateScenario — invalid enum values", () => {
  it("flags an unknown probe.method", () => {
    const s = validScenario();
    const mutated = { ...s, probes: [{ ...s.probes[0], method: "frobnicate" }, s.probes[1]] };
    expect(codes(mutated)).toContain("invalid_method");
  });

  it("flags an unknown mappingPolicy", () => {
    const s = validScenario();
    const mutated = { ...s, probes: [{ ...s.probes[0], mappingPolicy: "loose" }, s.probes[1]] };
    expect(codes(mutated)).toContain("invalid_mapping_policy");
  });

  it("flags an unknown confidence", () => {
    const s = validScenario();
    const mutated = { ...s, probes: [{ ...s.probes[0], confidence: "maybe" }, s.probes[1]] };
    expect(codes(mutated)).toContain("invalid_confidence");
  });

  it("flags an unknown dimension", () => {
    const s = validScenario();
    const mutated = { ...s, probes: [{ ...s.probes[0], dimension: "speed" }, s.probes[1]] };
    expect(codes(mutated)).toContain("invalid_dimension");
  });

  it("flags an unknown requiredDriver", () => {
    const s = validScenario();
    const mutated = {
      ...s,
      probes: [{ ...s.probes[0], requiredDrivers: ["rawLsp", "telepathy"] }, s.probes[1]],
    };
    expect(codes(mutated)).toContain("invalid_required_driver");
  });

  it("flags an unknown baseline requirement", () => {
    const s = validScenario();
    const mutated = { ...s, baselines: { ...s.baselines, tsgo: "someday" } };
    expect(codes(mutated)).toContain("invalid_baseline_requirement");
  });

  it("accepts an UNKNOWN capability requirement (the union is extensible)", () => {
    const s = validScenario();
    const mutated = {
      ...s,
      probes: [
        { ...s.probes[0], capabilityRequirements: ["acceptPath", "futureCapability"] },
        s.probes[1],
      ],
    };
    // An unknown but well-formed (non-empty string) capability is NOT an error.
    expect(codes(mutated)).not.toContain("invalid_capability_requirement");
    // …but a non-string capability entry IS.
    const broken = {
      ...s,
      probes: [{ ...s.probes[0], capabilityRequirements: [42] }, s.probes[1]],
    };
    expect(codes(broken)).toContain("invalid_capability_requirement");
  });
});

describe("validateScenario — requiresSourceMap consistency", () => {
  it("flags a `none`-policy probe that CLAIMS it requires a source map", () => {
    const s = validScenario();
    const mutated = {
      ...s,
      probes: [s.probes[0], { ...s.probes[1], mappingPolicy: "none", requiresSourceMap: true }],
    };
    expect(codes(mutated)).toContain("source_map_required_but_mapping_policy_none");
  });

  it("flags a mapping-policy probe that does NOT require a source map (the vice-versa case)", () => {
    const s = validScenario();
    const mutated = {
      ...s,
      probes: [{ ...s.probes[0], mappingPolicy: "strict", requiresSourceMap: false }, s.probes[1]],
    };
    expect(codes(mutated)).toContain("mapping_policy_requires_source_map");
  });
});

describe("validateScenario — structurally-impossible method dimension", () => {
  it("flags an operational method declared as vueSemanticValidity", () => {
    const s = validScenario();
    const churn = {
      id: "churn-probe",
      method: "churn",
      anchor: "memberAccess",
      mappingPolicy: "none",
      confidence: "high",
      dimension: "vueSemanticValidity",
      requiresSourceMap: false,
      requiredDrivers: ["rawLsp"],
      capabilityRequirements: [],
    };
    const mutated = { ...s, probes: [...s.probes, churn] };
    expect(codes(mutated)).toContain("method_dimension_structurally_impossible");
  });

  it("does NOT flag the same operational method as artifactParity", () => {
    const s = validScenario();
    const churn = {
      id: "churn-probe",
      method: "churn",
      anchor: "memberAccess",
      mappingPolicy: "none",
      confidence: "high",
      dimension: "artifactParity",
      requiresSourceMap: false,
      requiredDrivers: ["rawLsp"],
      capabilityRequirements: [],
    };
    const mutated = { ...s, probes: [...s.probes, churn] };
    expect(codes(mutated)).not.toContain("method_dimension_structurally_impossible");
  });
});

describe("validateScenario — confidence ceiling", () => {
  it("flags a nearest-token probe declared higher than low confidence", () => {
    const s = validScenario();
    const mutated = {
      ...s,
      probes: [
        {
          ...s.probes[0],
          mappingPolicy: "nearestTokenLowConfidence",
          confidence: "high",
        },
        s.probes[1],
      ],
    };
    expect(codes(mutated)).toContain("confidence_exceeds_mapping_policy_ceiling");
  });

  it("accepts a nearest-token probe at low confidence", () => {
    const s = validScenario();
    const mutated = {
      ...s,
      probes: [
        {
          ...s.probes[0],
          mappingPolicy: "nearestTokenLowConfidence",
          confidence: "low",
        },
        s.probes[1],
      ],
    };
    expect(codes(mutated)).not.toContain("confidence_exceeds_mapping_policy_ceiling");
  });
});

describe("validateScenario — totality over malformed input", () => {
  it("does not throw on non-object input and reports a typed error", () => {
    for (const bad of [null, undefined, 42, "scenario", []]) {
      const result = validateScenario(bad);
      expect(result.ok).toBe(false);
      expect(result.errors.length).toBeGreaterThan(0);
      // Every error carries a discriminable code + a human message.
      for (const err of result.errors) {
        expect(typeof err.code).toBe("string");
        expect(typeof err.message).toBe("string");
        expect(err.message.length).toBeGreaterThan(0);
      }
    }
  });

  it("flags missing required scalar fields", () => {
    const s = validScenario();
    expect(codes({ ...s, id: "" })).toContain("invalid_scenario_id");
    expect(codes({ ...s, fixture: 123 })).toContain("invalid_fixture");
    expect(codes({ ...s, entryFile: undefined })).toContain("invalid_entry_file");
  });
});

describe("validateScenario — thresholds are required", () => {
  it("flags a scenario whose `thresholds` are absent", () => {
    // `thresholds` is a required part of the model — threshold checks read
    // `scenario.thresholds.*`, so a passing validation must imply its presence.
    const partial = { ...validScenario() } as Record<string, unknown>;
    delete partial.thresholds;
    expect(codes(partial)).toContain("invalid_thresholds");
  });

  it("flags a `thresholds` that is not an object", () => {
    const s = validScenario();
    expect(codes({ ...s, thresholds: 42 })).toContain("invalid_thresholds");
    expect(codes({ ...s, thresholds: [] })).toContain("invalid_thresholds");
  });

  it("does NOT flag a well-formed thresholds object", () => {
    expect(codes(validScenario())).not.toContain("invalid_thresholds");
  });
});

describe("validateScenario — nested threshold fields are validated completely", () => {
  it("flags a present-but-non-object `latency` (a passing validation must imply a valid shape)", () => {
    // `asRecord(thresholds.latency)` returns undefined for a non-object, which must
    // NOT silently skip validation — a present-but-malformed sub-object is a fault.
    const s = validScenario();
    const result = validateScenario({ ...s, thresholds: { latency: 42 } });
    expect(result.ok).toBe(false);
    expect(result.errors.map((e) => e.code)).toContain("invalid_threshold_field");
    // The error names the offending nested path, not a bare blob.
    const err = result.errors.find((e) => e.code === "invalid_threshold_field");
    expect(err?.path).toBe("thresholds.latency");
  });

  it("flags a present-but-non-object `recovery`", () => {
    const s = validScenario();
    const result = validateScenario({ ...s, thresholds: { recovery: 42 } });
    expect(result.ok).toBe(false);
    expect(result.errors.map((e) => e.code)).toContain("invalid_threshold_field");
    const err = result.errors.find((e) => e.code === "invalid_threshold_field");
    expect(err?.path).toBe("thresholds.recovery");
  });

  it("flags a present-but-non-object nested field that is an array (not a record)", () => {
    const s = validScenario();
    expect(codes({ ...s, thresholds: { ...s.thresholds, latency: [] } })).toContain(
      "invalid_threshold_field",
    );
    expect(codes({ ...s, thresholds: { ...s.thresholds, recovery: [] } })).toContain(
      "invalid_threshold_field",
    );
  });

  it("flags a malformed nested numeric sub-field of each declared kind", () => {
    const s = validScenario();
    // latency sub-fields
    expect(codes({ ...s, thresholds: { ...s.thresholds, latency: { p50Ms: "soon" } } })).toContain(
      "invalid_threshold",
    );
    expect(codes({ ...s, thresholds: { ...s.thresholds, latency: { p95Ms: -1 } } })).toContain(
      "negative_threshold",
    );
    // recovery sub-fields
    expect(
      codes({ ...s, thresholds: { ...s.thresholds, recovery: { maxRecoveryMs: -5 } } }),
    ).toContain("negative_threshold");
    expect(
      codes({ ...s, thresholds: { ...s.thresholds, recovery: { stableIntervals: {} } } }),
    ).toContain("invalid_threshold");
    // top-level scalar threshold fields
    expect(
      codes({ ...s, thresholds: { ...s.thresholds, steadyStateCompileDelta: "lots" } }),
    ).toContain("invalid_threshold");
    expect(codes({ ...s, thresholds: { ...s.thresholds, flakeWindows: {} } })).toContain(
      "invalid_threshold",
    );
  });

  it("accepts a fully-valid nested thresholds with ZERO errors (unchanged)", () => {
    // An absent optional sub-object stays valid; only present-but-malformed faults.
    const s = validScenario();
    expect(validateScenario({ ...s, thresholds: { steadyStateCompileDelta: 0 } }).ok).toBe(true);
    expect(validateScenario(validScenario()).ok).toBe(true);
    expect(codes(validScenario())).not.toContain("invalid_threshold_field");
  });
});

describe("validateScenario — an invariant must assert on a Vue surface", () => {
  it("flags an invariant whose method is an operational signal (no Vue surface)", () => {
    // An invariant is a direct Vue-surface assertion; an operational signal
    // (churn/latency/log/recovery) produces a measurement, not a surface string.
    const s = validScenario();
    const mutated = { ...s, invariants: [{ ...s.invariants[0], method: "churn" }] };
    expect(codes(mutated)).toContain("invariant_method_not_a_surface");
  });

  it("accepts an invariant backed by any semantic-query method", () => {
    const s = validScenario();
    for (const method of [
      "completion",
      "hover",
      "definition",
      "diagnostics",
      "codeAction",
      "autoImport",
    ] as const) {
      const mutated = { ...s, invariants: [{ ...s.invariants[0], method }] };
      expect(codes(mutated)).not.toContain("invariant_method_not_a_surface");
    }
  });
});
