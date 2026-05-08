/**
 * Lowering from `TypeDescriptor` (`@verter/type-ir`) to the native
 * `TypeExpr` wire shape that the Rust host accepts.
 *
 * Used by {@link TypeInfoSession.resolveSymbol} to forward a
 * `typeArgs?: TypeRef[]` argument as a JSON array of native
 * `TypeExpr` values per §5.2 of the typeinfo plan.
 *
 * The mapping is structural; descriptor variants without a direct
 * native counterpart (`enum`, `recursiveRef` cycles) lower to a
 * `unknown(raw)` shell the resolver treats as opaque.
 */

import type { TypeDescriptor } from "@verter/type-ir";

import type {
  NativeFunctionExpr,
  NativeFunctionParam,
  NativeObjectMember,
  NativeTupleElement,
  NativeTypeExpr,
} from "./native-type-expr.js";

/** Lower a descriptor to the native wire shape. */
export function descriptorToNative(d: TypeDescriptor): NativeTypeExpr {
  switch (d.kind) {
    case "primitive":
      return {
        kind: "primitive",
        name: d.name === "bigint" ? "bigInt" : d.name,
      };
    case "literal":
      switch (typeof d.value) {
        case "string":
          return { kind: "literal", literalKind: "string", value: d.value };
        case "number":
          return { kind: "literal", literalKind: "number", value: d.value };
        case "boolean":
          return { kind: "literal", literalKind: "boolean", value: d.value };
      }
      // Fallthrough — descriptor literal type narrowed above.
      return { kind: "unknown", raw: "literal" };
    case "union":
      return { kind: "union", types: d.types.map(descriptorToNative) };
    case "intersection":
      return { kind: "intersection", types: d.types.map(descriptorToNative) };
    case "array":
      return {
        kind: "array",
        element: descriptorToNative(d.element),
        readonly: false,
      };
    case "tuple":
      return {
        kind: "tuple",
        elements: d.elements.map<NativeTupleElement>((el: TypeDescriptor) => ({
          ty: descriptorToNative(el),
          optional: false,
          rest: false,
        })),
        readonly: false,
      };
    case "object":
      return { kind: "object", properties: lowerObjectMembers(d) };
    case "function":
      return lowerFunction(d);
    case "ref":
      return {
        kind: "ref",
        name: d.name,
        typeArguments: (d.typeArguments ?? []).map(descriptorToNative),
      };
    case "recursiveRef":
      return {
        kind: "recursiveRef",
        name: d.name,
        typeArguments: d.typeArguments.map(descriptorToNative),
        conditionalContext: d.conditionalContext.map(
          (frame: {
            branch: "true" | "false";
            decided: boolean;
            check: TypeDescriptor;
            extends: TypeDescriptor;
          }) => ({
            branch: frame.branch,
            decided: frame.decided,
            check: descriptorToNative(frame.check),
            extends: descriptorToNative(frame.extends),
          }),
        ),
      };
    case "typeParameter":
      return {
        kind: "typeParameter",
        name: d.name,
        constraint: d.constraint ? descriptorToNative(d.constraint) : undefined,
        default: d.default ? descriptorToNative(d.default) : undefined,
      };
    case "unknown":
      return { kind: "unknown", raw: d.rawType };
    default:
      // `enum` and any future descriptor variant collapse to a raw
      // unknown shell. The Rust resolver will treat this as a hard
      // miss rather than a silent type-narrowing.
      return { kind: "unknown", raw: descriptorBriefName(d) };
  }
}

function lowerObjectMembers(d: Extract<TypeDescriptor, { kind: "object" }>): NativeObjectMember[] {
  const members: NativeObjectMember[] = [];
  for (const prop of d.properties) {
    members.push({
      memberKind: "property",
      name: prop.name,
      ty: descriptorToNative(prop.type),
      optional: prop.optional,
      readonly: false,
    });
  }
  if (d.indexSignatures && d.indexSignatures.length > 0) {
    for (const sig of d.indexSignatures) {
      members.push({
        memberKind: "indexSignature",
        keyName: sig.keyName,
        keyType: descriptorToNative(sig.keyType),
        valueType: descriptorToNative(sig.valueType),
        readonly: sig.readonly ?? false,
      });
    }
  }
  return members;
}

function lowerFunction(d: Extract<TypeDescriptor, { kind: "function" }>): NativeTypeExpr {
  const fn: NativeFunctionExpr = {
    parameters: d.parameters.map<NativeFunctionParam>((param) => ({
      name: param.name,
      ty: descriptorToNative(param.type),
      optional: param.optional,
      rest: false,
    })),
    returnType: descriptorToNative(d.returnType),
    typeParameters: undefined,
  };
  return {
    kind: "function",
    parameters: fn.parameters,
    returnType: fn.returnType,
    typeParameters: fn.typeParameters,
  };
}

function descriptorBriefName(d: TypeDescriptor): string {
  return (d as { kind: string }).kind ?? "unknown";
}
