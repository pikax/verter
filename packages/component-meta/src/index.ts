/**
 * @verter/component-meta — Extract Vue component metadata with a generic Type IR.
 *
 * @example
 * ```ts
 * import { extractComponentMeta, createAdapter } from '@verter/component-meta'
 *
 * const adapter = createAdapter()
 * adapter.upsert({ inputId: 'MyComponent.vue', source: sfcSource })
 * const meta = extractComponentMeta(adapter, 'MyComponent.vue')
 * ```
 */

// Core types
export type {
  ComponentMeta,
  PropMeta,
  EventMeta,
  SlotMeta,
  SlotBinding,
  ModelMeta,
  ExposedMeta,
  JsdocTag,
  ComponentPropUsage,
  ComponentUsage,
  TemplateRefMeta,
  ImportMeta,
  BindingMeta,
  VueApiCallMeta,
  StyleMeta,
  SelectorMeta,
  ComponentFlags,
} from "./types.js";

// Type IR
export type {
  TypeDescriptor,
  PrimitiveName,
  PrimitiveType,
  LiteralType,
  UnionType,
  IntersectionType,
  ArrayType,
  TupleType,
  ObjectType,
  ObjectProperty,
  FunctionType,
  FunctionParameter,
  EnumType,
  EnumMember,
  RefType,
  UnknownType,
} from "./type-ir.js";

export {
  primitive,
  literal,
  union,
  intersection,
  array,
  tuple,
  object,
  func,
  ref,
  unknown,
} from "./type-ir.js";

// Resolver
export { parseType, runtimeTypeToDescriptor } from "./resolver.js";

// Host adapter
export type { VerterHostAdapter, HostUpsertRequest } from "./host-adapter.js";
export {
  wrapNapiHost,
  wrapWasmHost,
  createNapiAdapter,
  createWasmAdapter,
  createAdapter,
} from "./host-adapter.js";

// Extractor
export { extractComponentMeta, snapshotToMeta } from "./extractor.js";

// Pooled project API
export { MetaProject, openMetaProject, evictMetaProject, shutdownMetaRuntime } from "./project.js";
export type { MetaProjectConfig } from "./project.js";
