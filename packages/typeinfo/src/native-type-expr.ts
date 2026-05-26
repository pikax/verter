/**
 * Native `TypeExpr` mirror.
 *
 * Mirrors the Rust enum
 * `verter_semantic::analysis::type_expr::TypeExpr` serialised via
 * its custom `to_json_value` helper. Each variant has a `kind` field
 * (camelCase Rust variant name) plus per-variant payload fields.
 *
 * Re-implementing the type here (rather than depending on
 * `@verter/component-meta`) keeps `@verter/typeinfo` independent of
 * the component-meta package per the architecture-guard contract.
 */

export type NativeTypeExpr =
  | { kind: "primitive"; name: NativePrimitiveName }
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
      returnType?: NativeTypeExpr | null;
      typeParameters?: NativeTypeParameter[];
    }
  | { kind: "ref"; name: string; typeArguments: NativeTypeExpr[] }
  | {
      kind: "recursiveRef";
      name: string;
      typeArguments: NativeTypeExpr[];
      conditionalContext: Array<{
        branch: "true" | "false";
        decided: boolean;
        check: NativeTypeExpr;
        extends: NativeTypeExpr;
      }>;
    }
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
  | {
      kind: "mapped";
      parameter: string;
      source: NativeTypeExpr;
      value: NativeTypeExpr;
    }
  | { kind: "templateLiteral"; quasis: string[]; expressions: NativeTypeExpr[] }
  | { kind: "parenthesized"; inner: NativeTypeExpr }
  | { kind: "unknown"; raw: string }
  | { kind: "infer"; name: string }
  | { kind: "rest"; inner: NativeTypeExpr }
  | {
      kind: "syntheticSlotBinding";
      scopeCanonicalId: string;
      surfaceKind: "slotBinding" | "binding";
      slotName?: string;
      bindingName: string;
      valueNode: string;
    };

export type NativePrimitiveName =
  | "string"
  | "number"
  | "boolean"
  | "symbol"
  | "bigInt"
  | "any"
  | "unknown"
  | "void"
  | "never"
  | "null"
  | "undefined"
  | "object";

export interface NativeTupleElement {
  label?: string | null;
  ty: NativeTypeExpr;
  optional: boolean;
  rest: boolean;
}

export interface NativeObjectMember {
  memberKind: "property" | "indexSignature" | "callSignature" | "constructSignature" | "method";
  name?: string;
  ty?: NativeTypeExpr;
  optional?: boolean;
  readonly?: boolean;
  keyName?: string;
  keyType?: NativeTypeExpr;
  valueType?: NativeTypeExpr;
  function?: NativeFunctionExpr;
}

export interface NativeFunctionParam {
  name?: string | null;
  ty: NativeTypeExpr;
  optional: boolean;
  rest: boolean;
}

export interface NativeFunctionExpr {
  parameters: NativeFunctionParam[];
  returnType?: NativeTypeExpr | null;
  typeParameters?: NativeTypeParameter[];
}

export interface NativeTypeParameter {
  name: string;
  constraint?: NativeTypeExpr;
  default?: NativeTypeExpr;
}
