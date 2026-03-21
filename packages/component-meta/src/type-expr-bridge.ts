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
  ref as typeRef,
  unknown,
} from "./type-ir.js";

// ── Native TypeExpr shape (mirrors Rust serde output) ─────────

/** Mirrors `EvaluatedComponentTypes` from Rust. */
export interface NativeEvaluatedTypes {
  props?: NativeEvaluatedField[];
  defineProps?: NativeEvaluatedMacroProps[];
  emits?: NativeEvaluatedField[];
  slotBindings?: NativeEvaluatedField[];
  bindings?: NativeEvaluatedField[];
}

export interface NativeEvaluatedField {
  name: string;
  type: NativeTypeExpr;
  optional?: boolean;
}

export interface NativeEvaluatedMacroProps {
  macroIndex: number;
  fields: NativeEvaluatedField[];
}

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
      typeParameters?: unknown[];
    }
  | { kind: "ref"; name: string; typeArguments: NativeTypeExpr[] }
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
  typeParameters?: unknown[];
}

// ── Converter ────────────────────────────────────────────────────

/**
 * Convert a native TypeExpr to a TypeDescriptor.
 *
 * This bridges the gap between the Rust evaluator output and the
 * JS type IR consumed by adapters (Storybook, Zod, JSON Schema, etc.).
 */
export function typeExprToDescriptor(expr: NativeTypeExpr): TypeDescriptor {
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
      return union(expr.types.map(typeExprToDescriptor));

    case "intersection":
      return intersection(expr.types.map(typeExprToDescriptor));

    case "array":
      return array(typeExprToDescriptor(expr.element));

    case "tuple":
      return tuple(expr.elements.map((e) => typeExprToDescriptor(e.ty)));

    case "object": {
      const props = expr.properties
        .filter((m) => m.memberKind === "property" || m.memberKind === "method")
        .map((m) => ({
          name: m.name ?? "",
          type:
            m.memberKind === "method" && m.function
              ? nativeFunctionToDescriptor(m.function)
              : m.ty
                ? typeExprToDescriptor(m.ty)
                : primitive("any"),
          optional: m.optional ?? false,
        }));
      const indexSignatures = expr.properties
        .filter((m) => m.memberKind === "indexSignature")
        .map((m) => ({
          keyName: m.keyName ?? "key",
          keyType: m.keyType ? typeExprToDescriptor(m.keyType) : primitive("string"),
          valueType: m.valueType ? typeExprToDescriptor(m.valueType) : primitive("any"),
          ...(m.readonly ? { readonly: true } : {}),
        }));
      const callSignatures = expr.properties
        .filter((m) => m.memberKind === "callSignature" && m.function)
        .map((m) => nativeFunctionToDescriptor(m.function!));
      const constructSignatures = expr.properties
        .filter((m) => m.memberKind === "constructSignature" && m.function)
        .map((m) => nativeFunctionToDescriptor(m.function!));
      return object(props, {
        ...(indexSignatures.length > 0 ? { indexSignatures } : {}),
        ...(callSignatures.length > 0 ? { callSignatures } : {}),
        ...(constructSignatures.length > 0 ? { constructSignatures } : {}),
      });
    }

    case "function": {
      const params = expr.parameters.map((p) => ({
        name: p.name ?? "",
        type: typeExprToDescriptor(p.ty),
        optional: p.optional,
      }));
      const returnType = expr.returnType
        ? typeExprToDescriptor(expr.returnType)
        : primitive("void");
      return func(params, returnType);
    }

    case "ref":
      if (expr.typeArguments.length > 0) {
        return typeRef(expr.name, expr.typeArguments.map(typeExprToDescriptor));
      }
      return typeRef(expr.name);

    case "keyOf":
    case "typeOf":
    case "indexedAccess":
    case "conditional":
    case "mapped":
    case "templateLiteral":
    case "infer":
    case "rest":
      // These operator forms should be evaluated by the native evaluator.
      // If they reach here, they couldn't be reduced — fall back to unknown.
      return unknown(nativeTypeExprToString(expr));

    case "parenthesized":
      return typeExprToDescriptor(expr.inner);

    case "unknown":
      return unknown(expr.raw);

    default:
      return unknown("unrecognized");
  }
}

function nativeFunctionToDescriptor(expr: NativeFunctionExpr) {
  return func(
    (expr.parameters ?? []).map((p) => ({
      name: p.name ?? "",
      type: typeExprToDescriptor(p.ty),
      optional: p.optional,
    })),
    expr.returnType ? typeExprToDescriptor(expr.returnType) : primitive("void"),
  );
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
