import type { TypeDescriptor } from "@verter/type-ir";

/**
 * Configuration for a {@link TypeInfoSession}.
 */
export interface TypeInfoSessionConfig {
  /** Project root canonical path. */
  root: string;
  /** Optional explicit `tsconfig.json` path; defaults to host probe. */
  tsconfig?: string;
  /** Workspace folder canonical paths. */
  workspaceFolders?: string[];
  /**
   * Enable per-request audit. Default `true`. When `false`, or when
   * the loaded `@verter/native` binding was built without audit capture
   * (`captureAvailability() === "Unavailable"`), the `auditRecord`
   * field on `ResolveSymbolResult` is
   * `undefined` (the resolution still runs).
   */
  auditEnabled?: boolean;
}

/**
 * Symbol-inventory entry returned by {@link TypeInfoSession.listSymbols}.
 *
 * Mirror of the host's `SymbolEntry` DTO. The `kind` discriminator
 * uses camelCase tags matching the FFI mirror in
 * `verter_protocol::typeinfo`.
 */
export interface SymbolEntry {
  name: string;
  kind: SymbolKind;
  /** The unit of {@link SymbolEntry.span}: UTF-8 byte offsets (`utf8.v1`). */
  spanEncoding: "utf8.v1";
  /** SFC-absolute span. `undefined` when no analysis-snapshot span was available. */
  span?: { start: number; end: number };
  isExported: boolean;
}

/** Discriminator for {@link SymbolEntry}. */
export type SymbolKind =
  | "typeAlias"
  | "interface"
  | "class"
  | "const"
  | "let"
  | "var"
  | "function"
  | "asyncFunction"
  | "classValue"
  | "enum";

/** Projection-mode tag — mirrors `verter_type_engine::semantic_query::ProjectionMode`. */
export type ProjectionMode = "identity" | "navigate" | "shallow" | "expanded" | "skeleton";

/**
 * Options for {@link TypeInfoSession.resolveSymbol}.
 */
export interface ResolveSymbolOpts {
  /** Projection mode. `undefined` selects the host's default. */
  mode?: ProjectionMode;
  /** Type-arguments slice for generic instantiation. */
  typeArgs?: TypeRef[];
}

/**
 * Type reference accepted as a `typeArg` for
 * {@link TypeInfoSession.resolveSymbol}.
 *
 * Mirrors the public {@link TypeDescriptor} IR — the session lowers
 * descriptors to the wire form and forwards them as a JSON array of
 * native `TypeExpr` values.
 */
export type TypeRef = TypeDescriptor;

/**
 * Audit record. Mirrors the snake_case wire shape produced by
 * `verter_audit::RequestAuditRecord`'s serde serialization
 * (`@verter/types/audit.generated` carries the full ts-rs bindings).
 *
 * Decoded from the raw JSON Buffer the NAPI substrate emits. Fields
 * not declared here are still present on the runtime object — the
 * type is open via index access.
 */
export interface AuditRecord {
  /** Decimal-string request id (u64 wire form). */
  request_id: string;
  /** Canonical id this request ran against. */
  canonical_id: string;
  /** Request kind discriminator (`"TypeResolution"`, etc.). */
  kind: string;
  /** Trace identifier propagated through tracing spans. */
  trace_id?: string;
  /** Whether the audited request was satisfied from a warm cache. */
  from_cache?: boolean;
  /** Wall-clock timing summary. */
  timings?: { total_ms?: number };
  // Open shape — additional fields surface unmodified.
  [key: string]: unknown;
}

/** Result of {@link TypeInfoSession.resolveSymbol}. */
export interface ResolveSymbolResult {
  /** Resolved type descriptor; `undefined` when resolution failed. */
  type?: TypeDescriptor;
  /**
   * Per-request audit record; `undefined` when audit is disabled or the
   * native binding was built without audit capture.
   */
  auditRecord?: AuditRecord;
}
