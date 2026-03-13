/**
 * Browser-safe entry point for @verter/component-meta.
 *
 * Excludes host-adapter (which depends on @verter/native) so this can be
 * used in browser contexts like the playground without pulling in Node.js
 * native bindings.
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

// Extractor (snapshotToMeta only — no extractComponentMeta which needs adapter)
export { snapshotToMeta } from "./extractor.js";
