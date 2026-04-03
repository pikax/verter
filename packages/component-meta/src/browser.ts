/**
 * Browser-safe entry point for @verter/component-meta.
 *
 * Excludes Node.js runtime/session APIs so this can be used in browser
 * contexts like the playground without pulling in native bindings.
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
  TypeParameterType,
  EnumType,
  EnumMember,
  RefType,
  RecursiveRefType,
  RecursiveRefConditionalFrame,
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
  typeParameter,
  ref,
  recursiveRef,
  unknown,
} from "./type-ir.js";

// Native type evaluation bridge
export { typeExprToDescriptor, buildEvaluatedTypeMap } from "./type-expr-bridge.js";
export type {
  NativeEvaluatedTypes,
  NativeEvaluatedField,
  NativeTypeExpr,
} from "./type-expr-bridge.js";
