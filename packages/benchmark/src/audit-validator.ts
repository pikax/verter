/**
 * Audit validator — plan §3 Commit 10 / F8.
 *
 * Validates a component-meta audit bundle against a declarative
 * specification. The validator replaces the legacy
 * `trace-validator.ts` regex-driven checker; both that file and its
 * pinned `trace-specs/component-meta/*.json` specs have been deleted
 * from the repo. The audit record is now the sole correctness
 * authority.
 *
 * ## Supported checks (legacy-parity)
 *
 * | Legacy TraceSpec field     | Audit-spec field                | Source in RustAuditRecord      |
 * | -------------------------- | ------------------------------- | ------------------------------ |
 * | required (file loaded)     | requireLoadedFiles              | footprint.loadedFiles()        |
 * | forbidden (file loaded)    | forbidLoadedFiles               | footprint.loadedFiles()        |
 * | required (instantiation)   | requireInstantiations           | footprint.instantiations       |
 * | forbidden (instantiation)  | forbidInstantiations            | footprint.instantiations       |
 * | maxCounts                  | maxCounts                       | footprint record-vector sizes  |
 * | maxDurations               | maxDurations                    | footprint.materializations[*]  |
 * | maxTotalDurationMs         | totalDurationMsMax              | timings.total_ms               |
 * | expectedResult.minProps    | expectedResult.minProps         | accompanying ComponentMeta     |
 * | expectedResult.minEvents   | expectedResult.minEvents        | accompanying ComponentMeta     |
 * | expectedResult.minSlots    | expectedResult.minSlots         | accompanying ComponentMeta     |
 * | (new)                      | expectedFootprintSnapshot       | pinned footprint JSON          |
 */

import type {
  ComponentMetaFlags,
  MaterializationRecord,
  RustAuditRecord,
} from "@verter/types/audit.generated";

/**
 * Shape of the analysis side-car the validator checks against
 * `expectedResult`. The FFI projection `FfiComponentMeta` uses
 * camelCase — we accept a structural subset with `unknown`-typed
 * escape hatches for flags / extras.
 */
export interface AnalysisSideCar {
  props?: unknown[];
  events?: unknown[];
  slots?: unknown[];
  flags?: Partial<ComponentMetaFlags> & Record<string, unknown>;
}

/** Audit bundle as emitted by `ComponentMetaSession.getComponentMetaWithAudit`. */
export interface AuditBundle {
  analysis: AnalysisSideCar;
  resolution: unknown;
  record: RustAuditRecord;
}

export interface RequireInstantiationSpec {
  declCanonicalId: string;
  declSymbolName: string;
  /** Matched against args fingerprint hex; optional (any args if omitted). */
  argsFingerprintHex?: string;
}

export interface ExpectedResultSpec {
  minProps?: number;
  minEvents?: number;
  minSlots?: number;
  hasEvaluatedTypes?: boolean;
}

export interface AuditSpec {
  /** Component name (e.g. "Accordion") for log output. */
  component: string;
  /** File that must appear in the request's `loaded_files()`. */
  requireLoadedFiles?: string[];
  /** File that must NOT appear in `loaded_files()`. */
  forbidLoadedFiles?: string[];
  /** Instantiation identities that must be recorded. */
  requireInstantiations?: RequireInstantiationSpec[];
  /** Instantiation identities that must NOT be recorded. */
  forbidInstantiations?: RequireInstantiationSpec[];
  /** Cap on per-record-kind counts (`instantiations`, `projections`, ...). */
  maxCounts?: Partial<Record<AuditCountableKind, number>>;
  /** Cap on per-subject materialization durations in ms. */
  maxDurations?: Array<{ subjectContains: string; maxDurationMs: number }>;
  /** Cap on the request's total wall-clock (`timings.total_ms`). */
  totalDurationMsMax?: number;
  /** Minimum-shape assertions against the accompanying ComponentMetaAnalysis. */
  expectedResult?: ExpectedResultSpec;
  /**
   * Optional pinned footprint snapshot (rendered JSON). Compared
   * string-equal against the live footprint after
   * `mask_incidental_spans()`-style normalization performed on the
   * Rust side. Plan §3 Commit 10 future-facing check.
   */
  expectedFootprintSnapshot?: string;
}

export type AuditCountableKind =
  | "indexedReadyBuilds"
  | "vfsReads"
  | "sharedLoadReuses"
  | "instantiations"
  | "projections"
  | "conditionalDecisions"
  | "substitutions"
  | "aliasResolutions"
  | "materializations";

export interface ValidationResult {
  passed: boolean;
  violations: string[];
}

