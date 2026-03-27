/**
 * Bridge between native TypeExpr (from Rust evaluator) and TypeDescriptor (JS type IR).
 *
 * Converts the evaluated type expressions returned by the native lightweight
 * evaluator into the public TypeDescriptor format used by adapters and consumers.
 */

import type { TypeDescriptor } from "./type-ir.js";
import {
  primitive,
  literal,
  union,
  intersection,
  array,
  tuple,
  object,
  func,
  typeParameter,
  ref as typeRef,
  unknown,
} from "./type-ir.js";

// ── Native TypeExpr shape (mirrors Rust serde output) ─────────

/** Mirrors `ExpandedComponentTypes` from Rust. */
export interface NativeEvaluatedTypes {
  props?: NativeExpandedField[];
  defineProps?: NativeExpandedMacroProps[];
  emits?: NativeExpandedField[];
  slotBindings?: NativeExpandedField[];
  bindings?: NativeExpandedField[];
}

export interface NativeExpandedField {
  name: string;
  type: NativeTypeExpr;
  optional?: boolean;
  completeness: "exact" | "partial";
  diagnostics: NativeExpansionDiagnostic[];
}

/** Mirrors `ExpandedMacroProps` from Rust. */
export interface NativeExpandedMacroProps {
  macroIndex: number;
  result: NativeExpansionResult<NativeExpandedObjectShape>;
}

/** Mirrors `ExpansionResult<T>` from Rust. */
export interface NativeExpansionResult<T> {
  value: T;
  completeness: "exact" | "partial";
  diagnostics: NativeExpansionDiagnostic[];
}

/** Mirrors `ExpandedObjectShape` from Rust. */
export interface NativeExpandedObjectShape {
  properties: NativeExpandedProperty[];
  indexSignatures: NativeExpandedIndexSignature[];
  callSignatures: unknown[];
}

export interface NativeExpandedProperty {
  name: string;
  ty: NativeTypeExpr;
  optional: boolean;
  readonly: boolean;
}

export interface NativeExpandedIndexSignature {
  keyType: NativeTypeExpr;
  valueType: NativeTypeExpr;
  readonly: boolean;
}

export interface NativeExpansionDiagnostic {
  reason: string;
  context: string;
  propertyName?: string;
}

/** @deprecated Use NativeExpandedField instead */
export type NativeEvaluatedField = NativeExpandedField;
/** @deprecated Use NativeExpandedMacroProps instead */
export type NativeEvaluatedMacroProps = NativeExpandedMacroProps;

/**
 * Mirrors the Rust `TypeExpr` enum serialized with `#[serde(tag = "kind")]`.
 *
 * Each variant has a `kind` field matching the Rust enum variant name (camelCase).
 */
export type NativeTypeExpr =
  | { kind: "primitive"; name: string }
  | { kind: "literal"; literalKind: "string"; value: string }
  | { kind: "literal"; literalKind: "number"; value: number }
  | { kind: "literal"; literalKind: "boolean"; value: boolean }
  | { kind: "literal"; literalKind: "bigInt"; value: string }
  | { kind: "union"; types: NativeTypeExpr[] }
  | { kind: "intersection"; types: NativeTypeExpr[] }
  | { kind: "array"; element: NativeTypeExpr; readonly: boolean }
  | { kind: "tuple"; elements: NativeTupleElement[]; readonly: boolean }
  | { kind: "object"; properties: NativeObjectMember[] }
  | {
      kind: "function";
      parameters: NativeFunctionParam[];
      returnType?: NativeTypeExpr;
      typeParameters?: NativeTypeParameter[];
    }
  | { kind: "ref"; name: string; typeArguments: NativeTypeExpr[] }
  | {
      kind: "typeParameter";
      name: string;
      constraint?: NativeTypeExpr;
      default?: NativeTypeExpr;
    }
  | { kind: "keyOf"; operand: NativeTypeExpr }
  | { kind: "typeOf"; path: string[] }
  | { kind: "indexedAccess"; object: NativeTypeExpr; index: NativeTypeExpr }
  | {
      kind: "conditional";
      check: NativeTypeExpr;
      extends: NativeTypeExpr;
      trueType: NativeTypeExpr;
      falseType: NativeTypeExpr;
    }
  | { kind: "mapped"; parameter: string; source: NativeTypeExpr; value: NativeTypeExpr }
  | { kind: "templateLiteral"; quasis: string[]; expressions: NativeTypeExpr[] }
  | { kind: "parenthesized"; inner: NativeTypeExpr }
  | { kind: "unknown"; raw: string }
  | { kind: "infer"; name: string }
  | { kind: "rest"; inner: NativeTypeExpr };

