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

export interface FunctionType {
  kind: "function";
  parameters: FunctionParameter[];
  returnType: TypeDescriptor;
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
  | EnumType
  | RefType
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

export function func(parameters: FunctionParameter[], returnType: TypeDescriptor): FunctionType {
  return { kind: "function", parameters, returnType };
}

export function ref(name: string, typeArguments?: TypeDescriptor[]): RefType {
  return typeArguments ? { kind: "ref", name, typeArguments } : { kind: "ref", name };
}

export function unknown(rawType: string): UnknownType {
  return { kind: "unknown", rawType };
}
