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
import { renderFootprintForSnapshot, validateAuditBundle } from "./audit-validator.js";

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
  it("fails on snapshot drift", () => {
    const bundle = emptyBundle();
    const firstRun = validateAuditBundle(bundle, {
      component: "C",
      expectedFootprintSnapshot: "definitely-not-the-snapshot",
    });
    expect(firstRun.passed).toBe(false);
    expect(firstRun.violations[0]).toContain("snapshot drift");
  });

  it("passes on exact match against the renderer's current output", () => {
    // Review F11: the drift-only test above would not discriminate
    // against a stub renderer that always returned
    // `"definitely-not-the-snapshot"`. This positive case pins
    // exact-match behaviour by first computing the renderer output
    // for the fixture, then feeding it back as the expected
    // snapshot — a real exact-match MUST yield `passed: true` with
    // zero violations.
    const bundle = emptyBundle();
    const expectedSnapshot = renderFootprintForSnapshot(bundle.record.footprint);
    const run = validateAuditBundle(bundle, {
      component: "C",
      expectedFootprintSnapshot: expectedSnapshot,
    });
    expect(run.passed, `expected exact match, got violations: ${run.violations.join(" / ")}`).toBe(
      true,
    );
    expect(run.violations).toEqual([]);
  });

  it("discriminates between bundles with differing footprints", () => {
    // Complementary discrimination: a snapshot rendered from one
    // bundle MUST NOT match a bundle with a structurally different
    // footprint. Catches a regression where the renderer collapsed
    // distinct inputs to identical outputs.
    const bundleA = emptyBundle();
    const snapshotA = renderFootprintForSnapshot(bundleA.record.footprint);

    const bundleB = emptyBundle();
    bundleB.record.footprint!.shared_load_reuses = [
      {
        canonical_id: "/shared.ts",
        winner_request_id: "42",
        winner_audited: false,
      },
    ];

    const runBWithAsSnapshot = validateAuditBundle(bundleB, {
      component: "C",
      expectedFootprintSnapshot: snapshotA,
    });
    expect(runBWithAsSnapshot.passed).toBe(false);
    expect(runBWithAsSnapshot.violations[0]).toContain("snapshot drift");
  });
});