type NativeTypeRegistry = Map<string, NativeTypeExpr>;

interface NativeTupleElement {
  label?: string | null;
  ty: NativeTypeExpr;
  optional: boolean;
  rest: boolean;
}

interface NativeObjectMember {
  memberKind: "property" | "indexSignature" | "callSignature" | "constructSignature" | "method";
  // Property fields
  name?: string;
  ty?: NativeTypeExpr;
  optional?: boolean;
  readonly?: boolean;
  // Index signature fields
  keyName?: string;
  keyType?: NativeTypeExpr;
  valueType?: NativeTypeExpr;
  // Method/call/construct fields
  function?: NativeFunctionExpr;
}

interface NativeFunctionParam {
  name?: string | null;
  ty: NativeTypeExpr;
  optional: boolean;
  rest: boolean;
}

interface NativeFunctionExpr {
  parameters: NativeFunctionParam[];
  returnType?: NativeTypeExpr | null;
  typeParameters?: NativeTypeParameter[];
}

interface NativeTypeParameter {
  name: string;
  constraint?: NativeTypeExpr | null;
  default?: NativeTypeExpr | null;
}

// ── Converter ────────────────────────────────────────────────────

/**
 * Convert a native TypeExpr to a TypeDescriptor.
 *
 * This bridges the gap between the Rust evaluator output and the
 * JS type IR consumed by adapters (Storybook, Zod, JSON Schema, etc.).
 */
