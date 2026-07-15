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
  /** LRU cap for the host's evaluate-type-expression scratch cache. */
  evaluateTypeExpressionCacheSize?: number;
  /**
   * Enable per-request audit. Default `true`. When `false` the
   * `auditRecord` field on `ResolveSymbolResult` /
   * `EvaluateTypeExpressionResult` is `undefined` (the resolution
   * still runs).
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

/** Projection-mode tag — mirrors `verter_session::semantic_query::ProjectionMode`. */
export type ProjectionMode = "identity" | "navigate" | "shallow" | "expanded" | "skeleton";

/**
 * Single import to inject into a synthesised scratch file evaluated
 * by {@link TypeInfoSession.evaluateTypeExpression}.
 */
export interface ImportSpec {
  /** Raw import specifier (e.g. `"./types"`, `"reka-ui"`). */
  specifier: string;
  /** Per-binding shape. */
  bindings: NamedImport[];
}

/** One binding in an {@link ImportSpec}. */
export type NamedImport =
  | { kind: "default"; localName: string }
  | {
      kind: "named";
      exportedName: string;
      /** Optional rename. `undefined` / empty means "no alias". */
      localAlias?: string;
      /** `true` for `import { type X }`. */
      typeOnly?: boolean;
    }
  | { kind: "namespace"; localName: string };

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
 * Request shape for {@link TypeInfoSession.evaluateTypeExpression}.
 *
 * Mirrors `verter_session::typeinfo::types::EvaluateTypeExpressionRequest`.
 */
export interface EvaluateTypeExpressionRequest {
  /** File scope the expression evaluates against. */
  scope: string;
  /** TypeScript type expression body. */
  expression: string;
  /** Imports to inject into the synthesised scratch. */
  extraImports?: ImportSpec[];
  /** Projection mode for the terminal evaluation. Default `"expanded"`. */
  mode?: ProjectionMode;
  /** When `true` (default) the scratch URI publishes to the host's LRU. */
  cacheable?: boolean;
}

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
  /** Per-request audit record; `undefined` when audit is disabled. */
  auditRecord?: AuditRecord;
}

/** Result of {@link TypeInfoSession.evaluateTypeExpression}. */
export interface EvaluateTypeExpressionResult {
  /** Resolved type descriptor; `undefined` when resolution failed. */
  type?: TypeDescriptor;
  /** Per-request audit record; `undefined` when audit is disabled. */
  auditRecord?: AuditRecord;
}
