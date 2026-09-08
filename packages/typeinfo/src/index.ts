/**
 * `@verter/typeinfo` — public type-resolution session API.
 *
 * Wraps the Rust host typeinfo substrate
 * over the `@verter/native` NAPI bindings. Returns
 * `TypeDescriptor`s from `@verter/type-ir` so downstream consumers
 * (Storybook adapters, JSON Schema generators, Zod codegen) can stay
 * framework-agnostic.
 *
 * **Architecture invariant:** this package
 * intentionally does NOT depend on `@verter/component-meta`. The
 * typeinfo substrate is the foundation; component-meta specialises
 * on top of it. Importing component-meta from here would invert the
 * dependency.
 */

export { TypeInfoSession } from "./session.js";
export { decodeResolveResult, TypeResolutionFaultError } from "./decode.js";
export { decodeFrameworkSurfaceResponse } from "./framework-surface.js";

// Typeinfo graph protocol operation DTOs — the consumer closure over
// the graph wire (`TypeInfoGraphRequest` / `TypeInfoGraphResponse` /
// the bounded `SemanticTypeGraph` export).
export {
  buildTypeInfoRequest,
  decodeTypeInfoResult,
  graphNodeToDescriptor,
  MAX_EXPANSION_DEPTH_BUDGET,
  MAX_EXPANSION_NODE_BUDGET,
} from "./graph.js";

export type {
  GraphClosureSpec,
  GraphProjectionModeTag,
  ResolveSymbolGraphQuery,
  SemanticTypeGraphView,
  TypeInfoResult,
} from "./graph.js";

export type {
  FrameworkSurface,
  FrameworkSurfaceError,
  FrameworkSurfaceKindResult,
  FrameworkSurfaceMember,
  FrameworkSurfaceMemberDeclaration,
  FrameworkSurfaceMemberOrigin,
  FrameworkSurfaceOriginHop,
} from "./framework-surface.js";

export type {
  AuditRecord,
  EvaluateTypeExpressionRequest,
  EvaluateTypeExpressionResult,
  ImportSpec,
  NamedImport,
  ProjectionMode,
  ResolveSymbolOpts,
  ResolveSymbolResult,
  SymbolEntry,
  SymbolKind,
  TypeRef,
  TypeInfoSessionConfig,
} from "./types.js";

export type { NativeTypeExpr } from "./native-type-expr.js";

export { nativeToDescriptor } from "./native-to-descriptor.js";
export { descriptorToNative } from "./descriptor-to-native.js";

// Re-export TypeDescriptor for callers that want a single import.
export type { TypeDescriptor } from "@verter/type-ir";
