/**
 * Lowering from the native `TypeExpr` wire shape (Rust serialisation
 * of `verter_semantic::analysis::type_expr::TypeExpr`) to the public
 * `TypeDescriptor` IR exported by `@verter/type-ir`.
 *
 * The mapping is structural and lossless for the descriptors that
 * `@verter/type-ir` represents. `indexedAccess` lowers to the dedicated
 * `IndexedAccessType` variant so consumers can structurally inspect
 * `T['K']` shapes. The remaining operator-shaped variants (`keyOf`,
 * `typeOf`, `templateLiteral`, `infer`, `rest`, `mapped`, `conditional`)
 * lower to `unknown(raw)` because the caller is expected to issue an
 * `Expanded`-mode resolution if it wants the body of the operator
 * instead of a syntax-preserving shell.
 */

import {
  primitive,
  literal,
  union,
  intersection,
  array,
  tuple,
  object,
  func,
  ref,
  recursiveRef,
  typeParameter,
  indexedAccess,
  unknown,
  syntheticSlotBinding,
  type ObjectIndexSignature,
  type ObjectProperty,
  type FunctionParameter,
  type RecursiveRefConditionalFrame,
  type TypeDescriptor,
} from "@verter/type-ir";

import type {
  NativeFunctionExpr,
  NativeFunctionParam,
  NativeObjectMember,
  NativePrimitiveName,
  NativeTypeExpr,
} from "./native-type-expr.js";

/** Map a native primitive to the public `PrimitiveType` name. */
function lowerPrimitiveName(
  name: NativePrimitiveName,
):
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
  | "object" {
  switch (name) {
    case "bigInt":
      return "bigint";
    default:
      return name;
  }
}

/**
 * Lower a native `TypeExpr` to a `TypeDescriptor`.
 *
 * The mapping is total — every native variant produces a descriptor.
 * Variants that don't have a direct IR counterpart map to
 * `unknown(raw)` so consumers can fall back to a string display
 * without crashing.
 */
export function nativeToDescriptor(expr: NativeTypeExpr): TypeDescriptor {
  switch (expr.kind) {
    case "primitive":
      return primitive(lowerPrimitiveName(expr.name));
    case "literal": {
      switch (expr.literalKind) {
        case "string":
          return literal(expr.value);
        case "number":
          return literal(expr.value);
        case "boolean":
          return literal(expr.value);
        case "bigInt":
          // @verter/type-ir's `literal()` only accepts string | number |
          // boolean; surface bigint as a refless `unknown(raw)` until a
          // dedicated bigint literal slot lands.
          return unknown(`${expr.value}n`);
      }
      // Unreachable — exhaustive switch above.
      return unknown("literal");
    }
    case "union":
      return union(expr.types.map(nativeToDescriptor));
    case "intersection":
      return intersection(expr.types.map(nativeToDescriptor));
    case "array":
      return array(nativeToDescriptor(expr.element));
    case "tuple":
      return tuple(expr.elements.map((e) => nativeToDescriptor(e.ty)));
    case "object":
      return lowerObject(expr.properties);
    case "function":
      return lowerFunction({
        parameters: expr.parameters,
        returnType: expr.returnType ?? undefined,
        typeParameters: expr.typeParameters,
      });
    case "ref":
      return ref(expr.name, expr.typeArguments.map(nativeToDescriptor));
    case "recursiveRef":
      return recursiveRef(
        expr.name,
        expr.typeArguments.map(nativeToDescriptor),
        expr.conditionalContext.map<RecursiveRefConditionalFrame>((frame) => ({
          branch: frame.branch,
          decided: frame.decided,
          check: nativeToDescriptor(frame.check),
          extends: nativeToDescriptor(frame.extends),
        })),
      );
    case "typeParameter":
      return typeParameter(expr.name, {
        constraint: expr.constraint ? nativeToDescriptor(expr.constraint) : undefined,
        default: expr.default ? nativeToDescriptor(expr.default) : undefined,
      });
    case "parenthesized":
      return nativeToDescriptor(expr.inner);
    case "unknown":
      return unknown(expr.raw);
    case "keyOf":
      return unknown(`keyof ${describeBrief(expr.operand)}`);
    case "typeOf":
      return unknown(`typeof ${expr.path.join(".")}`);
    case "indexedAccess":
      return indexedAccess(nativeToDescriptor(expr.object), nativeToDescriptor(expr.index));
    case "conditional":
      return unknown(
        `${describeBrief(expr.check)} extends ${describeBrief(expr.extends)} ? ` +
          `${describeBrief(expr.trueType)} : ${describeBrief(expr.falseType)}`,
      );
    case "mapped":
      return unknown(
        `{ [${expr.parameter} in ${describeBrief(expr.source)}]: ${describeBrief(expr.value)} }`,
      );
    case "templateLiteral":
      return unknown(`templateLiteral(${expr.quasis.length})`);
    case "infer":
      return unknown(`infer ${expr.name}`);
    case "rest":
      return unknown(`...${describeBrief(expr.inner)}`);
    case "syntheticSlotBinding":
      return syntheticSlotBinding(
        expr.scopeCanonicalId,
        expr.surfaceKind,
        expr.bindingName,
        expr.valueNode,
        expr.slotName,
      );
  }
}

function lowerObject(members: NativeObjectMember[]): TypeDescriptor {
  const properties: ObjectProperty[] = [];
  const indexSignatures: ObjectIndexSignature[] = [];
  for (const member of members) {
    switch (member.memberKind) {
      case "property":
        if (member.name && member.ty) {
          properties.push({
            name: member.name,
            type: nativeToDescriptor(member.ty),
            optional: member.optional ?? false,
          });
        }
        break;
      case "method":
        if (member.name && member.function) {
          properties.push({
            name: member.name,
            type: lowerFunction(member.function),
            optional: member.optional ?? false,
          });
        }
        break;
      case "indexSignature":
        if (member.keyType && member.valueType) {
          indexSignatures.push({
            keyName: member.keyName ?? "key",
            keyType: nativeToDescriptor(member.keyType),
            valueType: nativeToDescriptor(member.valueType),
            readonly: member.readonly ?? false,
          });
        }
        break;
      // callSignature / constructSignature: omitted — TypeDescriptor's
      // ObjectType *does* carry these but the substrate's typeinfo
      // raise pipeline doesn't surface them yet. A consumer that
      // needs call/construct signatures should re-evaluate with the
      // function shape.
      case "callSignature":
      case "constructSignature":
        break;
    }
  }
  return object(properties, indexSignatures.length > 0 ? { indexSignatures } : undefined);
}

function lowerFunction(fn: NativeFunctionExpr): TypeDescriptor {
  const parameters: FunctionParameter[] = fn.parameters.map<FunctionParameter>(
    (param: NativeFunctionParam, idx) => ({
      name: param.name ?? `arg${idx}`,
      type: nativeToDescriptor(param.ty),
      optional: param.optional,
    }),
  );
  const returnType = fn.returnType ? nativeToDescriptor(fn.returnType) : primitive("void");
  return func(parameters, returnType);
}

function describeBrief(expr: NativeTypeExpr): string {
  switch (expr.kind) {
    case "primitive":
      return expr.name;
    case "literal":
      return JSON.stringify(expr.value);
    case "ref":
      return expr.name;
    default:
      return expr.kind;
  }
}