/**
 * Main validator. Returns a `ValidationResult` with every violation
 * collected — callers can decide whether to exit-nonzero on the
 * presence of any violation, or to report-and-continue.
 */
export function validateAuditBundle(bundle: AuditBundle, spec: AuditSpec): ValidationResult {
  const violations: string[] = [];
  const record = bundle.record;
  const fp = record.footprint;

  if (spec.requireLoadedFiles?.length) {
    const loaded = fp ? loadedFilesOf(fp) : [];
    for (const required of spec.requireLoadedFiles) {
      if (!loaded.includes(required)) {
        violations.push(
          `requireLoadedFiles: ${spec.component} — expected ${required} in loaded_files, got [${loaded.join(", ")}]`,
        );
      }
    }
  }

  if (spec.forbidLoadedFiles?.length) {
    const loaded = fp ? loadedFilesOf(fp) : [];
    for (const forbidden of spec.forbidLoadedFiles) {
      if (loaded.includes(forbidden)) {
        violations.push(
          `forbidLoadedFiles: ${spec.component} — ${forbidden} must NOT be in loaded_files`,
        );
      }
    }
  }

  if (spec.requireInstantiations?.length) {
    const insts = fp?.instantiations ?? [];
    for (const req of spec.requireInstantiations) {
      const found = insts.some((i) => matchesInstantiation(i, req));
      if (!found) {
        violations.push(
          `requireInstantiations: ${spec.component} — expected instantiation of ` +
            `${req.declSymbolName} from ${req.declCanonicalId}${
              req.argsFingerprintHex ? ` (args=${req.argsFingerprintHex})` : ""
            } — none found`,
        );
      }
    }
  }

  if (spec.forbidInstantiations?.length) {
    const insts = fp?.instantiations ?? [];
    for (const forbidden of spec.forbidInstantiations) {
      const found = insts.some((i) => matchesInstantiation(i, forbidden));
      if (found) {
        violations.push(
          `forbidInstantiations: ${spec.component} — must NOT instantiate ` +
            `${forbidden.declSymbolName} from ${forbidden.declCanonicalId}`,
        );
      }
    }
  }

  if (spec.maxCounts) {
    for (const [kind, cap] of Object.entries(spec.maxCounts) as Array<
      [AuditCountableKind, number]
    >) {
      const actual = countForKind(fp, kind);
      if (actual > cap) {
        violations.push(`maxCounts.${kind}: ${spec.component} — expected ≤${cap}, got ${actual}`);
      }
    }
  }

  if (spec.maxDurations?.length) {
    const mats: MaterializationRecord[] = fp?.materializations ?? [];
    for (const rule of spec.maxDurations) {
      for (const m of mats) {
        const subject = describeMaterializationSubject(m);
        if (subject.includes(rule.subjectContains) && m.duration_ms > rule.maxDurationMs) {
          violations.push(
            `maxDurations: ${spec.component} — ${subject} took ${m.duration_ms.toFixed(
              2,
            )}ms, expected ≤${rule.maxDurationMs}ms`,
          );
        }
      }
    }
  }

  if (spec.totalDurationMsMax !== undefined && record.timings.total_ms > spec.totalDurationMsMax) {
    violations.push(
      `totalDurationMsMax: ${spec.component} — total ${record.timings.total_ms.toFixed(2)}ms, ` +
        `expected ≤${spec.totalDurationMsMax}ms`,
    );
  }

  if (spec.expectedResult) {
    validateExpectedResult(spec.component, bundle.analysis, spec.expectedResult, violations);
  }

  if (spec.expectedFootprintSnapshot !== undefined) {
    const rendered = renderFootprintForSnapshot(fp);
    if (rendered !== spec.expectedFootprintSnapshot) {
      violations.push(
        `expectedFootprintSnapshot: ${spec.component} — snapshot drift. ` +
          `Rerun the snapshot refresh (see docs/audit-footprint/README.md).`,
      );
    }
  }

  return { passed: violations.length === 0, violations };
}

function loadedFilesOf(fp: NonNullable<RustAuditRecord["footprint"]>): string[] {
  const set = new Set<string>();
  for (const r of fp.vfs_reads) set.add(r.canonical_id);
  for (const r of fp.shared_load_reuses) set.add(r.canonical_id);
  return Array.from(set).sort();
}

function matchesInstantiation(
  inst: {
    decl_canonical_id: string;
    decl_symbol_name: string;
    args_fingerprint: readonly number[];
  },
  req: RequireInstantiationSpec,
): boolean {
  if (inst.decl_canonical_id !== req.declCanonicalId) return false;
  if (inst.decl_symbol_name !== req.declSymbolName) return false;
  if (req.argsFingerprintHex) {
    const hex = argsFingerprintToHex(inst.args_fingerprint);
    if (hex !== req.argsFingerprintHex.toLowerCase()) return false;
  }
  return true;
}

