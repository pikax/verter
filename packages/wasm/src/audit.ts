/**
 * TypeScript helpers for the component-meta audit surface exposed by
 * `@verter/wasm` via `MetaSession.getComponentMetaWithAudit` and the
 * Rust-walker bindings `whyLoadedFromAuditJson` /
 * `whyInstantiatedFromAuditJson`.
 *
 * Mirror of `packages/native/audit.ts` (plan §3 Commit 8). The file
 * lives under `src/` to match this package's existing TS layout
 * (`tsdown.config.ts` entry = `src/index.ts`).
 *
 * These helpers are pure formatting / tiny convenience wrappers —
 * they never re-implement the walker. The walk itself lives in Rust;
 * this file only parses JSON round-trip results and renders them for
 * human readers.
 */

import type {
  ChainTermination,
  ProvenanceChain,
  ProvenanceStep,
  RustAuditRecord,
  RustSemanticFootprintAudit,
} from "@verter/types/audit.generated";

/**
 * Minimal structural type for the wasm-bindgen–generated
 * `MetaSession` class the walker bindings hang off. We keep this
 * interface local to avoid a load-time dependency on the generated
 * types — callers pass the concrete `MetaSession` instance they got
 * from `new MetaSession(...)` (or whatever constructor pattern the
 * wasm package exposes).
 */
export interface AuditCapableMetaSession {
  getComponentMetaWithAudit(canonicalOrAlias: string): unknown | null;
  whyLoadedFromAuditJson(auditJson: string, canonicalId: string): string;
  whyInstantiatedFromAuditJson(
    auditJson: string,
    declCanonicalId: string,
    declSymbolName: string,
    argsFingerprintHex: string,
  ): string;
}

/** JSON-shaped audit bundle — mirror of the NAPI `AuditBundle`. */
export interface AuditBundle {
  analysis: unknown;
  resolution: unknown;
  record: RustAuditRecord;
}

/**
 * Cast the raw `JsValue` returned by
 * `MetaSession.getComponentMetaWithAudit` to a typed `AuditBundle`.
 * Returns `null` when the session's query was unresolvable.
 */
export function decodeAuditBundle(value: unknown): AuditBundle | null {
  if (value === null || value === undefined) return null;
  return value as AuditBundle;
}

/**
 * Ask "why was this file loaded during this audited request?".
 * Delegates to the Rust walker via the `MetaSession`'s
 * `whyLoadedFromAuditJson` binding (plan §2.8 single-walker rule).
 */
export function whyLoaded(
  session: AuditCapableMetaSession,
  bundle: AuditBundle,
  canonicalId: string,
): ProvenanceChain {
  const auditJson = JSON.stringify(bundle);
  const chainJson = session.whyLoadedFromAuditJson(auditJson, canonicalId);
  return JSON.parse(chainJson) as ProvenanceChain;
}

/**
 * Ask "why was this type instantiated during this audited request?".
 * Keyed by `(declCanonicalId, declSymbolName, argsFingerprintHex)`.
 */
export function whyInstantiated(
  session: AuditCapableMetaSession,
  bundle: AuditBundle,
  declCanonicalId: string,
  declSymbolName: string,
  argsFingerprintHex: string,
): ProvenanceChain {
  const auditJson = JSON.stringify(bundle);
  const chainJson = session.whyInstantiatedFromAuditJson(
    auditJson,
    declCanonicalId,
    declSymbolName,
    argsFingerprintHex,
  );
  return JSON.parse(chainJson) as ProvenanceChain;
}

/** Set-equality assertion mirroring the native helper. Plan §1.4. */
export function assertLoadedFilesExactly(record: RustAuditRecord, expected: string[]): void {
  const actual = loadedFiles(record.footprint ?? null);
  const missing = expected.filter((f) => !actual.includes(f)).sort();
  const extra = actual.filter((f) => !expected.includes(f)).sort();
  if (missing.length === 0 && extra.length === 0) return;
  const lines = ["loaded_files set mismatch:"];
  for (const m of missing) lines.push(`  + ${m} (expected, missing from actual)`);
  for (const e of extra) lines.push(`  - ${e} (actual, not expected)`);
  throw new Error(lines.join("\n"));
}

/** Set-equality assertion for the broader dependency set. Plan §3.B Commit 7.B. */
export function assertDeclaredDependencyFilesExactly(
  record: RustAuditRecord,
  expected: string[],
): void {
  const actual = declaredDependencyFiles(record.footprint ?? null);
  const missing = expected.filter((f) => !actual.includes(f)).sort();
  const extra = actual.filter((f) => !expected.includes(f)).sort();
  if (missing.length === 0 && extra.length === 0) return;
  const lines = ["declared_dependency_files set mismatch:"];
  for (const m of missing) lines.push(`  + ${m} (expected, missing from actual)`);
  for (const e of extra) lines.push(`  - ${e} (actual, not expected)`);
  throw new Error(lines.join("\n"));
}

/** Sorted dedup of `vfs_reads ∪ shared_load_reuses` canonical ids. */
export function loadedFiles(footprint: RustSemanticFootprintAudit | null): string[] {
  if (!footprint) return [];
  const set = new Set<string>();
  for (const r of footprint.vfs_reads) set.add(r.canonical_id);
  for (const r of footprint.shared_load_reuses) set.add(r.canonical_id);
  return Array.from(set).sort();
}

/** Sorted dedup of all three dependency lanes. */
export function declaredDependencyFiles(footprint: RustSemanticFootprintAudit | null): string[] {
  if (!footprint) return [];
  const set = new Set<string>();
  for (const r of footprint.vfs_reads) set.add(r.canonical_id);
  for (const r of footprint.shared_load_reuses) set.add(r.canonical_id);
  for (const r of footprint.indexed_ready_builds) set.add(r.canonical_id);
  return Array.from(set).sort();
}

/** Render a `ProvenanceChain` as plain text. Pure formatting. */
export function renderChainText(chain: ProvenanceChain): string {
  const lines: string[] = [];
  if (chain.root === null) {
    lines.push("(no root found in audit record)");
  } else {
    lines.push(`root: NodeId(${chain.root.toString()})`);
  }
  for (const step of chain.steps) {
    lines.push(renderStep(step));
  }
  lines.push(renderTermination(chain.terminated));
  for (const t of chain.shared_load_terminals) {
    lines.push(
      `  [shared-load] ${t.canonical_id} (winner_request_id=${t.winner_request_id}, winner_audited=${t.winner_audited})`,
    );
  }
  return lines.join("\n");
}

function renderStep(step: ProvenanceStep): string {
  const indent = "  ".repeat(step.depth + 1);
  return `${indent}← edge #${step.edge_id.toString()} ${step.node_label} (kind=${step.edge.kind})`;
}

function renderTermination(t: ChainTermination): string {
  if (t === "Complete") return "  terminated: Complete";
  if (t === "NotFound") return "  terminated: NotFound";
  if (typeof t === "object" && t !== null) {
    if ("DepthExceeded" in t) {
      return `  terminated: DepthExceeded { cap: ${t.DepthExceeded.cap} }`;
    }
    if ("Cycle" in t) {
      return `  terminated: Cycle { at_edge: EdgeId(${t.Cycle.at_edge.toString()}) }`;
    }
  }
  return `  terminated: ${JSON.stringify(t)}`;
}