export function typeExprToDescriptor(
  expr: NativeTypeExpr,
  nativeRegistry?: NativeTypeRegistry,
  visiting: Set<string> = new Set(),
): TypeDescriptor {
  switch (expr.kind) {
    case "primitive": {
      const validPrimitives = new Set([
        "string",
        "number",
        "boolean",
        "symbol",
        "bigint",
        "any",
        "unknown",
        "void",
        "never",
        "null",
        "undefined",
        "object",
      ]);
      if (!validPrimitives.has(expr.name)) {
        return unknown(expr.name);
      }
      return primitive(expr.name as Parameters<typeof primitive>[0]);
    }

    case "literal":
      // BigInt literals: store the raw string value as a string literal
      // since TypeDescriptor.LiteralType doesn't have a bigint variant
      if (expr.literalKind === "bigInt") {
        return literal(String(expr.value));
      }
      return literal(expr.value);

    case "union":
      return union(expr.types.map((type) => typeExprToDescriptor(type, nativeRegistry, visiting)));

    case "intersection":
      return intersection(
        expr.types.map((type) => typeExprToDescriptor(type, nativeRegistry, visiting)),
      );

    case "array":
      return array(typeExprToDescriptor(expr.element, nativeRegistry, visiting));

    case "tuple":
      return tuple(expr.elements.map((e) => typeExprToDescriptor(e.ty, nativeRegistry, visiting)));

    case "object": {
      const props = expr.properties
        .filter((m) => m.memberKind === "property" || m.memberKind === "method")
        .map((m) => ({
          name: m.name ?? "",
          type:
            m.memberKind === "method" && m.function
              ? nativeFunctionToDescriptor(m.function, nativeRegistry, visiting)
              : m.ty
                ? typeExprToDescriptor(m.ty, nativeRegistry, visiting)
                : primitive("any"),
          optional: m.optional ?? false,
        }));
      const indexSignatures = expr.properties
        .filter((m) => m.memberKind === "indexSignature")
        .map((m) => ({
          keyName: m.keyName ?? "key",
          keyType: m.keyType
            ? typeExprToDescriptor(m.keyType, nativeRegistry, visiting)
            : primitive("string"),
          valueType: m.valueType
            ? typeExprToDescriptor(m.valueType, nativeRegistry, visiting)
            : primitive("any"),
          ...(m.readonly ? { readonly: true } : {}),
        }));
      const callSignatures = expr.properties
        .filter((m) => m.memberKind === "callSignature" && m.function)
        .map((m) => nativeFunctionToDescriptor(m.function!, nativeRegistry, visiting));
      const constructSignatures = expr.properties
        .filter((m) => m.memberKind === "constructSignature" && m.function)
        .map((m) => nativeFunctionToDescriptor(m.function!, nativeRegistry, visiting));
      return object(props, {
        ...(indexSignatures.length > 0 ? { indexSignatures } : {}),
        ...(callSignatures.length > 0 ? { callSignatures } : {}),
        ...(constructSignatures.length > 0 ? { constructSignatures } : {}),
      });
    }

    case "function": {
      const params = expr.parameters.map((p) => ({
        name: p.name ?? "",
        type: typeExprToDescriptor(p.ty, nativeRegistry, visiting),
        optional: p.optional,
      }));
      const returnType = expr.returnType
        ? typeExprToDescriptor(expr.returnType, nativeRegistry, visiting)
        : primitive("void");
      return func(params, returnType, {
        typeParameters: expr.typeParameters?.map((typeParam) =>
          nativeTypeParameterToDescriptor(typeParam, nativeRegistry, visiting),
        ),
      });
    }

    case "ref":
      if (expr.typeArguments.length > 0) {
        return typeRef(
          expr.name,
          expr.typeArguments.map((typeArgument) =>
            typeExprToDescriptor(typeArgument, nativeRegistry, visiting),
          ),
        );
      }
      return typeRef(expr.name);

    case "typeParameter":
      return nativeTypeParameterToDescriptor(expr, nativeRegistry, visiting);

    case "keyOf":
    case "typeOf":
    case "conditional":
    case "mapped":
    case "templateLiteral":
    case "infer":
    case "rest":
      // These operator forms should be evaluated by the native evaluator.
      // If they reach here, they couldn't be reduced — fall back to unknown.
      return unknown(nativeTypeExprToString(expr));

    case "indexedAccess": {
      const resolved = nativeRegistry
        ? resolveNativeIndexedAccess(expr, nativeRegistry, visiting)
        : undefined;
      if (resolved) {
        return typeExprToDescriptor(resolved, nativeRegistry, visiting);
      }
      return unknown(nativeTypeExprToString(expr));
    }

    case "parenthesized":
      return typeExprToDescriptor(expr.inner, nativeRegistry, visiting);

    case "unknown":
      return unknown(expr.raw);

    default:
      return unknown("unrecognized");
  }
}

function nativeFunctionToDescriptor(
  expr: NativeFunctionExpr,
  nativeRegistry?: NativeTypeRegistry,
  visiting: Set<string> = new Set(),
) {
  return func(
    (expr.parameters ?? []).map((p) => ({
      name: p.name ?? "",
      type: typeExprToDescriptor(p.ty, nativeRegistry, visiting),
      optional: p.optional,
    })),
    expr.returnType
      ? typeExprToDescriptor(expr.returnType, nativeRegistry, visiting)
      : primitive("void"),
    {
      typeParameters: expr.typeParameters?.map((typeParam) =>
        nativeTypeParameterToDescriptor(typeParam, nativeRegistry, visiting),
      ),
    },
  );
}