// `Hash16` = `[u8; 16]` on the Rust side; ts-rs emits a 16-tuple of
// `number` in the generated TS surface. Never a string on the wire —
// callers supplying hex strings use `argsFingerprintHex` on the spec,
// which is handled separately above.
function argsFingerprintToHex(fp: readonly number[]): string {
  return Array.from(fp)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

function countForKind(fp: RustAuditRecord["footprint"], kind: AuditCountableKind): number {
  if (!fp) return 0;
  switch (kind) {
    case "indexedReadyBuilds":
      return fp.indexed_ready_builds.length;
    case "vfsReads":
      return fp.vfs_reads.length;
    case "sharedLoadReuses":
      return fp.shared_load_reuses.length;
    case "instantiations":
      return fp.instantiations.length;
    case "projections":
      return fp.projections.length;
    case "conditionalDecisions":
      return fp.conditional_decisions.length;
    case "substitutions":
      return fp.substitutions.length;
    case "aliasResolutions":
      return fp.alias_resolutions.length;
    case "materializations":
      return fp.materializations.length;
  }
}

function describeMaterializationSubject(m: MaterializationRecord): string {
  const s = m.subject;
  if (typeof s === "object" && s !== null) {
    if ("MemberRoute" in s) return `MemberRoute(${s.MemberRoute.owner}#${s.MemberRoute.member})`;
    if ("PublicPropType" in s)
      return `PublicPropType(${s.PublicPropType.owner}#${s.PublicPropType.prop})`;
    if ("DefinePropsMember" in s)
      return `DefinePropsMember(${s.DefinePropsMember.owner}#${s.DefinePropsMember.member})`;
    if ("FallthroughInheritance" in s)
      return `FallthroughInheritance(${s.FallthroughInheritance.owner})`;
  }
  return JSON.stringify(s);
}

function validateExpectedResult(
  component: string,
  analysis: AnalysisSideCar,
  expected: ExpectedResultSpec,
  violations: string[],
): void {
  if (expected.minProps !== undefined) {
    const n = analysis.props?.length ?? 0;
    if (n < expected.minProps) {
      violations.push(
        `expectedResult.minProps: ${component} — expected ≥${expected.minProps}, got ${n}`,
      );
    }
  }
  if (expected.minEvents !== undefined) {
    const n = analysis.events?.length ?? 0;
    if (n < expected.minEvents) {
      violations.push(
        `expectedResult.minEvents: ${component} — expected ≥${expected.minEvents}, got ${n}`,
      );
    }
  }
  if (expected.minSlots !== undefined) {
    const n = analysis.slots?.length ?? 0;
    if (n < expected.minSlots) {
      violations.push(
        `expectedResult.minSlots: ${component} — expected ≥${expected.minSlots}, got ${n}`,
      );
    }
  }
  if (expected.hasEvaluatedTypes !== undefined) {
    const actual = analysis.flags?.has_evaluated_types === true;
    if (expected.hasEvaluatedTypes !== actual) {
      violations.push(
        `expectedResult.hasEvaluatedTypes: ${component} — expected ${expected.hasEvaluatedTypes}, ` +
          `got ${actual}`,
      );
    }
  }
}

function renderFootprintForSnapshot(fp: RustAuditRecord["footprint"]): string {
  // Normalized JSON for snapshot comparison. Strips VFS reads (they
  // are incidental to cache warmth and would flap across runs) and
  // the derivation subgraph edge metadata (kept as a count instead).
  // This mirrors the Rust-side `mask_incidental_spans()` philosophy.
  if (!fp) return "null";
  const normalized = {
    indexed_ready_builds: fp.indexed_ready_builds.map((r) => r.canonical_id).sort(),
    shared_load_reuses: fp.shared_load_reuses.map((r) => r.canonical_id).sort(),
    instantiations: fp.instantiations.length,
    projections: fp.projections.length,
    conditional_decisions: fp.conditional_decisions.length,
    substitutions: fp.substitutions.length,
    alias_resolutions: fp.alias_resolutions.length,
    materializations: fp.materializations.length,
    cache_outcomes: fp.cache_outcomes,
    derivation_nodes: fp.derivation_subgraph.nodes.length,
    derivation_edges: fp.derivation_subgraph.edges.length,
  };
  return JSON.stringify(normalized, null, 2);
}
