/**
 * @verter/component-meta — session-backed Vue component metadata powered by native Verter.
 *
 * @example
 * ```ts
 * import { openMetaProject } from '@verter/component-meta'
 *
 * const project = await openMetaProject({ root: '.', tsconfig: './tsconfig.json' })
 * const meta = await project.getComponentMeta('./src/MyComponent.vue')
 * project.close()
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
  ObjectIndexSignature,
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

// Native type evaluation bridge
export { typeExprToDescriptor, buildEvaluatedTypeMap } from "./type-expr-bridge.js";
export type {
  NativeEvaluatedTypes,
  NativeEvaluatedField,
  NativeTypeExpr,
} from "./type-expr-bridge.js";

// Pooled project API
export { MetaProject, openMetaProject, evictMetaProject, shutdownMetaRuntime } from "./project.js";
export type { MetaProjectConfig } from "./project.js";
