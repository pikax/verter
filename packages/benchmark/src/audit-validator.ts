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
 * | Legacy TraceSpec field     | Audit-spec field                | Source in RequestAuditRecord      |
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
  MaterializationRecord,
  MemberEdgeProvenance,
  OriginEdgeMetaDto,
  ProjectPathSegment,
  RequestAuditRecord,
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
  /** Subset of analysis exposed entries (`exposed: [{ name: ... }, ...]`). */
  exposed?: unknown[];
  flags?: Record<string, unknown>;
}

/** Audit bundle as emitted by `ComponentMetaSession.getComponentMetaWithAudit`. */
export interface AuditBundle {
  analysis: AnalysisSideCar;
  resolution: unknown;
  record: RequestAuditRecord;
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

/**
 * Rule-5 compliance spec — gates that every `ProjectMember` audit edge
 * either names a field on the user-visible published surface OR carries
 * a [`MemberEdgeProvenance`] variant in the `allowedIntermediateProvenance`
 * allowlist (a legitimate intermediate of a structural walk).
 *
 * The gate is a binary subset check (no ratios, no thresholds):
 *
 *   `audit_member_names \ legitimate_intermediate_names ⊆ published_surface_names`
 *
 * where `legitimate_intermediate_names` is the union of edge member
 * names whose provenance ∈ `allowedIntermediateProvenance`. Names
 * outside both sets indicate a Rule-5 leak — a future producer change
 * that broadens the audit footprint past the user's declared surface.
 *
 * The default allowlist is the closure of legitimate-structural
 * provenance variants set at Verter's current emit sites (KeyOf
 * enumeration, Mapped instantiation, multi-hop path projection).
 * `PublishedField` is intentionally NOT in the allowlist — edges with
 * that provenance MUST name a published-surface field.
 */
export interface Rule5ComplianceSpec {
  enabled: boolean;
  /**
   * Provenance variants whose member-name edges are subtracted as
   * legitimate structural intermediates. Defaults to
   * `["PathProjection", "KeyOfEnumerated", "MappedKeyEnumerated"]` —
   * see [`MemberEdgeProvenance`] for the per-variant rationale.
   */
  allowedIntermediateProvenance?: MemberEdgeProvenance[];
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
  /**
   * Rule-5 compliance check. When `enabled: true`, the validator
   * gates that every `ProjectMember` audit edge member-name appears
   * on the user-visible published surface OR is emitted with a
   * structurally-legitimate provenance variant.
   */
  rule5Compliance?: Rule5ComplianceSpec;
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

  if (spec.rule5Compliance?.enabled === true) {
    validateRule5Compliance(spec.component, bundle, spec.rule5Compliance, violations);
  }