function nativeTypeParameterToDescriptor(
  expr: NativeTypeParameter,
  nativeRegistry?: NativeTypeRegistry,
  visiting: Set<string> = new Set(),
) {
  return typeParameter(expr.name, {
    ...(expr.constraint
      ? { constraint: typeExprToDescriptor(expr.constraint, nativeRegistry, visiting) }
      : {}),
    ...(expr.default
      ? { default: typeExprToDescriptor(expr.default, nativeRegistry, visiting) }
      : {}),
  });
}

function resolveNativeIndexedAccess(
  expr: Extract<NativeTypeExpr, { kind: "indexedAccess" }>,
  nativeRegistry: NativeTypeRegistry,
  visiting: Set<string>,
): NativeTypeExpr | undefined {
  const objectExpr = resolveNativeRegistryExpr(expr.object, nativeRegistry, visiting);
  const indexExpr = resolveNativeRegistryExpr(expr.index, nativeRegistry, visiting);
  if (indexExpr.kind !== "literal" || indexExpr.literalKind !== "string") {
    return undefined;
  }

  const property = resolveNativeObjectProperty(objectExpr, indexExpr.value);
  if (!property) {
    return undefined;
  }

  if (!property.optional) {
    return property.ty;
  }

  return {
    kind: "union",
    types: [property.ty, { kind: "primitive", name: "undefined" }],
  };
}

function resolveNativeRegistryExpr(
  expr: NativeTypeExpr,
  nativeRegistry: NativeTypeRegistry,
  visiting: Set<string>,
): NativeTypeExpr {
  if (expr.kind === "parenthesized") {
    return resolveNativeRegistryExpr(expr.inner, nativeRegistry, visiting);
  }

  if (expr.kind === "ref" && expr.typeArguments.length === 0) {
    if (visiting.has(expr.name)) {
      return expr;
    }
    const resolved = nativeRegistry.get(expr.name);
    if (!resolved) {
      return expr;
    }
    visiting.add(expr.name);
    const next = resolveNativeRegistryExpr(resolved, nativeRegistry, visiting);
    visiting.delete(expr.name);
    return next;
  }

  if (expr.kind === "indexedAccess") {
    return resolveNativeIndexedAccess(expr, nativeRegistry, visiting) ?? expr;
  }

  return expr;
}

function resolveNativeObjectProperty(
  expr: NativeTypeExpr,
  propertyName: string,
): { ty: NativeTypeExpr; optional: boolean } | undefined {
  if (expr.kind !== "object") {
    return undefined;
  }

  const member = expr.properties.find(
    (candidate) => candidate.memberKind === "property" && candidate.name === propertyName,
  );
  if (!member?.ty) {
    return undefined;
  }

  return {
    ty: member.ty,
    optional: member.optional ?? false,
  };
}

/**
 * Build a lookup map from native evaluated types.
 * Returns a Map<fieldName, TypeDescriptor> for quick lookup.
 */
export function buildEvaluatedTypeMap(
  fields: NativeEvaluatedField[] | undefined,
): Map<string, TypeDescriptor> {
  const map = new Map<string, TypeDescriptor>();
  if (!fields) return map;
  for (const field of fields) {
    map.set(field.name, typeExprToDescriptor(field.type));
  }
  return map;
}

/** Rough string representation for unknown/unreduced operator forms. */
function nativeTypeExprToString(expr: NativeTypeExpr): string {
  switch (expr.kind) {
    case "primitive":
      return expr.name;
    case "literal":
      return String(expr.value);
    case "ref":
      return expr.name;
    case "keyOf":
      return `keyof ${nativeTypeExprToString(expr.operand)}`;
    case "typeOf":
      return `typeof ${expr.path.join(".")}`;
    case "unknown":
      return expr.raw;
    default:
      return expr.kind;
  }
}
