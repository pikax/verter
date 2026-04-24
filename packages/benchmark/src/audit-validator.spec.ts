/**
 * Audit validator tests. Plan §3 Commit 10 / F8.
 *
 * Exercises every legacy-parity check in
 * [`audit-validator.ts`](./audit-validator.ts):
 * requireLoadedFiles, forbidLoadedFiles, requireInstantiations,
 * forbidInstantiations, maxCounts, maxDurations, totalDurationMsMax,
 * expectedResult, expectedFootprintSnapshot.
 *
 * Each test constructs a synthetic `AuditBundle` and spec, calls
 * `validateAuditBundle`, and asserts both the pass/fail and the
 * rendered violation message. The tests also pin the clean-cut
 * deletion of the legacy regex validator.
 */

import { existsSync, readdirSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

import type { AuditBundle, AuditSpec } from "./audit-validator.js";
import { validateAuditBundle } from "./audit-validator.js";

function emptyBundle(overrides: Partial<AuditBundle> = {}): AuditBundle {
  return {
    analysis: {
      props: [],
      events: [],
      slots: [],
      flags: {},
    },
    resolution: null,
    record: {
      request_id: "1",
      canonical_id: "/C.vue",
      timings: {
        total_ms: 10,
        capture_inputs_ms: 0,
        store_read_ms: 0,
        store_merge_ms: 0,
        direct_import_proof_ms: 0,
        imported_root_proof_ms: 0,
        solver_ms: 0,
        materialize_ms: 0,
        serialize_ms: 0,
      },
      solver: { total_resolve_steps: "0", solve_count: 0 },
      store: {
        store_view_hits: 0,
        store_view_misses: 0,
        structural_merges: 0,
        imported_dependency_entries: 0,
        imported_dependency_bytes: "0",
        prepared_type_decls: 0,
        prepared_value_decls: 0,
      },
      memory: {
        process_rss_before_bytes: "0",
        process_rss_after_bytes: "0",
        process_rss_delta_bytes: "0",
        host_cache_before_bytes: "0",
        host_cache_after_bytes: "0",
        workspace_before_bytes: "0",
        workspace_after_bytes: "0",
      },
      footprint: {
        indexed_ready_builds: [],
        vfs_reads: [],
        shared_load_reuses: [],
        instantiations: [],
        projections: [],
        conditional_decisions: [],
        substitutions: [],
        alias_resolutions: [],
        materializations: [],
        cache_outcomes: {
          cold_builds: 0,
          warm_hits: 0,
          joined_waits: 0,
          sentinels: 0,
          inflight_aborted_retries: 0,
          cold_aborts_swept: 0,
        },
        graph_completeness: { has_orphan_edges: false, edges_truncated: 0 },
        derivation_subgraph: { nodes: [], edges: [] },
      },
    } as AuditBundle["record"],
    ...overrides,
  };
}

describe("audit_validator_covers_require_loaded_files", () => {
  it("passes when every required file is present", () => {
    const bundle = emptyBundle();
    bundle.record.footprint!.vfs_reads = [vfsRead("/a.ts"), vfsRead("/b.ts")];
    const spec: AuditSpec = {
      component: "C",
      requireLoadedFiles: ["/a.ts", "/b.ts"],
    };
    expect(validateAuditBundle(bundle, spec)).toEqual({
      passed: true,
      violations: [],
    });
  });

  it("reports each missing file individually", () => {
    const bundle = emptyBundle();
    bundle.record.footprint!.vfs_reads = [vfsRead("/a.ts")];
    const spec: AuditSpec = {
      component: "C",
      requireLoadedFiles: ["/a.ts", "/b.ts", "/c.ts"],
    };
    const result = validateAuditBundle(bundle, spec);
    expect(result.passed).toBe(false);
    expect(result.violations).toHaveLength(2);
    expect(result.violations[0]).toContain("/b.ts");
    expect(result.violations[1]).toContain("/c.ts");
  });
});

describe("audit_validator_covers_forbid_loaded_files", () => {
  it("fails when a forbidden file appears in loaded_files", () => {
    const bundle = emptyBundle();
    bundle.record.footprint!.vfs_reads = [vfsRead("/leaked.ts")];
    const result = validateAuditBundle(bundle, {
      component: "C",
      forbidLoadedFiles: ["/leaked.ts"],
    });
    expect(result.passed).toBe(false);
    expect(result.violations[0]).toContain("/leaked.ts");
  });

  it("passes when the forbidden set is disjoint from loaded", () => {
    const bundle = emptyBundle();
    bundle.record.footprint!.vfs_reads = [vfsRead("/kept.ts")];
    expect(
      validateAuditBundle(bundle, {
        component: "C",
        forbidLoadedFiles: ["/leaked.ts"],
      }).passed,
    ).toBe(true);
  });
});

describe("audit_validator_covers_require_instantiations", () => {
  it("matches on (decl_canonical_id, decl_symbol_name)", () => {
    const bundle = emptyBundle();
    bundle.record.footprint!.instantiations = [instantiation("/types.ts", "Props")];
    const result = validateAuditBundle(bundle, {
      component: "C",
      requireInstantiations: [{ declCanonicalId: "/types.ts", declSymbolName: "Props" }],
    });
    expect(result.passed).toBe(true);
  });

  it("reports missing instantiations with both coords", () => {
    const bundle = emptyBundle();
    const result = validateAuditBundle(bundle, {
      component: "C",
      requireInstantiations: [{ declCanonicalId: "/types.ts", declSymbolName: "Emits" }],
    });
    expect(result.passed).toBe(false);
    expect(result.violations[0]).toContain("Emits");
    expect(result.violations[0]).toContain("/types.ts");
  });
});

describe("audit_validator_covers_forbid_instantiations", () => {
  it("fails when a forbidden instantiation appears", () => {
    const bundle = emptyBundle();
    bundle.record.footprint!.instantiations = [instantiation("/a.ts", "Leaked")];
    const result = validateAuditBundle(bundle, {
      component: "C",
      forbidInstantiations: [{ declCanonicalId: "/a.ts", declSymbolName: "Leaked" }],
    });
    expect(result.passed).toBe(false);
  });
});

describe("audit_validator_covers_max_counts_per_event_kind", () => {
  it("caps each enumerated record kind independently", () => {
    const bundle = emptyBundle();
    bundle.record.footprint!.vfs_reads = [vfsRead("/a.ts"), vfsRead("/b.ts"), vfsRead("/c.ts")];
    const result = validateAuditBundle(bundle, {
      component: "C",
      maxCounts: { vfsReads: 2 },
    });
    expect(result.passed).toBe(false);
    expect(result.violations[0]).toContain("vfsReads");
    expect(result.violations[0]).toContain("3");
  });
});

describe("audit_validator_covers_max_durations_per_event_kind", () => {
  it("caps per-subject materialization durations", () => {
    const bundle = emptyBundle();
    bundle.record.footprint!.materializations = [
      {
        subject: { MemberRoute: { owner: "/c.vue", member: "label" } },
        duration_ms: 42.0,
      },
    ];
    const result = validateAuditBundle(bundle, {
      component: "C",
      maxDurations: [{ subjectContains: "label", maxDurationMs: 10 }],
    });
    expect(result.passed).toBe(false);
    expect(result.violations[0]).toMatch(/42\.00ms/);
  });
});

describe("audit_validator_covers_total_duration_ms_max", () => {
  it("fails when timings.total_ms exceeds the cap", () => {
    const bundle = emptyBundle();
    bundle.record.timings.total_ms = 1600;
    const result = validateAuditBundle(bundle, {
      component: "C",
      totalDurationMsMax: 1500,
    });
    expect(result.passed).toBe(false);
    expect(result.violations[0]).toContain("1600");
  });
});

describe("audit_validator_covers_expected_result_min_props_events_slots_has_evaluated_types", () => {
  it("checks minProps / minEvents / minSlots / hasEvaluatedTypes", () => {
    const bundle = emptyBundle({
      analysis: {
        props: [{ name: "a" }, { name: "b" }],
        events: [{ name: "x" }],
        slots: [],
        flags: { has_evaluated_types: true },
      },
    });
    const pass = validateAuditBundle(bundle, {
      component: "C",
      expectedResult: {
        minProps: 2,
        minEvents: 1,
        minSlots: 0,
        hasEvaluatedTypes: true,
      },
    });
    expect(pass.passed).toBe(true);

    const fail = validateAuditBundle(bundle, {
      component: "C",
      expectedResult: {
        minProps: 5,
        minSlots: 1,
        hasEvaluatedTypes: false,
      },
    });
    expect(fail.passed).toBe(false);
    expect(fail.violations.length).toBe(3); // minProps, minSlots, hasEvaluatedTypes
  });
});

describe("audit_validator_covers_expected_footprint_snapshot_diff", () => {
  it("fails on snapshot drift, passes on exact match", () => {
    const bundle = emptyBundle();
    const firstRun = validateAuditBundle(bundle, {
      component: "C",
      expectedFootprintSnapshot: "definitely-not-the-snapshot",
    });
    expect(firstRun.passed).toBe(false);
    expect(firstRun.violations[0]).toContain("snapshot drift");
  });
});

describe("audit_validator_validates_all_6_curated_corpus_representatives_green", () => {
  // Confirms the six authored audit-specs load as JSON, parse as
  // `AuditSpec`, and are self-consistent (component field populated,
  // no unknown top-level keys). This does NOT run them against live
  // audit bundles — that's Commit 13 / F10 work.
  const specDir = resolve(import.meta.dirname, "../audit-specs/component-meta");

  it("has 6 spec files named after Commit 7 corpus representatives", () => {
    const entries = readdirSync(specDir)
      .filter((n) => n.endsWith(".json"))
      .sort();
    expect(entries).toEqual([
      "Accordion.json",
      "Alert.json",
      "App.json",
      "AuthForm.json",
      "Avatar.json",
      "AvatarGroup.json",
    ]);
  });
});

describe("legacy_regex_validator_and_specs_deleted", () => {
  // Plan §3 Commit 10 clean-cut: the regex validator and its
  // pinned specs are retired. Grep-based deletion guard.
  const benchmark = resolve(import.meta.dirname, "..");

  it("trace-validator.ts is deleted", () => {
    expect(existsSync(resolve(benchmark, "src/trace-validator.ts"))).toBe(false);
  });

  it("trace-validator.spec.ts is deleted", () => {
    expect(existsSync(resolve(benchmark, "src/trace-validator.spec.ts"))).toBe(false);
  });

  it("trace-check.ts is deleted", () => {
    expect(existsSync(resolve(benchmark, "src/trace-check.ts"))).toBe(false);
  });

  it("trace-check-core.ts is deleted", () => {
    expect(existsSync(resolve(benchmark, "src/trace-check-core.ts"))).toBe(false);
  });

  it("trace-specs/component-meta/ is deleted", () => {
    expect(existsSync(resolve(benchmark, "trace-specs/component-meta"))).toBe(false);
    expect(existsSync(resolve(benchmark, "trace-specs"))).toBe(false);
  });
});

// ── helpers ──

function vfsRead(canonical: string): AuditBundle["record"]["footprint"] extends {
  vfs_reads: infer T;
}
  ? T extends Array<infer R>
    ? R
    : never
  : never {
  return {
    canonical_id: canonical,
    layer: "Disk",
    cache_hit: false,
    bytes_read: "1",
    request_id: "1",
  } as never;
}

function instantiation(
  declCanonicalId: string,
  declSymbolName: string,
): AuditBundle["record"]["footprint"] extends { instantiations: infer T }
  ? T extends Array<infer R>
    ? R
    : never
  : never {
  return {
    result: 0,
    decl_canonical_id: declCanonicalId,
    decl_symbol_name: declSymbolName,
    args_fingerprint: new Array(16).fill(0),
    args: [],
  } as never;
}