  return { passed: violations.length === 0, violations };
}

/**
 * Default allowlist of legitimate-intermediate provenance variants.
 * Each variant names a structural operation Verter's resolver performs
 * during type resolution; their member-name edges are NOT leaks even
 * when the names fall outside the published surface (e.g. a
 * `Pick<Foo, "a">` walk emits a `MappedKeyEnumerated` edge for "a"
 * which is legitimate by construction).
 *
 * `PublishedField` is intentionally NOT in this list: any edge tagged
 * `PublishedField` MUST name a published-surface field, and the
 * Rule-5 gate enforces that constraint.
 */
const DEFAULT_ALLOWED_INTERMEDIATE_PROVENANCE: ReadonlyArray<MemberEdgeProvenance> = [
  "PathProjection",
  "KeyOfEnumerated",
  "MappedKeyEnumerated",
];

interface AuditMemberEdge {
  memberName: string;
  /**
   * Provenance carried on the edge. Either a typed
   * [`MemberEdgeProvenance`] (from a single-hop `ProjectMember` edge)
   * or the `"ProjectPath"` sentinel (from a multi-segment
   * `ProjectPath` edge — the segment is legitimate by edge-kind
   * alone, no typed per-segment provenance exists). `null` is
   * intentionally NOT a value here — every audit-name carries an
   * explicit, source-of-truth provenance bucket so the validator
   * cannot silently auto-legitimize an off-surface leak.
   */
  provenance: MemberEdgeProvenance | "ProjectPath";
}

/**
 * Collect every member-name reference in the audit footprint:
 *   - Single-hop `OriginEdgeMetaDto::ProjectMember` edges contribute
 *     `(member_name, provenance)`.
 *   - Multi-segment `OriginEdgeMetaDto::ProjectPath` edges contribute
 *     `(segment.name, null)` for every `Member` segment in the path.
 *
 * `ProjectPath` segments do not carry per-segment provenance (the whole
 * path is the structural operation). For Rule-5 purposes, every
 * `ProjectPath` segment counts as a structurally legitimate intermediate
 * (the path is the user's declared projection by definition).
 */
function collectAuditMemberEdges(
  fp: NonNullable<RequestAuditRecord["footprint"]>,
): AuditMemberEdge[] {
  const edges: AuditMemberEdge[] = [];
  for (const edge of fp.derivation_subgraph.edges) {
    const meta = edge.meta as OriginEdgeMetaDto;
    if (typeof meta !== "object" || meta === null) continue;
    if ("ProjectMember" in meta) {
      const pm = meta.ProjectMember;
      edges.push({
        memberName: pm.member_name,
        provenance: pm.provenance,
      });
    } else if ("ProjectPath" in meta) {
      // Multi-segment path-walk — every Member segment is a
      // structurally legitimate intermediate by edge-kind alone.
      // We tag each name with the `"ProjectPath"` sentinel so the
      // validator never sees a `null`-provenance bucket (the
      // round-17 false-negative class).
      for (const seg of meta.ProjectPath.path as ProjectPathSegment[]) {
        if (typeof seg === "object" && seg !== null && "Member" in seg) {
          edges.push({ memberName: seg.Member.name, provenance: "ProjectPath" });
        }
      }
    }
  }
  return edges;
}

function collectPublishedSurfaceNames(analysis: AnalysisSideCar): Set<string> {
  const names = new Set<string>();
  const harvest = (xs?: unknown[]) => {
    if (!xs) return;
    for (const item of xs) {
      if (typeof item === "object" && item !== null) {
        const name = (item as { name?: unknown }).name;
        if (typeof name === "string") {
          names.add(name);
        }
      }
    }
  };
  harvest(analysis.props);
  harvest(analysis.events);
  harvest(analysis.slots);
  harvest(analysis.exposed);
  return names;
}

function validateRule5Compliance(
  component: string,
  bundle: AuditBundle,
  spec: Rule5ComplianceSpec,
  violations: string[],
): void {
  const fp = bundle.record.footprint;
  if (!fp) return;
  const allowlist = new Set<MemberEdgeProvenance>(
    spec.allowedIntermediateProvenance ?? DEFAULT_ALLOWED_INTERMEDIATE_PROVENANCE,
  );
  const publishedNames = collectPublishedSurfaceNames(bundle.analysis);
  const edges = collectAuditMemberEdges(fp);

  // Per-edge structural classification — no cross-edge masking. An
  // edge's legitimacy depends ONLY on its own provenance bucket and
  // (for non-allowlisted typed provenance, e.g. `PublishedField`)
  // whether its member-name is on the component's published surface.
  //
  //   - `"ProjectPath"`  sentinel → always legitimate (multi-segment
  //                                 structural walk; intermediate by
  //                                 edge kind).
  //   - typed ∈ allowlist          → always legitimate (the path-walk
  //                                 / keyspace-fan-out / mapped-key
  //                                 emission is structural by kind).
  //   - typed ∉ allowlist          → MUST name a published-surface
  //     (e.g. `PublishedField`)    field; otherwise it is a Rule-5
  //                                 leak. No other edge's allowlisted
  //                                 provenance can mask this — the
  //                                 PublishedField claim itself is
  //                                 the contract.
  const offending: Array<{
    name: string;
    provenance: MemberEdgeProvenance;
  }> = [];
  for (const edge of edges) {
    if (edge.provenance === "ProjectPath") continue;
    if (allowlist.has(edge.provenance)) continue;
    if (publishedNames.has(edge.memberName)) continue;
    if (offending.some((o) => o.name === edge.memberName && o.provenance === edge.provenance)) {
      continue;
    }
    offending.push({ name: edge.memberName, provenance: edge.provenance });
  }

  if (offending.length > 0) {
    const offendersStr = offending.map((o) => `${o.name}@${o.provenance}`).join(", ");
    violations.push(
      `rule5Compliance: ${component} — audit edges name members outside the ` +
        `published surface and outside the structural-intermediate allowlist ` +
        `[${Array.from(allowlist).join(", ")}]. ` +
        `Offending edges: [${offendersStr}]. ` +
        `Published surface: [${Array.from(publishedNames).sort().join(", ")}].`,
    );
  }
}

function loadedFilesOf(fp: NonNullable<RequestAuditRecord["footprint"]>): string[] {
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

function countForKind(fp: RequestAuditRecord["footprint"], kind: AuditCountableKind): number {
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

/**
 * Render the audit footprint as a deterministic, snapshot-friendly
 * string. Exported so tests can pin exact-match behaviour without
 * copy-pasting the normalization schema. Plan §3 Commit 10 + review F11.
 */
export function renderFootprintForSnapshot(fp: RequestAuditRecord["footprint"]): string {
  // Normalized JSON for snapshot comparison. Intentionally diverges
  // from the Rust-side `mask_incidental_spans` helper: Rust only
  // clears the fields enumerated by the `IncidentalFields` trait
  // implementation on `RustSemanticFootprintAudit` (today:
  // `vfs_reads`) and keeps every other record-vector as-is. This TS
  // form collapses per-record lists to counts so snapshots are
  // readable in a PR diff without flapping on subgraph detail —
  // useful for bench specs, unsuitable when the exact record list
  // matters. Use the Rust helper + `RequestAuditRecord::emit_json`
  // when you need the full structural payload. Plan §3 Commit 10 +
  // review F9.
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
