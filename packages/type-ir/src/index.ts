/**
 * @verter/type-ir — Generic Type IR shared across Verter consumers.
 *
 * The Type IR is a JSON-serializable, framework-agnostic type descriptor tree
 * used to represent Vue component prop/event/slot types in a structured format
 * that can be converted to Storybook argTypes, Histoire stories, Zod schemas,
 * or JSON Schema at runtime.
 *
 * @example
 * ```ts
 * import { primitive, object, union, type TypeDescriptor } from "@verter/type-ir";
 *
 * const numberOrString: TypeDescriptor = union([primitive("number"), primitive("string")]);
 * ```
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
  IndexedAccessType,
  UnknownType,
  TypeDescriptor,
} from "./type-ir.js";

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
  indexedAccess,
  unknown,
} from "./type-ir.js";
