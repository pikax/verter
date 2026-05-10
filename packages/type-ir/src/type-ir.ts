/**
 * Generic Type IR — a JSON-serializable, framework-agnostic type descriptor tree.
 *
 * Used to represent Vue component prop/event/slot types in a structured format
 * that can be converted to Storybook argTypes, Histoire stories, Zod schemas,
 * or JSON Schema at runtime.
 */

// ── Primitive ────────────────────────────────────────────────────

export type PrimitiveName =
  | "string"
  | "number"
  | "boolean"
  | "symbol"
  | "bigint"
  | "any"
  | "unknown"
  | "void"
  | "never"
  | "null"
  | "undefined"
  | "object";

export interface PrimitiveType {
  kind: "primitive";
  name: PrimitiveName;
}

// ── Literal ──────────────────────────────────────────────────────

export interface LiteralType {
  kind: "literal";
  value: string | number | boolean;
}

// ── Union / Intersection ─────────────────────────────────────────

export interface UnionType {
  kind: "union";
  types: TypeDescriptor[];
}

export interface IntersectionType {
  kind: "intersection";
  types: TypeDescriptor[];
}

// ── Array / Tuple ────────────────────────────────────────────────

export interface ArrayType {
  kind: "array";
  element: TypeDescriptor;
}

export interface TupleType {
  kind: "tuple";
  elements: TypeDescriptor[];
}

// ── Object ───────────────────────────────────────────────────────

export interface ObjectProperty {
  name: string;
  type: TypeDescriptor;
  optional: boolean;
}

export interface ObjectIndexSignature {
  keyName: string;
  keyType: TypeDescriptor;
  valueType: TypeDescriptor;
  readonly?: boolean;
}

export interface ObjectType {
  kind: "object";
  properties: ObjectProperty[];
  indexSignatures?: ObjectIndexSignature[];
  callSignatures?: FunctionType[];
  constructSignatures?: FunctionType[];
}

// ── Function ─────────────────────────────────────────────────────

export interface FunctionParameter {
  name: string;
  type: TypeDescriptor;
  optional: boolean;
}

export interface TypeParameterType {
  kind: "typeParameter";
  name: string;
  constraint?: TypeDescriptor;
  default?: TypeDescriptor;
}

export interface FunctionType {
  kind: "function";
  parameters: FunctionParameter[];
  returnType: TypeDescriptor;
  typeParameters?: TypeParameterType[];
}

// ── Enum ─────────────────────────────────────────────────────────

export interface EnumMember {
  name: string;
  value?: string | number;
}

export interface EnumType {
  kind: "enum";
  name: string;
  members: EnumMember[];
}

// ── Ref (named type reference) ───────────────────────────────────

export interface RefType {
  kind: "ref";
  name: string;
  typeArguments?: TypeDescriptor[];
}

// ── RecursiveRef (recursive type back-reference) ────────────────

export interface RecursiveRefConditionalFrame {
  branch: "true" | "false";
  decided: boolean;
  check: TypeDescriptor;
  extends: TypeDescriptor;
}

export interface RecursiveRefType {
  kind: "recursiveRef";
  name: string;
  typeArguments: TypeDescriptor[];
  conditionalContext: RecursiveRefConditionalFrame[];
}

// ── IndexedAccess ────────────────────────────────────────────────

/**
 * Represents an indexed-access TypeScript type (`T['K']` / `T[K]`).
 *
 * Surfaces unresolvable indexed-access shapes (e.g. when the object type
 * is a generic parameter or an unresolved external ref) so consumers can
 * structurally inspect the form rather than recovering it from raw text.
 *
 * When resolution succeeds (the object type is a concrete object whose
 * `K` member is known), the bridge collapses the indexed access to the
 * member type directly — `IndexedAccessType` only appears when the
 * structural shape has to survive opaquely.
 */
export interface IndexedAccessType {
  kind: "indexedAccess";
  objectType: TypeDescriptor;
  indexType: TypeDescriptor;
}

// ── Unknown (fallback) ──────────────────────────────────────────

export interface UnknownType {
  kind: "unknown";
  rawType: string;
}

// ── Discriminated Union ──────────────────────────────────────────

export type TypeDescriptor =
  | PrimitiveType
  | LiteralType
  | UnionType
  | IntersectionType
  | ArrayType
  | TupleType
  | ObjectType
  | FunctionType
  | TypeParameterType
  | EnumType
  | RefType
  | RecursiveRefType
  | IndexedAccessType
  | UnknownType;

// ── Factory helpers ──────────────────────────────────────────────

export function primitive(name: PrimitiveName): PrimitiveType {
  return { kind: "primitive", name };
}

export function literal(value: string | number | boolean): LiteralType {
  return { kind: "literal", value };
}

export function union(types: TypeDescriptor[]): TypeDescriptor {
  if (types.length === 1) return types[0];
  return { kind: "union", types };
}

export function intersection(types: TypeDescriptor[]): TypeDescriptor {
  if (types.length === 1) return types[0];
  return { kind: "intersection", types };
}

export function array(element: TypeDescriptor): ArrayType {
  return { kind: "array", element };
}

export function tuple(elements: TypeDescriptor[]): TupleType {
  return { kind: "tuple", elements };
}

export function object(
  properties: ObjectProperty[],
  options?: {
    indexSignatures?: ObjectIndexSignature[];
    callSignatures?: FunctionType[];
    constructSignatures?: FunctionType[];
  },
): ObjectType {
  return {
    kind: "object",
    properties,
    ...(options?.indexSignatures?.length ? { indexSignatures: options.indexSignatures } : {}),
    ...(options?.callSignatures?.length ? { callSignatures: options.callSignatures } : {}),
    ...(options?.constructSignatures?.length
      ? { constructSignatures: options.constructSignatures }
      : {}),
  };
}

export function typeParameter(
  name: string,
  options?: {
    constraint?: TypeDescriptor;
    default?: TypeDescriptor;
  },
): TypeParameterType {
  return {
    kind: "typeParameter",
    name,
    ...(options?.constraint ? { constraint: options.constraint } : {}),
    ...(options?.default ? { default: options.default } : {}),
  };
}

export function func(
  parameters: FunctionParameter[],
  returnType: TypeDescriptor,
  options?: {
    typeParameters?: TypeParameterType[];
  },
): FunctionType {
  return {
    kind: "function",
    parameters,
    returnType,
    ...(options?.typeParameters?.length ? { typeParameters: options.typeParameters } : {}),
  };
}

export function ref(name: string, typeArguments?: TypeDescriptor[]): RefType {
  return typeArguments ? { kind: "ref", name, typeArguments } : { kind: "ref", name };
}

export function recursiveRef(
  name: string,
  typeArguments: TypeDescriptor[],
  conditionalContext: RecursiveRefConditionalFrame[],
): RecursiveRefType {
  return { kind: "recursiveRef", name, typeArguments, conditionalContext };
}

export function indexedAccess(
  objectType: TypeDescriptor,
  indexType: TypeDescriptor,
): IndexedAccessType {
  return { kind: "indexedAccess", objectType, indexType };
}

export function unknown(rawType: string): UnknownType {
  return { kind: "unknown", rawType };
}