describe("audit_validator_consumes_curated_specs_without_panicking", () => {
  // Plan §3 Commit 10. Confirms every authored audit-spec file
  // parses as valid JSON, has the required `component` identifier,
  // and (when run against an empty synthetic bundle) produces a
  // usable `ValidationResult` — i.e. the validator's match/branch
  // logic handles every field-combination the specs declare without
  // panicking or falling through a missing branch.
  //
  // This is **NOT** the plan-originally-named
  // `audit_validator_validates_all_6_curated_corpus_representatives_green`
  // contract (review finding F2). That contract — "the specs pass
  // against LIVE audit bundles from a working native build" —
  // requires a NAPI-wired runner and lives in the Commit 13 corpus
  // integration tests (`crates/verter_session/tests/component_meta_audit_corpus/`).
  // This test is narrower by design: it pins the validator's
  // match/branch coverage against empty synthetic bundles so a
  // regression in the validator itself surfaces before the corpus
  // run. The "green against live bundles" assertion is delegated
  // to the corpus suite; see review F2 for the full rationale.
  const specDir = resolve(import.meta.dirname, "../audit-specs/component-meta");
  const expectedSpecFiles = [
    "Accordion.json",
    "Alert.json",
    "App.json",
    "AuthForm.json",
    "Avatar.json",
    "AvatarGroup.json",
  ];

  it("has 6 spec files named after Commit 7 corpus representatives", () => {
    const entries = readdirSync(specDir)
      .filter((n) => n.endsWith(".json"))
      .sort();
    expect(entries).toEqual(expectedSpecFiles);
  });

  it("every spec parses as JSON, carries a non-empty component id, and is consumable by validateAuditBundle", async () => {
    const { readFile } = await import("node:fs/promises");
    for (const specFile of expectedSpecFiles) {
      const raw = await readFile(resolve(specDir, specFile), "utf-8");
      const spec = JSON.parse(raw) as AuditSpec;
      expect(spec.component, `${specFile}: component id must be set`).toBeTruthy();
      expect(spec.component.length, `${specFile}: component id must be non-empty`).toBeGreaterThan(
        0,
      );
      // Running the spec against an empty bundle exercises every
      // declared field's match branch without needing live audit
      // data. A validator regression that panics on a spec shape
      // surfaces here (vs. only surfacing when the CI bench runs).
      const result = validateAuditBundle(emptyBundle(), spec);
      expect(result, `${specFile}: validator returned a usable result`).toMatchObject({
        passed: expect.any(Boolean),
        violations: expect.any(Array),
      });
    }
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

// ── Rule-5 compliance test helpers ────────────────────────────────

type DerivationEdge = {
  result: number;
  kind: string;
  sources: number[];
  meta: unknown;
};

function projectMemberEdge(
  resultId: number,
  memberName: string,
  provenance: string,
): DerivationEdge {
  return {
    result: resultId,
    kind: "ProjectMember",
    sources: [resultId + 100],
    meta: { ProjectMember: { member_name: memberName, provenance } },
  };
}

function projectPathRecord(resultId: number, baseId: number, names: string[]) {
  return {
    result: resultId,
    base: baseId,
    path: names.map((n) => ({ Member: { name: n } })),
  };
}

function projectPathEdge(resultId: number, sourceId: number, names: string[]): DerivationEdge {
  return {
    result: resultId,
    kind: "ProjectPath",
    sources: [sourceId],
    meta: {
      ProjectPath: {
        path: names.map((n) => ({ Member: { name: n } })),
      },
    },
  };
}

function namedSurfaceEntry(name: string): { name: string } {
  return { name };
}

describe("audit_validator_rule5_compliance_published_surface_pass", () => {
  it("passes when every ProjectMember edge names a published surface field", () => {
    // ChatMessages-style scenario: 3 leak-edge candidate names
    // (messages, user, assistant) ALL appear on the published
    // surface, so the gate is vacuously satisfied.
    const bundle = emptyBundle();
    bundle.analysis.props = [
      namedSurfaceEntry("messages"),
      namedSurfaceEntry("user"),
      namedSurfaceEntry("assistant"),
    ];
    bundle.record.footprint!.derivation_subgraph.edges = [
      projectMemberEdge(1, "messages", "MappedKeyEnumerated"),
      projectMemberEdge(2, "user", "MappedKeyEnumerated"),
      projectMemberEdge(3, "assistant", "MappedKeyEnumerated"),
    ] as never;
    const result = validateAuditBundle(bundle, {
      component: "ChatMessages",
      rule5Compliance: { enabled: true },
    });
    expect(result.passed).toBe(true);
    expect(result.violations).toHaveLength(0);
  });
});

describe("audit_validator_rule5_compliance_extends_inherited_legitimate_intermediate", () => {
  it("passes when leak-edge names are subtracted via legitimate-intermediate provenance", () => {
    // Editor-style scenario: members inherited via `extends`
    // chain show up as MappedKeyEnumerated edges (Pick-equivalent
    // structural walk). Names are not on the published surface
    // but the provenance is allowlisted -> subtracted.
    const bundle = emptyBundle();
    bundle.analysis.props = [namedSurfaceEntry("content")];
    bundle.record.footprint!.derivation_subgraph.edges = [
      projectMemberEdge(1, "content", "PublishedField"),
      // EditorOptions members walked via extends — legitimate.
      projectMemberEdge(2, "fontFamily", "MappedKeyEnumerated"),
      projectMemberEdge(3, "lineHeight", "MappedKeyEnumerated"),
    ] as never;
    const result = validateAuditBundle(bundle, {
      component: "Editor",
      rule5Compliance: { enabled: true },
    });
    expect(result.passed).toBe(true);
    expect(result.violations).toHaveLength(0);
  });
});

describe("audit_validator_rule5_compliance_listbox_own_surface_pass", () => {
  it("passes when every edge member-name is in the component's published surface", () => {
    // Listbox-style scenario: every leak-edge name (label, value,
    // multiple, etc.) is published. The gate trivially passes.
    const bundle = emptyBundle();
    const surface = [
      "label",
      "value",
      "multiple",
      "by",
      "disabled",
      "name",
      "size",
      "color",
      "highlight",
      "modelValue",
      "items",
      "trailingIcon",
    ];
    bundle.analysis.props = surface.map(namedSurfaceEntry);
    bundle.record.footprint!.derivation_subgraph.edges = surface.map((n, i) =>
      projectMemberEdge(i + 1, n, "MappedKeyEnumerated"),
    ) as never;
    const result = validateAuditBundle(bundle, {
      component: "Listbox",
      rule5Compliance: { enabled: true },
    });
    expect(result.passed).toBe(true);
    expect(result.violations).toHaveLength(0);
  });
});

describe("audit_validator_rule5_compliance_published_field_must_match_surface", () => {
  it("FAILS when a PublishedField edge names a member outside the published surface", () => {
    // PublishedField provenance is intentionally NOT in the
    // allowlist — it must name a real published-surface field.
    const bundle = emptyBundle();
    bundle.analysis.props = [namedSurfaceEntry("label")];
    bundle.record.footprint!.derivation_subgraph.edges = [
      projectMemberEdge(1, "label", "PublishedField"),
      // This edge claims to publish "phantom" but the surface only
      // has "label" — Rule-5 leak.
      projectMemberEdge(2, "phantom", "PublishedField"),
    ] as never;
    const result = validateAuditBundle(bundle, {
      component: "Listbox",
      rule5Compliance: { enabled: true },
    });
    expect(result.passed).toBe(false);
    expect(result.violations[0]).toContain("phantom@PublishedField");
    expect(result.violations[0]).toContain("rule5Compliance: Listbox");
  });
});

describe("audit_validator_rule5_compliance_path_segments_count_as_legitimate", () => {
  it("subtracts ProjectPath segment names as legitimate intermediates", () => {
    const bundle = emptyBundle();
    bundle.analysis.props = [namedSurfaceEntry("root")];
    // Multi-segment `ProjectPath` edge walking
    // `intermediate.deeper.root`. By edge kind, every Member segment
    // is a structural intermediate — collected with the
    // `"ProjectPath"` sentinel provenance (NOT `null`, which would
    // re-introduce the round-17 false-negative class).
    bundle.record.footprint!.derivation_subgraph.edges = [
      projectPathEdge(1, 100, ["intermediate", "deeper", "root"]),
    ] as never;
    const result = validateAuditBundle(bundle, {
      component: "C",
      rule5Compliance: { enabled: true },
    });
    expect(result.passed).toBe(true);
    expect(result.violations).toHaveLength(0);
  });
});

describe("audit_validator_rule5_compliance_published_field_not_masked_by_fp_projections", () => {
  it("FAILS when a PublishedField edge names an off-surface member even if fp.projections lifts the same name (round-17 codex bug)", () => {
    // Reproducer for the round-17 codex BINDING finding:
    //
    //   `mine_footprint` lifts every `ProjectMember` edge into
    //   `fp.projections` (member-name only, no provenance).
    //   The pre-round-17 validator iterated `fp.projections` and
    //   inserted `(name, provenance: null)` rows that auto-classified
    //   as "legitimate intermediate" — masking the offending
    //   `PublishedField` edge that named the same off-surface name.
    //
    // This test wires both halves of the bug condition and asserts
    // the validator now refuses to mask:
    //
    //   1. A real `PublishedField` edge naming `phantom`, which is
    //      NOT on the published surface (surface = ["label"]).
    //   2. An `fp.projections` record also naming `phantom` (the
    //      lift that used to auto-legitimize via null provenance).
    //
    // POST round-17 fix: `collectAuditMemberEdges` ignores
    // `fp.projections` entirely (every audit-name comes from a
    // typed edge in `derivation_subgraph.edges`), and the validator
    // checks each edge structurally without cross-edge masking.
    const bundle = emptyBundle();
    bundle.analysis.props = [namedSurfaceEntry("label")];
    bundle.record.footprint!.derivation_subgraph.edges = [
      projectMemberEdge(2, "phantom", "PublishedField"),
    ] as never;
    bundle.record.footprint!.projections = [
      // The bug-condition lift: same name appears in fp.projections,
      // which used to give the validator a `null`-provenance
      // legitimacy bucket and mask the PublishedField mismatch.
      projectPathRecord(2, 100, ["phantom"]),
    ] as never;
    const result = validateAuditBundle(bundle, {
      component: "ChatMessages",
      rule5Compliance: { enabled: true },
    });
    expect(result.passed).toBe(false);
    expect(result.violations[0]).toContain("phantom@PublishedField");
    expect(result.violations[0]).toContain("rule5Compliance: ChatMessages");
  });
});

describe("audit_validator_rule5_compliance_published_field_not_masked_by_cross_edge", () => {
  it("FAILS when a PublishedField edge names an off-surface member even if another edge legitimizes the same name (round-17 Claude caveat)", () => {
    // Reproducer for the round-17 Claude live-code caveat:
    //
    //   The pre-round-17 validator aggregated a
    //   `legitimateIntermediateNames` set across ALL edges — if
    //   ANY edge with allowlisted provenance named a member, every
    //   non-allowlisted edge for the same member was masked.
    //
    //   This silently legitimized a mistagged `PublishedField` edge
    //   whose name happened to ALSO appear on a `MappedKeyEnumerated`
    //   edge (e.g. a member walked via both a Pick-equivalent
    //   structural walk and a direct `PublishedField` emit).
    //
    // POST round-17 fix: the validator's check is per-edge
    // structural — `PublishedField` MUST name a published-surface
    // field regardless of what any other edge says about the same
    // name. Cross-edge masking is removed.
    const bundle = emptyBundle();
    bundle.analysis.props = [namedSurfaceEntry("label")];
    bundle.record.footprint!.derivation_subgraph.edges = [
      // Allowlisted edge naming `outputSchema` — pre-fix this used
      // to add `outputSchema` to the legitimateIntermediateNames set
      // and mask the PublishedField mismatch below.
      projectMemberEdge(1, "outputSchema", "MappedKeyEnumerated"),
      // PublishedField edge for the same name. NOT on surface —
      // this is the Rule-5 leak the validator must catch.
      projectMemberEdge(2, "outputSchema", "PublishedField"),
    ] as never;
    const result = validateAuditBundle(bundle, {
      component: "ChatMessages",
      rule5Compliance: { enabled: true },
    });
    expect(result.passed).toBe(false);
    expect(result.violations[0]).toContain("outputSchema@PublishedField");
    expect(result.violations[0]).toContain("rule5Compliance: ChatMessages");
  });
});

describe("audit_validator_rule5_compliance_disabled_short_circuits", () => {
  it("does NOT run the gate when rule5Compliance.enabled is false", () => {
    const bundle = emptyBundle();
    bundle.record.footprint!.derivation_subgraph.edges = [
      projectMemberEdge(1, "leaked", "PublishedField"),
    ] as never;
    // No `rule5Compliance` spec at all — the gate is silent.
    expect(validateAuditBundle(bundle, { component: "C" }).passed).toBe(true);
    // Explicit `enabled: false` — gate is silent.
    expect(
      validateAuditBundle(bundle, {
        component: "C",
        rule5Compliance: { enabled: false },
      }).passed,
    ).toBe(true);
  });
});
