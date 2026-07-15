/// <reference types="node" />

/**
 * TypeScript helpers for the component-meta audit surface exposed by
 * `@verter/native` via `ComponentMetaSession.getComponentMetaWithAudit`
 * and the Rust-walker bindings `whyLoadedFromAuditJson` /
 * `whyInstantiatedFromAuditJson`.
 *
 * The walker itself lives in Rust; this file only parses JSON
 * round-trip results and renders them for human readers.
 *
 * The Wave-3 typed entry-points exposed on `VerterHost`
 * (`compileWithAudit`, `analyzeWithAudit`, `resolveTypeWithAudit`,
 * `auditWorkspaceOp`, `getLastAuditRecord`, `getAuditRecords`,
 * `getBundlerBatchSummary`) return JSON Buffers carrying the full
 * `RequestAuditRecord` (or a list / `RustBundlerBatchPayload`). The
 * decoders below read those buffers into the matching types.
 */

import type {
  BundlerBatchPayload,
  ChainTermination,
  ProvenanceChain,
  ProvenanceStep,
  RequestAuditRecord,
  RequestFootprintAudit,
} from "@verter/types/audit.generated";

import type { ComponentMetaSession, VerterHost } from "./index";

/**
 * Backward-compatible aliases for the older `Rust*` names. Earlier
 * Wave-1 fixtures and downstream consumers used the prefixed names;
 * the ts-rs–generated bindings emit the unprefixed `RequestAudit*`
 * shape directly. The aliases keep import paths stable while the
 * fixtures are migrated.
 */
export type RustAuditRecord = RequestAuditRecord;
export type RustSemanticFootprintAudit = RequestFootprintAudit;

/**
 * JSON-shaped audit bundle returned by
 * `ComponentMetaSession.getComponentMetaWithAudit`. Mirrors the Rust
 * `AuditBundle`. `analysis` and `resolution` are
 * FFI projections; `record` is the full ts-rs–generated record.
 */
export interface AuditBundle {
  analysis: unknown;
  resolution: unknown;
  record: RequestAuditRecord;
}

/**
 * Decode the Buffer returned by
 * `ComponentMetaSession.getComponentMetaWithAudit` into a typed
 * `AuditBundle`. Returns `null` when the session's query was
 * unresolvable.
 */
export function decodeAuditBundle(buffer: Buffer | null): AuditBundle | null {
  if (buffer === null) return null;
  const text = buffer.toString("utf-8");
  return JSON.parse(text) as AuditBundle;
}

/**
 * Ask "why was this file loaded during this audited request?".
 * Delegates to the Rust walker via the `ComponentMetaSession`'s
 * `whyLoadedFromAuditJson` binding so the traversal logic stays in
 * one place (single-walker rule).
 */
export function whyLoaded(
  session: ComponentMetaSession,
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
 * `argsFingerprintHex` is the 32-char lowercase hex rendering of the
 * 16-byte `Hash16`.
 */
export function whyInstantiated(
  session: ComponentMetaSession,
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

/**
 * Assert (set-equality) that `loaded_files()` — the files the
 * scheduler actually read on behalf of this request — equals
 * `expected`.
 *
 * Mirrors the Rust-side `RequestAuditRecord::assert_loaded_files_exactly`
 * helper; throws with a unified-diff style message on mismatch so the
 * failure renders the symmetric difference clearly.
 */
export function assertLoadedFilesExactly(record: RequestAuditRecord, expected: string[]): void {
  const actual = loadedFiles(record.footprint ?? null);
  const missing = expected.filter((f) => !actual.includes(f)).sort();
  const extra = actual.filter((f) => !expected.includes(f)).sort();
  if (missing.length === 0 && extra.length === 0) return;
  const lines = ["loaded_files set mismatch:"];
  for (const m of missing) lines.push(`  + ${m} (expected, missing from actual)`);
  for (const e of extra) lines.push(`  - ${e} (actual, not expected)`);
  throw new Error(lines.join("\n"));
}

/**
 * Assert (set-equality) that `declared_dependency_files()` — the
 * broader "this request's dependency closure" set (`vfs_reads ∪
 * shared_load_reuses ∪ indexed_ready_builds`) — equals `expected`.
 *
 * Use this when the fixture's intent is "these files appeared in the
 * request's dependency graph", which is a distinct claim from
 * `loaded_files()`'s "these files were actually read by the
 * scheduler".
 */
export function assertDeclaredDependencyFilesExactly(
  record: RequestAuditRecord,
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

/**
 * Compute the loaded-files set (VFS reads + shared-load reuses) for a
 * given footprint. Mirrors the Rust-side `loaded_files()` helper
 * exactly. Returns a sorted deduplicated array.
 */
export function loadedFiles(footprint: RustSemanticFootprintAudit | null): string[] {
  if (!footprint) return [];
  const set = new Set<string>();
  for (const r of footprint.vfs_reads) set.add(r.canonical_id);
  for (const r of footprint.shared_load_reuses) set.add(r.canonical_id);
  return Array.from(set).sort();
}

/**
 * Compute the declared-dependency-files set (`vfs_reads ∪
 * shared_load_reuses ∪ indexed_ready_builds`) for a given footprint.
 * Mirrors the Rust-side `declared_dependency_files()` helper exactly.
 * Returns a sorted deduplicated array.
 */
export function declaredDependencyFiles(footprint: RustSemanticFootprintAudit | null): string[] {
  if (!footprint) return [];
  const set = new Set<string>();
  for (const r of footprint.vfs_reads) set.add(r.canonical_id);
  for (const r of footprint.shared_load_reuses) set.add(r.canonical_id);
  for (const r of footprint.indexed_ready_builds) set.add(r.canonical_id);
  return Array.from(set).sort();
}

/**
 * Render a `ProvenanceChain` as plain text — a tiny indented list of
 * edges with the termination reason. Pure formatting; does NOT walk
 * the graph — the walker lives in Rust; TS callers render
 * results.
 */
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
