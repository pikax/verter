/**
 * Backward-compatible re-export of the canonical Type IR.
 *
 * The Type IR moved to `@verter/type-ir`; this module is a permanent
 * re-export shim so existing consumers of `@verter/component-meta`
 * continue to receive the same surface.
 */

export type {
  PrimitiveName,
  PrimitiveType,
  LiteralType,
  UnionType,
  IntersectionType,
  ArrayType,
  TupleType,
  ObjectProperty,
  ObjectIndexSignature,
  ObjectType,
  FunctionParameter,
  TypeParameterType,
  FunctionType,
  EnumMember,
  EnumType,
  RefType,
  RecursiveRefConditionalFrame,
  RecursiveRefType,
  SyntheticSlotBindingType,
  IndexedAccessType,
  UnknownType,
  TypeDescriptor,
} from "@verter/type-ir";

export {
  primitive,
  literal,
  union,
  intersection,
  array,
  tuple,
  object,
  typeParameter,
  func,
  ref,
  recursiveRef,
  syntheticSlotBinding,
  indexedAccess,
  unknown,
} from "@verter/type-ir";
