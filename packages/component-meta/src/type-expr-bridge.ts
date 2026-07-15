/**
 * Bridge between native TypeExpr (from Rust evaluator) and TypeDescriptor (JS type IR).
 *
 * Converts the evaluated type expressions returned by the native lightweight
 * evaluator into the public TypeDescriptor format used by adapters and consumers.
 */

import type { TypeDescriptor } from "@verter/type-ir";
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
  recursiveRef,
  indexedAccess,
  unknown,
  syntheticSlotBinding,
} from "@verter/type-ir";
import {
  createGraphTypeExprRef,
  DecodedTypeGraph,
  isGraphTypeExprRef,
  LITERAL_BIG_INT,
  LITERAL_BOOLEAN,
  LITERAL_NUMBER,
  LITERAL_STRING,
  MEMBER_CALL_SIGNATURE,
  MEMBER_CONSTRUCT_SIGNATURE,
  MEMBER_INDEX_SIGNATURE,
  MEMBER_METHOD,
  MEMBER_PROPERTY,
  NODE_ARRAY,
  NODE_CONDITIONAL,
  NODE_FUNCTION,
  NODE_INDEXED_ACCESS,
  NODE_INFER,
  NODE_INTERSECTION,
  NODE_KEY_OF,
  NODE_LITERAL,
  NODE_MAPPED,
  NODE_OBJECT,
  NODE_PARENTHESIZED,
  NODE_PRIMITIVE,
  NODE_RECURSIVE_REF,
  NODE_REF,
  NODE_REST,
  NODE_SYNTHETIC_SLOT_BINDING,
  NODE_TEMPLATE_LITERAL,
  NODE_TUPLE,
  SYNTHETIC_CARRIER_SURFACE_BINDING,
  SYNTHETIC_CARRIER_SURFACE_SLOT_BINDING,
  NODE_TYPE_OF,
  NODE_TYPE_PARAMETER,
  NODE_UNION,
  NODE_UNKNOWN,
  type GraphNodeRecord,
  type GraphTypeExprRef,
} from "./type-graph-core.js";

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
  exactness: "exactConcrete" | "exactSymbolic" | "incomplete";
  executionStatus: "completed" | "cancelled" | "interrupted" | "hardStop";
  diagnostics: NativeExpansionDiagnostic[];
  /**
   * Shallow lowered typed form carried alongside `type` (post-expansion).
   * Surfaces the bare annotation expression the user wrote
   * (e.g. `Ref { name: "ImportedAlias" }`) so consumers that need the
   * syntactic shape do not have to reparse the display `rawType`. `None`
   * when the analyzer's shallow source was absent.
   */
  shallowTypeExpr?: NativeTypeExpr;
  /**
   * Scope of `shallowTypeExpr`: canonical_id of the file whose OXC parse
   * produced the shallow expression. Pairing invariant:
   * `shallowTypeExpr` is set iff `shallowTypeExprScope` is set.
   */
  shallowTypeExprScope?: string;
}

/** Mirrors `ExpandedMacroProps` from Rust. */
export interface NativeExpandedMacroProps {
  macroIndex: number;
  result: NativeExpansionResult<NativeExpandedObjectShape>;
}

/** Mirrors `ExpansionResult<T>` from Rust. */
export interface NativeExpansionResult<T> {
  value: T;
  exactness: "exactConcrete" | "exactSymbolic" | "incomplete";
  executionStatus: "completed" | "cancelled" | "interrupted" | "hardStop";
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
 * The native (raw-JSON) mapped-type modifier as the Rust producer serializes it
 * (`MappedModifier` -> `modifier_str`): `"none"` (no modifier), `"add"` (`+?` /
 * `+readonly`), or `"remove"` (`-?` / `-readonly`).
 */
export type MappedModifierName = "none" | "add" | "remove";

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
  | {
      // A bare constructor type (`new (...) => R`). Rust's `to_json_value`
      // emits the identical payload as `function` with `kind:
      // "constructorType"`. The bridge maps it function-like — the
      // constructor-vs-function distinction is consumed in Rust before the
      // wire (Vue runtime-ctor reducer + wire-graph builder).
      kind: "constructorType";
      parameters: NativeFunctionParam[];
      returnType?: NativeTypeExpr;
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
      // The Rust producer (`TypeExpr::Mapped` -> `to_json_value`) emits the
      // optional / readonly modifier (`"none"` / `"add"` / `"remove"`) and the
      // `as N` key-remap clause (`nameType`, a nested expr or `null`). They are
      // optional on the wire — an absent field means "no-op" (`MappedModifier::
      // None` / no remap). `readMappedInfo` reads them so a modifier- or
      // remap-bearing native mapped bails out of the `Record` recovery.
      optional?: MappedModifierName;
      readonly?: MappedModifierName;
      nameType?: NativeTypeExpr | null;
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

export type NativeTypeExprLike = NativeTypeExpr | GraphTypeExprRef;

type NativeTypeRegistry = Map<string, NativeTypeExprLike>;

interface FinitePropertyEntry {
  name: string;
  optional: boolean;
}

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
  expr: NativeTypeExprLike,
  nativeRegistry?: NativeTypeRegistry,
  visiting: Set<string> = new Set(),
  graphVisiting: Set<number> = new Set(),
): TypeDescriptor {
  if (isGraphTypeExprRef(expr)) {
    return graphTypeExprToDescriptor(expr, nativeRegistry, visiting, graphVisiting);
  }

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
      return union(
        expr.types.map((type) =>
          typeExprToDescriptor(type, nativeRegistry, visiting, graphVisiting),
        ),
      );

    case "intersection":
      return simplifyIntersectionDescriptor(
        expr.types.map((type) =>
          typeExprToDescriptor(type, nativeRegistry, visiting, graphVisiting),
        ),
      );

    case "array":
      return array(typeExprToDescriptor(expr.element, nativeRegistry, visiting, graphVisiting));

    case "tuple": {
      const elements = expr.elements.map((e) =>
        typeExprToDescriptor(e.ty, nativeRegistry, visiting, graphVisiting),
      );
      // Preserve per-element labels. Producers emit `label: string | null`
      // (Rust `TupleElement.label: Option<String>`); we mirror that across the
      // bridge so renderers can produce `[label: type]` output instead of the
      // pre-fix `[{ label: type }]` shape (which leaked the typed schema into
      // user-visible display text).
      const labels = expr.elements.map((e) => e.label ?? null);
      const t = tuple(elements);
      // Attach `labels` only when at least one position has a label — the
      // schema rule is "absent labels === all anonymous".
      if (labels.some((l) => l !== null)) {
        return { ...t, labels };
      }
      return t;
    }

    case "object": {
      const props = expr.properties
        .filter((m) => m.memberKind === "property" || m.memberKind === "method")
        .map((m) => ({
          name: m.name ?? "",
          type:
            m.memberKind === "method" && m.function
              ? nativeFunctionToDescriptor(m.function, nativeRegistry, visiting, graphVisiting)
              : m.ty
                ? typeExprToDescriptor(m.ty, nativeRegistry, visiting, graphVisiting)
                : primitive("any"),
          optional: m.optional ?? false,
        }));
      const indexSignatures = expr.properties
        .filter((m) => m.memberKind === "indexSignature")
        .map((m) => ({
          keyName: m.keyName ?? "key",
          keyType: m.keyType
            ? typeExprToDescriptor(m.keyType, nativeRegistry, visiting, graphVisiting)
            : primitive("string"),
          valueType: m.valueType
            ? typeExprToDescriptor(m.valueType, nativeRegistry, visiting, graphVisiting)
            : primitive("any"),
          ...(m.readonly ? { readonly: true } : {}),
        }));
      const callSignatures = expr.properties
        .filter((m) => m.memberKind === "callSignature" && m.function)
        .map((m) =>
          nativeFunctionToDescriptor(m.function!, nativeRegistry, visiting, graphVisiting),
        );
      const constructSignatures = expr.properties
        .filter((m) => m.memberKind === "constructSignature" && m.function)
        .map((m) =>
          nativeFunctionToDescriptor(m.function!, nativeRegistry, visiting, graphVisiting),
        );
      return object(props, {
        ...(indexSignatures.length > 0 ? { indexSignatures } : {}),
        ...(callSignatures.length > 0 ? { callSignatures } : {}),
        ...(constructSignatures.length > 0 ? { constructSignatures } : {}),
      });
    }

    // `function` and `constructorType` share the identical native payload
    // (parameters / returnType / typeParameters) and both map to a function-
    // like descriptor. The bare-constructor-vs-function distinction is
    // consumed in Rust before this bridge, so a `constructorType` node is
    // treated function-like rather than left as an unrecognised `unknown`.
    case "function":
    case "constructorType": {
      const params = expr.parameters.map((p) => ({
        name: p.name ?? "",
        type: typeExprToDescriptor(p.ty, nativeRegistry, visiting, graphVisiting),
        optional: p.optional,
      }));
      const returnType = expr.returnType
        ? typeExprToDescriptor(expr.returnType, nativeRegistry, visiting, graphVisiting)
        : primitive("void");
      return func(params, returnType, {
        typeParameters: expr.typeParameters?.map((typeParam) =>
          nativeTypeParameterToDescriptor(typeParam, nativeRegistry, visiting, graphVisiting),
        ),
      });
    }

    case "ref":
      if (expr.typeArguments.length > 0) {
        const resolved = resolveRegistryRefDescriptor(
          expr.name,
          expr.typeArguments,
          nativeRegistry,
          visiting,
          graphVisiting,
        );
        if (resolved) {
          return resolved;
        }
        return typeRef(
          expr.name,
          expr.typeArguments.map((typeArgument) =>
            typeExprToDescriptor(typeArgument, nativeRegistry, visiting, graphVisiting),
          ),
        );
      }
      return typeRef(expr.name);

    case "recursiveRef":
      return recursiveRef(
        expr.name,
        expr.typeArguments.map((typeArgument) =>
          typeExprToDescriptor(typeArgument, nativeRegistry, visiting, graphVisiting),
        ),
        expr.conditionalContext.map((frame) => ({
          branch: frame.branch,
          decided: frame.decided,
          check: typeExprToDescriptor(frame.check, nativeRegistry, visiting, graphVisiting),
          extends: typeExprToDescriptor(frame.extends, nativeRegistry, visiting, graphVisiting),
        })),
      );

    case "typeParameter":
      return nativeTypeParameterToDescriptor(expr, nativeRegistry, visiting, graphVisiting);

    case "keyOf": {
      const resolved = resolveKeyOfDescriptor(
        expr.operand,
        nativeRegistry,
        visiting,
        graphVisiting,
      );
      return resolved ?? unknown(nativeTypeExprToString(expr));
    }
    case "typeOf":
    case "conditional":
    case "templateLiteral":
    case "infer":
    case "rest":
      // These operator forms should be evaluated by the native evaluator.
      // If they reach here, they couldn't be reduced — fall back to unknown.
      return unknown(nativeTypeExprToString(expr));
    case "mapped": {
      const resolved = resolveMappedDescriptor(expr, nativeRegistry, visiting, graphVisiting);
      return resolved ?? unknown(nativeTypeExprToString(expr));
    }

    case "indexedAccess": {
      const resolved = nativeRegistry
        ? resolveIndexedAccessDescriptor(
            expr.object,
            expr.index,
            nativeRegistry,
            visiting,
            graphVisiting,
          )
        : undefined;
      if (resolved) {
        return resolved;
      }
      return indexedAccess(
        typeExprToDescriptor(expr.object, nativeRegistry, visiting, graphVisiting),
        typeExprToDescriptor(expr.index, nativeRegistry, visiting, graphVisiting),
      );
    }

    case "parenthesized":
      return typeExprToDescriptor(expr.inner, nativeRegistry, visiting, graphVisiting);

    case "unknown":
      return unknown(expr.raw);

    case "syntheticSlotBinding":
      return syntheticSlotBinding(
        expr.scopeCanonicalId,
        expr.surfaceKind,
        expr.bindingName,
        expr.valueNode,
        expr.slotName,
      );

    default:
      return unknown("unrecognized");
  }
}

function nativeFunctionToDescriptor(
  expr: NativeFunctionExpr,
  nativeRegistry?: NativeTypeRegistry,
  visiting: Set<string> = new Set(),
  graphVisiting: Set<number> = new Set(),
) {
  return func(
    (expr.parameters ?? []).map((p) => ({
      name: p.name ?? "",
      type: typeExprToDescriptor(p.ty, nativeRegistry, visiting, graphVisiting),
      optional: p.optional,
    })),
    expr.returnType
      ? typeExprToDescriptor(expr.returnType, nativeRegistry, visiting, graphVisiting)
      : primitive("void"),
    {
      typeParameters: expr.typeParameters?.map((typeParam) =>
        nativeTypeParameterToDescriptor(typeParam, nativeRegistry, visiting, graphVisiting),
      ),
    },
  );
}

function nativeTypeParameterToDescriptor(
  expr: NativeTypeParameter,
  nativeRegistry?: NativeTypeRegistry,
  visiting: Set<string> = new Set(),
  graphVisiting: Set<number> = new Set(),
) {
  return typeParameter(expr.name, {
    ...(expr.constraint
      ? {
          constraint: typeExprToDescriptor(
            expr.constraint,
            nativeRegistry,
            visiting,
            graphVisiting,
          ),
        }
      : {}),
    ...(expr.default
      ? { default: typeExprToDescriptor(expr.default, nativeRegistry, visiting, graphVisiting) }
      : {}),
  });
}

function resolveIndexedAccessDescriptor(
  objectExpr: NativeTypeExprLike,
  indexExpr: NativeTypeExprLike,
  nativeRegistry: NativeTypeRegistry,
  visiting: Set<string>,
  graphVisiting: Set<number>,
): TypeDescriptor | undefined {
  const resolvedObject = resolveRegistryExpr(objectExpr, nativeRegistry, visiting);
  const resolvedIndex = resolveRegistryExpr(indexExpr, nativeRegistry, visiting);
  const propertyName = resolveStringLiteralValue(resolvedIndex);
  if (propertyName === undefined) {
    return undefined;
  }

  const property = resolveObjectProperty(
    resolvedObject,
    propertyName,
    nativeRegistry,
    visiting,
    graphVisiting,
  );
  if (!property) {
    return undefined;
  }

  if (!property.optional) {
    return property.ty;
  }

  return union([property.ty, primitive("undefined")]);
}

function resolveRegistryExpr(
  expr: NativeTypeExprLike,
  nativeRegistry: NativeTypeRegistry,
  visiting: Set<string>,
): NativeTypeExprLike {
  if (isGraphTypeExprRef(expr)) {
    const node = expr.graph.getNode(expr.nodeId);
    if (node.kind === NODE_PARENTHESIZED) {
      return resolveRegistryExpr(
        createGraphTypeExprRef(expr.graph, node.innerNodeId),
        nativeRegistry,
        visiting,
      );
    }
    if (node.kind === NODE_REF && node.typeArgumentNodeIds.length === 0) {
      const name = expr.graph.getString(node.nameId);
      if (visiting.has(name)) {
        return expr;
      }
      const resolved = nativeRegistry.get(name);
      if (!resolved) {
        return expr;
      }
      visiting.add(name);
      const next = resolveRegistryExpr(resolved, nativeRegistry, visiting);
      visiting.delete(name);
      return next;
    }
    if (node.kind === NODE_TYPE_PARAMETER) {
      if (node.defaultNodeId) {
        return resolveRegistryExpr(
          createGraphTypeExprRef(expr.graph, node.defaultNodeId),
          nativeRegistry,
          visiting,
        );
      }
      if (node.constraintNodeId) {
        return resolveRegistryExpr(
          createGraphTypeExprRef(expr.graph, node.constraintNodeId),
          nativeRegistry,
          visiting,
        );
      }
    }
    return expr;
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
    const next = resolveRegistryExpr(resolved, nativeRegistry, visiting);
    visiting.delete(expr.name);
    return next;
  }

  if (expr.kind === "parenthesized") {
    return resolveRegistryExpr(expr.inner, nativeRegistry, visiting);
  }

  if (expr.kind === "typeParameter") {
    if (expr.default) {
      return resolveRegistryExpr(expr.default, nativeRegistry, visiting);
    }
    if (expr.constraint) {
      return resolveRegistryExpr(expr.constraint, nativeRegistry, visiting);
    }
  }

  return expr;
}

function resolveStringLiteralValue(expr: NativeTypeExprLike): string | undefined {
  if (isGraphTypeExprRef(expr)) {
    const node = expr.graph.getNode(expr.nodeId);
    if (node.kind === NODE_LITERAL && node.literalKind === LITERAL_STRING && node.stringId) {
      return expr.graph.getString(node.stringId);
    }
    return undefined;
  }

  if (expr.kind === "literal" && expr.literalKind === "string") {
    return expr.value;
  }

  return undefined;
}

function isSameTypeExprReference(left: NativeTypeExprLike, right: NativeTypeExprLike): boolean {
  if (isGraphTypeExprRef(left) && isGraphTypeExprRef(right)) {
    return left.graph === right.graph && left.nodeId === right.nodeId;
  }

  return left === right;
}

function resolveRegistryRefDescriptor(
  name: string,
  typeArguments: NativeTypeExprLike[],
  nativeRegistry: NativeTypeRegistry | undefined,
  visiting: Set<string>,
  graphVisiting: Set<number>,
): TypeDescriptor | undefined {
  if (!nativeRegistry || visiting.has(name)) {
    return undefined;
  }

  const resolved = nativeRegistry.get(name);
  if (!resolved) {
    return undefined;
  }

  visiting.add(name);
  try {
    const base = typeExprToDescriptor(resolved, nativeRegistry, visiting, graphVisiting);
    if (typeArguments.length === 0) {
      return base;
    }

    const parameterNames = collectDescriptorTypeParameterNames(base);
    if (parameterNames.length === 0) {
      return base;
    }

    const bindings = new Map<string, TypeDescriptor>();
    parameterNames.forEach((parameterName, index) => {
      const typeArgument = typeArguments[index];
      if (!typeArgument) {
        return;
      }
      bindings.set(
        parameterName,
        typeExprToDescriptor(typeArgument, nativeRegistry, visiting, graphVisiting),
      );
    });
    return substituteDescriptorTypeParameters(base, bindings);
  } finally {
    visiting.delete(name);
  }
}

function collectDescriptorTypeParameterNames(descriptor: TypeDescriptor): string[] {
  const names: string[] = [];
  const seen = new Set<string>();

  const visit = (current: TypeDescriptor) => {
    switch (current.kind) {
      case "primitive":
      case "literal":
      case "enum":
      case "unknown":
      case "syntheticSlotBinding":
        // Leaf kinds carry no nested `TypeDescriptor` children to walk.
        return;
      case "union":
      case "intersection":
        current.types.forEach(visit);
        return;
      case "array":
        visit(current.element);
        return;
      case "tuple":
        current.elements.forEach(visit);
        return;
      case "object":
        current.properties.forEach((property) => visit(property.type));
        current.indexSignatures?.forEach((signature) => {
          visit(signature.keyType);
          visit(signature.valueType);
        });
        current.callSignatures?.forEach(visit);
        current.constructSignatures?.forEach(visit);
        return;
      case "function":
        current.parameters.forEach((parameter) => visit(parameter.type));
        visit(current.returnType);
        current.typeParameters?.forEach(visit);
        return;
      case "ref":
        current.typeArguments?.forEach(visit);
        return;
      case "indexedAccess":
        // A mapped key parameter can hide in EITHER position of an indexed
        // access (`T[P]` is `{ objectType: T, indexType: P }`); walk both.
        visit(current.objectType);
        visit(current.indexType);
        return;
      case "recursiveRef":
        current.typeArguments.forEach(visit);
        current.conditionalContext.forEach((frame) => {
          visit(frame.check);
          visit(frame.extends);
        });
        return;
      case "typeParameter":
        if (!seen.has(current.name)) {
          seen.add(current.name);
          names.push(current.name);
        }
        if (current.constraint) {
          visit(current.constraint);
        }
        if (current.default) {
          visit(current.default);
        }
        return;
    }
  };

  visit(descriptor);
  return names;
}

/**
 * Structural predicate: does `descriptor` contain an `unknown` node anywhere in
 * its tree?
 *
 * An `unknown(...)` is the bridge's residue for a type it could NOT fully
 * understand — e.g. an unsupported operator (`conditional` / `template literal`
 * / `infer` / `rest`) that may itself reference a mapped key parameter the
 * type-parameter collector cannot see through. The open-key-domain `Record`
 * recovery uses this to refuse a value carrying any such residue: `Record<K, V>`
 * is only sound when `V` is fully understood AND provably key-independent.
 */
function descriptorContainsUnknown(descriptor: TypeDescriptor): boolean {
  switch (descriptor.kind) {
    case "unknown":
      return true;
    case "primitive":
    case "literal":
    case "enum":
    case "syntheticSlotBinding":
      return false;
    case "typeParameter":
      // A type parameter is NOT a leaf: an `unknown(...)` residue (e.g. an
      // unsupported operator such as a conditional `P extends … ? … : …`) can
      // hide inside its `constraint` or `default` — `<Q = P extends X ? A : B>`
      // lowers `Q.default` to `unknown(...)`. Recurse both sub-positions so the
      // open-key-domain `Record` recovery refuses any value carrying such
      // residue. (The `function` branch enumerates each `typeParameters` entry
      // but relies on this case to inspect its constraint/default.)
      return (
        (descriptor.constraint !== undefined && descriptorContainsUnknown(descriptor.constraint)) ||
        (descriptor.default !== undefined && descriptorContainsUnknown(descriptor.default))
      );
    case "union":
    case "intersection":
      return descriptor.types.some(descriptorContainsUnknown);
    case "array":
      return descriptorContainsUnknown(descriptor.element);
    case "tuple":
      return descriptor.elements.some(descriptorContainsUnknown);
    case "object":
      return (
        descriptor.properties.some((property) => descriptorContainsUnknown(property.type)) ||
        (descriptor.indexSignatures?.some(
          (signature) =>
            descriptorContainsUnknown(signature.keyType) ||
            descriptorContainsUnknown(signature.valueType),
        ) ??
          false) ||
        (descriptor.callSignatures?.some(descriptorContainsUnknown) ?? false) ||
        (descriptor.constructSignatures?.some(descriptorContainsUnknown) ?? false)
      );
    case "function":
      return (
        descriptor.parameters.some((parameter) => descriptorContainsUnknown(parameter.type)) ||
        descriptorContainsUnknown(descriptor.returnType) ||
        (descriptor.typeParameters?.some(descriptorContainsUnknown) ?? false)
      );
    case "ref":
      return descriptor.typeArguments?.some(descriptorContainsUnknown) ?? false;
    case "indexedAccess":
      return (
        descriptorContainsUnknown(descriptor.objectType) ||
        descriptorContainsUnknown(descriptor.indexType)
      );
    case "recursiveRef":
      return (
        descriptor.typeArguments.some(descriptorContainsUnknown) ||
        descriptor.conditionalContext.some(
          (frame) =>
            descriptorContainsUnknown(frame.check) || descriptorContainsUnknown(frame.extends),
        )
      );
  }
}

function maskTypeParameterBindings(
  bindings: Map<string, TypeDescriptor>,
  names: string[],
): Map<string, TypeDescriptor> {
  if (names.length === 0) {
    return bindings;
  }

  const masked = new Map(bindings);
  names.forEach((name) => masked.delete(name));
  return masked;
}

function substituteDescriptorTypeParameters(
  descriptor: TypeDescriptor,
  bindings: Map<string, TypeDescriptor>,
): TypeDescriptor {
  switch (descriptor.kind) {
    case "primitive":
    case "literal":
    case "enum":
    case "unknown":
      return descriptor;
    case "union":
      return union(
        descriptor.types.map((type) => substituteDescriptorTypeParameters(type, bindings)),
      );
    case "intersection":
      return intersection(
        descriptor.types.map((type) => substituteDescriptorTypeParameters(type, bindings)),
      );
    case "array":
      return array(substituteDescriptorTypeParameters(descriptor.element, bindings));
    case "tuple": {
      const t = tuple(
        descriptor.elements.map((element) => substituteDescriptorTypeParameters(element, bindings)),
      );
      // Preserve labels through type-parameter substitution.
      if (descriptor.labels) {
        return { ...t, labels: descriptor.labels };
      }
      return t;
    }
    case "object":
      return object(
        descriptor.properties.map((property) => ({
          ...property,
          type: substituteDescriptorTypeParameters(property.type, bindings),
        })),
        {
          ...(descriptor.indexSignatures
            ? {
                indexSignatures: descriptor.indexSignatures.map((signature) => ({
                  ...signature,
                  keyType: substituteDescriptorTypeParameters(signature.keyType, bindings),
                  valueType: substituteDescriptorTypeParameters(signature.valueType, bindings),
                })),
              }
            : {}),
          ...(descriptor.callSignatures
            ? {
                callSignatures: descriptor.callSignatures.map((signature) =>
                  substituteDescriptorTypeParameters(signature, bindings),
                ) as Extract<TypeDescriptor, { kind: "function" }>[],
              }
            : {}),
          ...(descriptor.constructSignatures
            ? {
                constructSignatures: descriptor.constructSignatures.map((signature) =>
                  substituteDescriptorTypeParameters(signature, bindings),
                ) as Extract<TypeDescriptor, { kind: "function" }>[],
              }
            : {}),
        },
      );
    case "function": {
      const maskedBindings = maskTypeParameterBindings(
        bindings,
        descriptor.typeParameters?.map((typeParameter) => typeParameter.name) ?? [],
      );
      return func(
        descriptor.parameters.map((parameter) => ({
          ...parameter,
          type: substituteDescriptorTypeParameters(parameter.type, maskedBindings),
        })),
        substituteDescriptorTypeParameters(descriptor.returnType, maskedBindings),
        {
          ...(descriptor.typeParameters
            ? {
                typeParameters: descriptor.typeParameters.map((typeParameterDescriptor) =>
                  substituteDescriptorTypeParameters(typeParameterDescriptor, maskedBindings),
                ) as Extract<TypeDescriptor, { kind: "typeParameter" }>[],
              }
            : {}),
        },
      );
    }
    case "ref":
      return typeRef(
        descriptor.name,
        descriptor.typeArguments?.map((typeArgument) =>
          substituteDescriptorTypeParameters(typeArgument, bindings),
        ),
      );
    case "recursiveRef":
      return recursiveRef(
        descriptor.name,
        descriptor.typeArguments.map((typeArgument) =>
          substituteDescriptorTypeParameters(typeArgument, bindings),
        ),
        descriptor.conditionalContext.map((frame) => ({
          ...frame,
          check: substituteDescriptorTypeParameters(frame.check, bindings),
          extends: substituteDescriptorTypeParameters(frame.extends, bindings),
        })),
      );
    case "typeParameter": {
      const bound = bindings.get(descriptor.name);
      if (bound) {
        return bound;
      }
      if (descriptor.default) {
        return substituteDescriptorTypeParameters(descriptor.default, bindings);
      }
      if (descriptor.constraint) {
        return substituteDescriptorTypeParameters(descriptor.constraint, bindings);
      }
      return descriptor;
    }
    case "indexedAccess":
      return indexedAccess(
        substituteDescriptorTypeParameters(descriptor.objectType, bindings),
        substituteDescriptorTypeParameters(descriptor.indexType, bindings),
      );
    case "syntheticSlotBinding":
      // Synthetic carriers are inert terminals — no type-parameter bindings
      // can apply through them.
      return descriptor;
  }
}

function resolveRefProperty(
  name: string,
  typeArguments: NativeTypeExprLike[],
  propertyName: string,
  nativeRegistry: NativeTypeRegistry,
  visiting: Set<string>,
  graphVisiting: Set<number>,
): { ty: TypeDescriptor; optional: boolean } | undefined {
  if (
    (name === "Required" || name === "Partial" || name === "Readonly" || name === "Id") &&
    typeArguments.length > 0
  ) {
    const property = resolveObjectProperty(
      typeArguments[0],
      propertyName,
      nativeRegistry,
      visiting,
      graphVisiting,
    );
    if (!property) {
      return undefined;
    }
    if (name === "Required") {
      return { ...property, optional: false };
    }
    if (name === "Partial") {
      return { ...property, optional: true };
    }
    return property;
  }

  const descriptor = resolveRegistryRefDescriptor(
    name,
    typeArguments,
    nativeRegistry,
    visiting,
    graphVisiting,
  );
  return descriptor
    ? resolveDescriptorProperty(descriptor, propertyName, nativeRegistry, visiting, graphVisiting)
    : undefined;
}

function resolveObjectProperty(
  expr: NativeTypeExprLike,
  propertyName: string,
  nativeRegistry: NativeTypeRegistry,
  visiting: Set<string>,
  graphVisiting: Set<number>,
): { ty: TypeDescriptor; optional: boolean } | undefined {
  if (isGraphTypeExprRef(expr)) {
    const node = expr.graph.getNode(expr.nodeId);
    switch (node.kind) {
      case NODE_OBJECT: {
        const member = node.members.find(
          (candidate) =>
            candidate.kind === MEMBER_PROPERTY &&
            candidate.nameId !== 0 &&
            expr.graph.getString(candidate.nameId) === propertyName,
        );
        if (!member) {
          return undefined;
        }

        return {
          ty: typeExprToDescriptor(
            createGraphTypeExprRef(expr.graph, member.typeNodeId),
            nativeRegistry,
            visiting,
            graphVisiting,
          ),
          optional: member.optional,
        };
      }
      case NODE_UNION:
        return resolveUnionObjectProperty(
          node.typeNodeIds.map((id) => createGraphTypeExprRef(expr.graph, id)),
          propertyName,
          nativeRegistry,
          visiting,
          graphVisiting,
        );
      case NODE_INTERSECTION:
        return resolveIntersectionObjectProperty(
          node.typeNodeIds.map((id) => createGraphTypeExprRef(expr.graph, id)),
          propertyName,
          nativeRegistry,
          visiting,
          graphVisiting,
        );
      case NODE_PARENTHESIZED:
        return resolveObjectProperty(
          createGraphTypeExprRef(expr.graph, node.innerNodeId),
          propertyName,
          nativeRegistry,
          visiting,
          graphVisiting,
        );
      case NODE_TYPE_PARAMETER: {
        const resolved = resolveRegistryExpr(expr, nativeRegistry, visiting);
        return isSameTypeExprReference(resolved, expr)
          ? undefined
          : resolveObjectProperty(resolved, propertyName, nativeRegistry, visiting, graphVisiting);
      }
      case NODE_INDEXED_ACCESS: {
        const resolved = resolveIndexedAccessDescriptor(
          createGraphTypeExprRef(expr.graph, node.objectNodeId),
          createGraphTypeExprRef(expr.graph, node.indexNodeId),
          nativeRegistry,
          visiting,
          graphVisiting,
        );
        return resolved
          ? resolveDescriptorProperty(
              resolved,
              propertyName,
              nativeRegistry,
              visiting,
              graphVisiting,
            )
          : undefined;
      }
      case NODE_REF:
        return resolveRefProperty(
          expr.graph.getString(node.nameId),
          node.typeArgumentNodeIds.map((id) => createGraphTypeExprRef(expr.graph, id)),
          propertyName,
          nativeRegistry,
          visiting,
          graphVisiting,
        );
      default:
        return undefined;
    }
  }

  switch (expr.kind) {
    case "object": {
      const member = expr.properties.find(
        (candidate) => candidate.memberKind === "property" && candidate.name === propertyName,
      );
      if (!member?.ty) {
        return undefined;
      }

      return {
        ty: typeExprToDescriptor(member.ty, nativeRegistry, visiting, graphVisiting),
        optional: member.optional ?? false,
      };
    }
    case "union":
      return resolveUnionObjectProperty(
        expr.types,
        propertyName,
        nativeRegistry,
        visiting,
        graphVisiting,
      );
    case "intersection":
      return resolveIntersectionObjectProperty(
        expr.types,
        propertyName,
        nativeRegistry,
        visiting,
        graphVisiting,
      );
    case "parenthesized":
      return resolveObjectProperty(
        expr.inner,
        propertyName,
        nativeRegistry,
        visiting,
        graphVisiting,
      );
    case "typeParameter": {
      const resolved = resolveRegistryExpr(expr, nativeRegistry, visiting);
      return isSameTypeExprReference(resolved, expr)
        ? undefined
        : resolveObjectProperty(resolved, propertyName, nativeRegistry, visiting, graphVisiting);
    }
    case "indexedAccess": {
      const resolved = resolveIndexedAccessDescriptor(
        expr.object,
        expr.index,
        nativeRegistry,
        visiting,
        graphVisiting,
      );
      return resolved
        ? resolveDescriptorProperty(resolved, propertyName, nativeRegistry, visiting, graphVisiting)
        : undefined;
    }
    case "ref":
      return resolveRefProperty(
        expr.name,
        expr.typeArguments,
        propertyName,
        nativeRegistry,
        visiting,
        graphVisiting,
      );
    default:
      return undefined;
  }
}

function resolveDescriptorProperty(
  descriptor: TypeDescriptor,
  propertyName: string,
  nativeRegistry: NativeTypeRegistry,
  visiting: Set<string>,
  graphVisiting: Set<number>,
): { ty: TypeDescriptor; optional: boolean } | undefined {
  switch (descriptor.kind) {
    case "object": {
      const property = descriptor.properties.find((candidate) => candidate.name === propertyName);
      if (!property) {
        return undefined;
      }
      return {
        ty: property.type,
        optional: property.optional,
      };
    }
    case "union": {
      const members = descriptor.types.map((candidate) =>
        resolveDescriptorProperty(candidate, propertyName, nativeRegistry, visiting, graphVisiting),
      );
      if (members.some((member) => !member)) {
        return undefined;
      }
      const resolved = members as Array<{ ty: TypeDescriptor; optional: boolean }>;
      return {
        ty: union(resolved.map((member) => member.ty)),
        optional: resolved.some((member) => member.optional),
      };
    }
    case "intersection": {
      const members = descriptor.types
        .map((candidate) =>
          resolveDescriptorProperty(
            candidate,
            propertyName,
            nativeRegistry,
            visiting,
            graphVisiting,
          ),
        )
        .filter(
          (member): member is { ty: TypeDescriptor; optional: boolean } => member !== undefined,
        );
      if (members.length === 0) {
        return undefined;
      }
      return {
        ty: intersection(members.map((member) => member.ty)),
        optional: members.every((member) => member.optional),
      };
    }
    case "ref": {
      if (descriptor.typeArguments?.length || visiting.has(descriptor.name)) {
        return undefined;
      }
      const resolved = nativeRegistry.get(descriptor.name);
      if (!resolved) {
        return undefined;
      }
      visiting.add(descriptor.name);
      const next = typeExprToDescriptor(resolved, nativeRegistry, visiting, graphVisiting);
      const property = resolveDescriptorProperty(
        next,
        propertyName,
        nativeRegistry,
        visiting,
        graphVisiting,
      );
      visiting.delete(descriptor.name);
      return property;
    }
    default:
      return undefined;
  }
}

function resolveKeyOfDescriptor(
  expr: NativeTypeExprLike,
  nativeRegistry: NativeTypeRegistry | undefined,
  visiting: Set<string>,
  graphVisiting: Set<number>,
): TypeDescriptor | undefined {
  const entries = resolveFiniteSourceEntries(expr, nativeRegistry, visiting, graphVisiting);
  if (!entries || entries.length === 0) {
    return undefined;
  }
  return union(entries.map((entry) => literal(entry.name)));
}

function resolveMappedDescriptor(
  expr: NativeTypeExprLike,
  nativeRegistry: NativeTypeRegistry | undefined,
  visiting: Set<string>,
  graphVisiting: Set<number>,
): TypeDescriptor | undefined {
  const registry = nativeRegistry ?? new Map<string, NativeTypeExprLike>();

  const mapped = readMappedInfo(expr);
  if (!mapped) {
    return undefined;
  }

  const entries = resolveFiniteSourceEntries(mapped.source, registry, visiting, graphVisiting);
  if (!entries || entries.length === 0) {
    // The source key domain is not a finite enumeration (e.g. `string`). A
    // homomorphic mapping over an open `Record`-key domain is the
    // `Record<K, V>` alias — recover it instead of degrading to
    // `unknown("mapped")`.
    return resolveOpenKeyDomainMappedDescriptor(mapped, registry, visiting, graphVisiting);
  }

  return object(
    entries.map((entry) => ({
      name: entry.name,
      type: resolveMappedValueDescriptor(
        mapped.value,
        mapped.parameterName,
        entry.name,
        registry,
        visiting,
        graphVisiting,
      ),
      optional: applyMappedOptionalModifier(entry.optional, mapped.optionalModifier),
    })),
  );
}

/**
 * Recover the `Record<K, V>` alias for a mapped type `{ [P in K]: V }` whose
 * key domain `K` is OPEN (no finite key enumeration).
 *
 * `Record<K, V>` is defined as `{ [P in K]: V }` where the key `K` is a
 * `Record`-key primitive (`string` / `number` / `symbol`, or a union of those)
 * and the value `V` does not reference the mapped parameter `P`. When the
 * native producer surfaces `Record<string, any>` in its expanded mapped form,
 * reconstruct the alias so the compat display layer renders
 * `Record<string, any>` (matching `vue-component-meta`) rather than the lossy
 * `unknown("mapped")` placeholder.
 *
 * Any other open-domain shape returns `undefined`, preserving the existing
 * `unknown` fallback — this is a conservative recovery, not a blanket mapped
 * collapse. The bail conditions are:
 * - a key-remapping clause (`{ [P in K as N]: V }`) — `as` FILTERS (`as never`
 *   drops keys) or TRANSFORMS (`as `prefix_${P}`` renames) the produced key
 *   domain, so the surface is not a plain `Record<K, V>`;
 * - an added/removed optional (`+?` / `-?`) OR readonly (`+readonly` /
 *   `-readonly`) modifier — `Record` expresses neither;
 * - a non-`Record`-key source (e.g. `keyof <generic>`);
 * - a value that references the mapped parameter `P` (including `P` nested in
 *   an indexed access such as `T[P]`); OR
 * - a value carrying ANY `unknown` residue (e.g. an unsupported operator such
 *   as a conditional `P extends … ? … : …` that lowered to `unknown(...)` and
 *   may itself reference `P` in a way the collector cannot observe).
 */
function resolveOpenKeyDomainMappedDescriptor(
  mapped: {
    parameterName: string;
    source: NativeTypeExprLike;
    value: NativeTypeExprLike;
    optionalModifier: number;
    readonlyModifier: number;
    nameTypeNodeId: number;
  },
  nativeRegistry: NativeTypeRegistry,
  visiting: Set<string>,
  graphVisiting: Set<number>,
): TypeDescriptor | undefined {
  // A key-remapping clause (`{ [P in K as N]: V }`) FILTERS (`as never` drops
  // keys) or TRANSFORMS (`as `prefix_${P}`` renames) the produced key domain,
  // so the result is NOT a plain `Record<K, V>`. Refuse the recovery whenever a
  // name-type clause is present — the absent (no-remap) case is the sentinel
  // `0` (the legacy `NativeTypeExpr` mapped form has no remap and reports `0`).
  if (mapped.nameTypeNodeId !== 0) {
    return undefined;
  }
  // `Record<K, V>` is a non-modifying homomorphic mapping. An added
  // (`MappedModifier::Add`, 2) or removed (`MappedModifier::Remove`, 3) optional
  // OR readonly modifier is a different type that `Record` cannot express.
  if (mapped.optionalModifier === 2 || mapped.optionalModifier === 3) {
    return undefined;
  }
  if (mapped.readonlyModifier === 2 || mapped.readonlyModifier === 3) {
    return undefined;
  }

  const keyDescriptor = typeExprToDescriptor(
    mapped.source,
    nativeRegistry,
    visiting,
    graphVisiting,
  );
  if (!isRecordKeyDomainDescriptor(keyDescriptor)) {
    return undefined;
  }

  const valueDescriptor = typeExprToDescriptor(
    mapped.value,
    nativeRegistry,
    visiting,
    graphVisiting,
  );
  // A `Record` value must be FULLY understood. Any `unknown` residue (e.g. an
  // unsupported operator that lowered to `unknown(...)`) could hide a reference
  // to the mapped key `P`, so refuse the recovery on any uncertainty.
  if (descriptorContainsUnknown(valueDescriptor)) {
    return undefined;
  }
  // A `Record` value is fixed — it does not reference the mapped key `P`
  // anywhere (top-level OR nested, e.g. the index position of `T[P]`).
  if (collectDescriptorTypeParameterNames(valueDescriptor).includes(mapped.parameterName)) {
    return undefined;
  }

  return typeRef("Record", [keyDescriptor, valueDescriptor]);
}

/**
 * Structural test: is `descriptor` a valid `Record` key domain — a
 * `string` / `number` / `symbol` primitive, or a union of those?
 */
function isRecordKeyDomainDescriptor(descriptor: TypeDescriptor): boolean {
  if (descriptor.kind === "primitive") {
    return (
      descriptor.name === "string" || descriptor.name === "number" || descriptor.name === "symbol"
    );
  }
  if (descriptor.kind === "union") {
    return descriptor.types.every(isRecordKeyDomainDescriptor);
  }
  return false;
}

function resolveMappedValueDescriptor(
  expr: NativeTypeExprLike,
  parameterName: string,
  propertyName: string,
  nativeRegistry: NativeTypeRegistry,
  visiting: Set<string>,
  graphVisiting: Set<number>,
): TypeDescriptor {
  if (isGraphTypeExprRef(expr)) {
    const node = expr.graph.getNode(expr.nodeId);
    switch (node.kind) {
      case NODE_KEY_OF: {
        const resolved =
          resolveFiniteSourceEntries(
            createGraphTypeExprRef(expr.graph, node.operandNodeId),
            nativeRegistry,
            visiting,
            graphVisiting,
          ) ??
          resolveFiniteDescriptorEntries(
            resolveMappedIndexedAccessDescriptor(
              createGraphTypeExprRef(expr.graph, node.operandNodeId),
              parameterName,
              propertyName,
              nativeRegistry,
              visiting,
              graphVisiting,
            ),
            nativeRegistry,
            visiting,
            graphVisiting,
          );
        return resolved && resolved.length > 0
          ? union(resolved.map((entry) => literal(entry.name)))
          : typeExprToDescriptor(expr, nativeRegistry, visiting, graphVisiting);
      }
      case NODE_INDEXED_ACCESS: {
        const resolved = resolveMappedIndexedAccessDescriptor(
          expr,
          parameterName,
          propertyName,
          nativeRegistry,
          visiting,
          graphVisiting,
        );
        return resolved ?? typeExprToDescriptor(expr, nativeRegistry, visiting, graphVisiting);
      }
      case NODE_TYPE_PARAMETER:
        if (expr.graph.getString(node.nameId) === parameterName) {
          return literal(propertyName);
        }
        break;
      case NODE_PARENTHESIZED:
        return resolveMappedValueDescriptor(
          createGraphTypeExprRef(expr.graph, node.innerNodeId),
          parameterName,
          propertyName,
          nativeRegistry,
          visiting,
          graphVisiting,
        );
    }
  } else {
    switch (expr.kind) {
      case "keyOf": {
        const resolved =
          resolveFiniteSourceEntries(expr.operand, nativeRegistry, visiting, graphVisiting) ??
          resolveFiniteDescriptorEntries(
            resolveMappedIndexedAccessDescriptor(
              expr.operand,
              parameterName,
              propertyName,
              nativeRegistry,
              visiting,
              graphVisiting,
            ),
            nativeRegistry,
            visiting,
            graphVisiting,
          );
        return resolved && resolved.length > 0
          ? union(resolved.map((entry) => literal(entry.name)))
          : typeExprToDescriptor(expr, nativeRegistry, visiting, graphVisiting);
      }
      case "indexedAccess": {
        const resolved = resolveMappedIndexedAccessDescriptor(
          expr,
          parameterName,
          propertyName,
          nativeRegistry,
          visiting,
          graphVisiting,
        );
        return resolved ?? typeExprToDescriptor(expr, nativeRegistry, visiting, graphVisiting);
      }
      case "typeParameter":
        if (expr.name === parameterName) {
          return literal(propertyName);
        }
        break;
      case "parenthesized":
        return resolveMappedValueDescriptor(
          expr.inner,
          parameterName,
          propertyName,
          nativeRegistry,
          visiting,
          graphVisiting,
        );
    }
  }

  return typeExprToDescriptor(expr, nativeRegistry, visiting, graphVisiting);
}

function resolveMappedIndexedAccessDescriptor(
  expr: NativeTypeExprLike,
  parameterName: string,
  propertyName: string,
  nativeRegistry: NativeTypeRegistry,
  visiting: Set<string>,
  graphVisiting: Set<number>,
): TypeDescriptor | undefined {
  const parts = readIndexedAccessInfo(expr);
  if (!parts) {
    return undefined;
  }
  const index = mappedIndexMatchesParameter(parts.index, parameterName)
    ? ({
        kind: "literal",
        literalKind: "string",
        value: propertyName,
      } satisfies NativeTypeExpr)
    : parts.index;
  return resolveIndexedAccessDescriptor(
    parts.object,
    index,
    nativeRegistry,
    visiting,
    graphVisiting,
  );
}

function resolveFiniteSourceEntries(
  expr: NativeTypeExprLike,
  nativeRegistry: NativeTypeRegistry | undefined,
  visiting: Set<string>,
  graphVisiting: Set<number>,
): FinitePropertyEntry[] | undefined {
  if (isGraphTypeExprRef(expr)) {
    const node = expr.graph.getNode(expr.nodeId);
    switch (node.kind) {
      case NODE_KEY_OF:
        return resolveFiniteObjectEntries(
          createGraphTypeExprRef(expr.graph, node.operandNodeId),
          nativeRegistry,
          visiting,
          graphVisiting,
        );
      case NODE_UNION: {
        const entries = node.typeNodeIds.map((id) =>
          resolveStringLiteralValue(createGraphTypeExprRef(expr.graph, id)),
        );
        return entries.every((entry): entry is string => entry !== undefined)
          ? entries.map((name) => ({ name, optional: false }))
          : undefined;
      }
      default:
        return resolveFiniteObjectEntries(expr, nativeRegistry, visiting, graphVisiting);
    }
  }

  switch (expr.kind) {
    case "keyOf":
      return resolveFiniteObjectEntries(expr.operand, nativeRegistry, visiting, graphVisiting);
    case "union": {
      const entries = expr.types.map((type) => resolveStringLiteralValue(type));
      return entries.every((entry): entry is string => entry !== undefined)
        ? entries.map((name) => ({ name, optional: false }))
        : undefined;
    }
    default:
      return resolveFiniteObjectEntries(expr, nativeRegistry, visiting, graphVisiting);
  }
}

function resolveFiniteObjectEntries(
  expr: NativeTypeExprLike,
  nativeRegistry: NativeTypeRegistry | undefined,
  visiting: Set<string>,
  graphVisiting: Set<number>,
): FinitePropertyEntry[] | undefined {
  if (isGraphTypeExprRef(expr)) {
    const node = expr.graph.getNode(expr.nodeId);
    switch (node.kind) {
      case NODE_OBJECT:
        return node.members
          .filter((member) => member.kind === MEMBER_PROPERTY && member.nameId !== 0)
          .map((member) => ({
            name: expr.graph.getString(member.nameId),
            optional: member.optional,
          }));
      case NODE_INTERSECTION: {
        const entries = node.typeNodeIds
          .map((id) =>
            resolveFiniteObjectEntries(
              createGraphTypeExprRef(expr.graph, id),
              nativeRegistry,
              visiting,
              graphVisiting,
            ),
          )
          .filter((value): value is FinitePropertyEntry[] => value !== undefined);
        return entries.length > 0 ? mergeFiniteEntrySets(entries) : undefined;
      }
      case NODE_PARENTHESIZED:
        return resolveFiniteObjectEntries(
          createGraphTypeExprRef(expr.graph, node.innerNodeId),
          nativeRegistry,
          visiting,
          graphVisiting,
        );
      case NODE_TYPE_PARAMETER: {
        const resolved = resolveRegistryExpr(
          expr,
          nativeRegistry ?? new Map<string, NativeTypeExprLike>(),
          visiting,
        );
        return isSameTypeExprReference(resolved, expr)
          ? undefined
          : resolveFiniteObjectEntries(resolved, nativeRegistry, visiting, graphVisiting);
      }
      case NODE_REF:
        return resolveFiniteEntriesFromRef(
          expr.graph.getString(node.nameId),
          node.typeArgumentNodeIds.map((id) => createGraphTypeExprRef(expr.graph, id)),
          nativeRegistry,
          visiting,
          graphVisiting,
        );
      case NODE_INDEXED_ACCESS:
        return resolveFiniteDescriptorEntries(
          resolveIndexedAccessDescriptor(
            createGraphTypeExprRef(expr.graph, node.objectNodeId),
            createGraphTypeExprRef(expr.graph, node.indexNodeId),
            nativeRegistry ?? new Map<string, NativeTypeExprLike>(),
            visiting,
            graphVisiting,
          ),
          nativeRegistry,
          visiting,
          graphVisiting,
        );
      case NODE_MAPPED:
        return resolveFiniteDescriptorEntries(
          resolveMappedDescriptor(expr, nativeRegistry, visiting, graphVisiting),
          nativeRegistry,
          visiting,
          graphVisiting,
        );
      default:
        return undefined;
    }
  }

  switch (expr.kind) {
    case "object":
      return expr.properties
        .filter((member) => member.memberKind === "property" && member.name)
        .map((member) => ({
          name: member.name!,
          optional: member.optional ?? false,
        }));
    case "intersection": {
      const entries = expr.types
        .map((type) => resolveFiniteObjectEntries(type, nativeRegistry, visiting, graphVisiting))
        .filter((value): value is FinitePropertyEntry[] => value !== undefined);
      return entries.length > 0 ? mergeFiniteEntrySets(entries) : undefined;
    }
    case "parenthesized":
      return resolveFiniteObjectEntries(expr.inner, nativeRegistry, visiting, graphVisiting);
    case "typeParameter": {
      const resolved = resolveRegistryExpr(
        expr,
        nativeRegistry ?? new Map<string, NativeTypeExprLike>(),
        visiting,
      );
      return isSameTypeExprReference(resolved, expr)
        ? undefined
        : resolveFiniteObjectEntries(resolved, nativeRegistry, visiting, graphVisiting);
    }
    case "ref":
      return resolveFiniteEntriesFromRef(
        expr.name,
        expr.typeArguments,
        nativeRegistry,
        visiting,
        graphVisiting,
      );
    case "indexedAccess":
      return resolveFiniteDescriptorEntries(
        resolveIndexedAccessDescriptor(
          expr.object,
          expr.index,
          nativeRegistry ?? new Map<string, NativeTypeExprLike>(),
          visiting,
          graphVisiting,
        ),
        nativeRegistry,
        visiting,
        graphVisiting,
      );
    case "mapped":
      return resolveFiniteDescriptorEntries(
        resolveMappedDescriptor(expr, nativeRegistry, visiting, graphVisiting),
        nativeRegistry,
        visiting,
        graphVisiting,
      );
    default:
      return undefined;
  }
}

function resolveFiniteEntriesFromRef(
  name: string,
  typeArguments: NativeTypeExprLike[],
  nativeRegistry: NativeTypeRegistry | undefined,
  visiting: Set<string>,
  graphVisiting: Set<number>,
): FinitePropertyEntry[] | undefined {
  if (name === "Required" && typeArguments.length > 0) {
    return resolveFiniteObjectEntries(
      typeArguments[0],
      nativeRegistry,
      visiting,
      graphVisiting,
    )?.map((entry) => ({ ...entry, optional: false }));
  }
  if (name === "Partial" && typeArguments.length > 0) {
    return resolveFiniteObjectEntries(
      typeArguments[0],
      nativeRegistry,
      visiting,
      graphVisiting,
    )?.map((entry) => ({ ...entry, optional: true }));
  }
  if ((name === "Id" || name === "Readonly") && typeArguments.length > 0) {
    return resolveFiniteObjectEntries(typeArguments[0], nativeRegistry, visiting, graphVisiting);
  }
  const descriptor = resolveRegistryRefDescriptor(
    name,
    typeArguments,
    nativeRegistry,
    visiting,
    graphVisiting,
  );
  if (!descriptor) {
    return undefined;
  }
  return resolveFiniteDescriptorEntries(descriptor, nativeRegistry, visiting, graphVisiting);
}

function resolveFiniteDescriptorEntries(
  descriptor: TypeDescriptor | undefined,
  nativeRegistry: NativeTypeRegistry | undefined,
  visiting: Set<string>,
  graphVisiting: Set<number>,
): FinitePropertyEntry[] | undefined {
  if (!descriptor) {
    return undefined;
  }
  switch (descriptor.kind) {
    case "object":
      return descriptor.properties.map((property) => ({
        name: property.name,
        optional: property.optional,
      }));
    case "intersection": {
      const entries = descriptor.types
        .map((type) =>
          resolveFiniteDescriptorEntries(type, nativeRegistry, visiting, graphVisiting),
        )
        .filter((value): value is FinitePropertyEntry[] => value !== undefined);
      return entries.length > 0 ? mergeFiniteEntrySets(entries) : undefined;
    }
    case "ref":
      return resolveFiniteDescriptorEntries(
        resolveDescriptorRefDescriptor(
          descriptor.name,
          descriptor.typeArguments ?? [],
          nativeRegistry,
          visiting,
          graphVisiting,
        ),
        nativeRegistry,
        visiting,
        graphVisiting,
      );
    default:
      return undefined;
  }
}

function resolveDescriptorRefDescriptor(
  name: string,
  typeArguments: TypeDescriptor[],
  nativeRegistry: NativeTypeRegistry | undefined,
  visiting: Set<string>,
  graphVisiting: Set<number>,
): TypeDescriptor | undefined {
  if (!nativeRegistry || visiting.has(name)) {
    return undefined;
  }

  const resolved = nativeRegistry.get(name);
  if (!resolved) {
    return undefined;
  }

  visiting.add(name);
  try {
    const base = typeExprToDescriptor(resolved, nativeRegistry, visiting, graphVisiting);
    if (typeArguments.length === 0) {
      return base;
    }

    const parameterNames = collectDescriptorTypeParameterNames(base);
    if (parameterNames.length === 0) {
      return base;
    }

    const bindings = new Map<string, TypeDescriptor>();
    parameterNames.forEach((parameterName, index) => {
      const typeArgument = typeArguments[index];
      if (!typeArgument) {
        return;
      }
      bindings.set(parameterName, typeArgument);
    });

    return substituteDescriptorTypeParameters(base, bindings);
  } finally {
    visiting.delete(name);
  }
}

function mergeFiniteEntrySets(entrySets: FinitePropertyEntry[][]): FinitePropertyEntry[] {
  const merged = new Map<string, FinitePropertyEntry>();
  for (const entrySet of entrySets) {
    for (const entry of entrySet) {
      const existing = merged.get(entry.name);
      if (existing) {
        existing.optional = existing.optional && entry.optional;
      } else {
        merged.set(entry.name, { ...entry });
      }
    }
  }
  return [...merged.values()];
}

function applyMappedOptionalModifier(optional: boolean, modifier: number): boolean {
  if (modifier === 2) {
    return true;
  }
  if (modifier === 3) {
    return false;
  }
  return optional;
}

function readMappedInfo(expr: NativeTypeExprLike):
  | {
      parameterName: string;
      source: NativeTypeExprLike;
      value: NativeTypeExprLike;
      optionalModifier: number;
      readonlyModifier: number;
      nameTypeNodeId: number;
    }
  | undefined {
  if (isGraphTypeExprRef(expr)) {
    const node = expr.graph.getNode(expr.nodeId);
    if (node.kind !== NODE_MAPPED) {
      return undefined;
    }
    return {
      parameterName: expr.graph.getString(node.parameterId),
      source: createGraphTypeExprRef(expr.graph, node.sourceNodeId),
      value: createGraphTypeExprRef(expr.graph, node.valueNodeId),
      optionalModifier: node.optionalModifier,
      readonlyModifier: node.readonlyModifier,
      // The `[P in K as N]` key-remap clause node id; `0` is the absent
      // sentinel (a plain `{ [P in K]: V }` with no remapping).
      nameTypeNodeId: node.nameTypeNodeId,
    };
  }
  if (expr.kind !== "mapped") {
    return undefined;
  }
  // The native (raw-JSON) `NativeTypeExpr` mapped form DOES carry the optional /
  // readonly modifier and the `as N` key-remap clause (the Rust producer emits
  // `optional` / `readonly` / `nameType` on `TypeExpr::Mapped`). Read the actual
  // values into the shared graph encoding so a modifier- or remap-bearing native
  // mapped bails out of the `Record` recovery exactly like its graph
  // counterpart — never hard-code them absent. A genuinely-plain native mapped
  // (modifier absent / `"none"`, `nameType` null) decodes to the no-op encoding
  // (`1` / `1` / `0`) and still recovers `Record`. `nameTypeNodeId` is a
  // presence flag here (`1` = remap present) — the native form has no graph node
  // id, and the recovery only tests it for `!== 0`.
  return {
    parameterName: expr.parameter,
    source: expr.source,
    value: expr.value,
    optionalModifier: mappedModifierToCode(expr.optional),
    readonlyModifier: mappedModifierToCode(expr.readonly),
    nameTypeNodeId: expr.nameType == null ? 0 : 1,
  };
}

/**
 * Map the native (raw-JSON) mapped modifier string to the graph numeric
 * encoding the modifier consumers share (`applyMappedOptionalModifier` and the
 * `resolveOpenKeyDomainMappedDescriptor` bails): `1` = `MappedModifier::None`,
 * `2` = `Add` (`+?` / `+readonly`), `3` = `Remove` (`-?` / `-readonly`). An
 * absent or `"none"` modifier is the no-op encoding (`1`).
 */
function mappedModifierToCode(modifier: MappedModifierName | undefined): number {
  if (modifier === "add") {
    return 2;
  }
  if (modifier === "remove") {
    return 3;
  }
  return 1;
}

function readIndexedAccessInfo(
  expr: NativeTypeExprLike,
): { object: NativeTypeExprLike; index: NativeTypeExprLike } | undefined {
  if (isGraphTypeExprRef(expr)) {
    const node = expr.graph.getNode(expr.nodeId);
    if (node.kind !== NODE_INDEXED_ACCESS) {
      return undefined;
    }
    return {
      object: createGraphTypeExprRef(expr.graph, node.objectNodeId),
      index: createGraphTypeExprRef(expr.graph, node.indexNodeId),
    };
  }
  if (expr.kind !== "indexedAccess") {
    return undefined;
  }
  return { object: expr.object, index: expr.index };
}

function mappedIndexMatchesParameter(expr: NativeTypeExprLike, parameterName: string): boolean {
  if (isGraphTypeExprRef(expr)) {
    const node = expr.graph.getNode(expr.nodeId);
    return node.kind === NODE_TYPE_PARAMETER && expr.graph.getString(node.nameId) === parameterName;
  }
  return expr.kind === "typeParameter" && expr.name === parameterName;
}

function simplifyIntersectionDescriptor(types: TypeDescriptor[]): TypeDescriptor {
  const flattened = types.flatMap((type) => (type.kind === "intersection" ? type.types : [type]));
  const filtered = flattened.filter((type) => !isEmptyObjectDescriptor(type));
  if (filtered.length === 0) {
    return object([]);
  }
  if (filtered.every((type) => type.kind === "object")) {
    return mergeObjectDescriptors(filtered as Array<Extract<TypeDescriptor, { kind: "object" }>>);
  }
  return intersection(filtered);
}

function isEmptyObjectDescriptor(type: TypeDescriptor): boolean {
  return (
    type.kind === "object" &&
    type.properties.length === 0 &&
    !type.indexSignatures?.length &&
    !type.callSignatures?.length &&
    !type.constructSignatures?.length
  );
}

function mergeObjectDescriptors(
  types: Array<Extract<TypeDescriptor, { kind: "object" }>>,
): TypeDescriptor {
  const properties = new Map<string, { type: TypeDescriptor; optional: boolean }>();
  const indexSignatures: NonNullable<
    Extract<TypeDescriptor, { kind: "object" }>["indexSignatures"]
  > = [];
  const callSignatures: NonNullable<Extract<TypeDescriptor, { kind: "object" }>["callSignatures"]> =
    [];
  const constructSignatures: NonNullable<
    Extract<TypeDescriptor, { kind: "object" }>["constructSignatures"]
  > = [];

  for (const type of types) {
    for (const property of type.properties) {
      const existing = properties.get(property.name);
      if (existing) {
        existing.type = simplifyIntersectionDescriptor([existing.type, property.type]);
        existing.optional = existing.optional && property.optional;
      } else {
        properties.set(property.name, {
          type: property.type,
          optional: property.optional,
        });
      }
    }
    if (type.indexSignatures) {
      indexSignatures.push(...type.indexSignatures);
    }
    if (type.callSignatures) {
      callSignatures.push(...type.callSignatures);
    }
    if (type.constructSignatures) {
      constructSignatures.push(...type.constructSignatures);
    }
  }

  return object(
    [...properties.entries()].map(([name, property]) => ({
      name,
      type: property.type,
      optional: property.optional,
    })),
    {
      ...(indexSignatures.length > 0 ? { indexSignatures } : {}),
      ...(callSignatures.length > 0 ? { callSignatures } : {}),
      ...(constructSignatures.length > 0 ? { constructSignatures } : {}),
    },
  );
}

function resolveUnionObjectProperty(
  exprs: readonly NativeTypeExprLike[],
  propertyName: string,
  nativeRegistry: NativeTypeRegistry,
  visiting: Set<string>,
  graphVisiting: Set<number>,
): { ty: TypeDescriptor; optional: boolean } | undefined {
  const members = exprs.map((expr) =>
    resolveObjectProperty(expr, propertyName, nativeRegistry, visiting, graphVisiting),
  );
  if (members.some((member) => !member)) {
    return undefined;
  }

  const resolved = members as Array<{ ty: TypeDescriptor; optional: boolean }>;
  return {
    ty: union(resolved.map((member) => member.ty)),
    optional: resolved.some((member) => member.optional),
  };
}

function resolveIntersectionObjectProperty(
  exprs: readonly NativeTypeExprLike[],
  propertyName: string,
  nativeRegistry: NativeTypeRegistry,
  visiting: Set<string>,
  graphVisiting: Set<number>,
): { ty: TypeDescriptor; optional: boolean } | undefined {
  const members = exprs
    .map((expr) =>
      resolveObjectProperty(expr, propertyName, nativeRegistry, visiting, graphVisiting),
    )
    .filter((member): member is { ty: TypeDescriptor; optional: boolean } => member !== undefined);
  if (members.length === 0) {
    return undefined;
  }

  return {
    ty: intersection(members.map((member) => member.ty)),
    optional: members.every((member) => member.optional),
  };
}

function graphTypeExprToDescriptor(
  expr: GraphTypeExprRef,
  nativeRegistry?: NativeTypeRegistry,
  visiting: Set<string> = new Set(),
  graphVisiting: Set<number> = new Set(),
): TypeDescriptor {
  const memo = getGraphDescriptorMemo(expr.graph, nativeRegistry);
  const cached = memo.get(expr.nodeId);
  if (cached) {
    return cached;
  }
  if (graphVisiting.has(expr.nodeId)) {
    return unknown(graphTypeExprToString(expr));
  }

  graphVisiting.add(expr.nodeId);
  try {
    const descriptor = graphNodeToDescriptor(
      expr.graph.getNode(expr.nodeId),
      expr,
      nativeRegistry,
      visiting,
      graphVisiting,
    );
    memo.set(expr.nodeId, descriptor);
    return descriptor;
  } finally {
    graphVisiting.delete(expr.nodeId);
  }
}

function getGraphDescriptorMemo(
  graph: DecodedTypeGraph,
  nativeRegistry?: NativeTypeRegistry,
): Map<number, TypeDescriptor> {
  if (!nativeRegistry) {
    return graph.descriptorMemo;
  }

  let memo = graph.descriptorMemoByContext.get(nativeRegistry);
  if (!memo) {
    memo = new Map<number, TypeDescriptor>();
    graph.descriptorMemoByContext.set(nativeRegistry, memo);
  }
  return memo;
}

function graphNodeToDescriptor(
  node: GraphNodeRecord,
  expr: GraphTypeExprRef,
  nativeRegistry?: NativeTypeRegistry,
  visiting: Set<string> = new Set(),
  graphVisiting: Set<number> = new Set(),
): TypeDescriptor {
  switch (node.kind) {
    case NODE_PRIMITIVE:
      return primitive(graphPrimitiveName(node.primitive));
    case NODE_LITERAL:
      switch (node.literalKind) {
        case LITERAL_STRING:
          return literal(expr.graph.getString(node.stringId!));
        case LITERAL_NUMBER:
          return literal(node.numberValue!);
        case LITERAL_BOOLEAN:
          return literal(Boolean(node.booleanValue));
        case LITERAL_BIG_INT:
          return literal(expr.graph.getString(node.stringId!));
        default:
          throw new Error(
            `component-meta graph payload has unknown literal kind ${node.literalKind}`,
          );
      }
    case NODE_UNION:
      return union(
        node.typeNodeIds.map((id) =>
          typeExprToDescriptor(
            createGraphTypeExprRef(expr.graph, id),
            nativeRegistry,
            visiting,
            graphVisiting,
          ),
        ),
      );
    case NODE_INTERSECTION:
      return simplifyIntersectionDescriptor(
        node.typeNodeIds.map((id) =>
          typeExprToDescriptor(
            createGraphTypeExprRef(expr.graph, id),
            nativeRegistry,
            visiting,
            graphVisiting,
          ),
        ),
      );
    case NODE_ARRAY:
      return array(
        typeExprToDescriptor(
          createGraphTypeExprRef(expr.graph, node.elementNodeId),
          nativeRegistry,
          visiting,
          graphVisiting,
        ),
      );
    case NODE_TUPLE: {
      const els = node.elements.map((element) =>
        typeExprToDescriptor(
          createGraphTypeExprRef(expr.graph, element.typeNodeId),
          nativeRegistry,
          visiting,
          graphVisiting,
        ),
      );
      const labels = node.elements.map((element) =>
        element.labelId ? expr.graph.getString(element.labelId) : null,
      );
      const t = tuple(els);
      if (labels.some((l) => l !== null)) {
        return { ...t, labels };
      }
      return t;
    }
    case NODE_OBJECT: {
      const props = node.members
        .filter((member) => member.kind === MEMBER_PROPERTY || member.kind === MEMBER_METHOD)
        .map((member) => ({
          name: member.nameId ? expr.graph.getString(member.nameId) : "",
          type:
            member.kind === MEMBER_METHOD && member.functionNodeId
              ? typeExprToDescriptor(
                  createGraphTypeExprRef(expr.graph, member.functionNodeId),
                  nativeRegistry,
                  visiting,
                  graphVisiting,
                )
              : member.typeNodeId
                ? typeExprToDescriptor(
                    createGraphTypeExprRef(expr.graph, member.typeNodeId),
                    nativeRegistry,
                    visiting,
                    graphVisiting,
                  )
                : primitive("any"),
          optional: member.optional,
        }));
      const indexSignatures = node.members
        .filter((member) => member.kind === MEMBER_INDEX_SIGNATURE)
        .map((member) => ({
          keyName: member.keyNameId ? expr.graph.getString(member.keyNameId) : "key",
          keyType: member.keyTypeNodeId
            ? typeExprToDescriptor(
                createGraphTypeExprRef(expr.graph, member.keyTypeNodeId),
                nativeRegistry,
                visiting,
                graphVisiting,
              )
            : primitive("string"),
          valueType: member.valueTypeNodeId
            ? typeExprToDescriptor(
                createGraphTypeExprRef(expr.graph, member.valueTypeNodeId),
                nativeRegistry,
                visiting,
                graphVisiting,
              )
            : primitive("any"),
          ...(member.readonly ? { readonly: true } : {}),
        }));
      const callSignatures = node.members
        .filter((member) => member.kind === MEMBER_CALL_SIGNATURE && member.functionNodeId)
        .map(
          (member) =>
            typeExprToDescriptor(
              createGraphTypeExprRef(expr.graph, member.functionNodeId),
              nativeRegistry,
              visiting,
              graphVisiting,
            ) as Extract<TypeDescriptor, { kind: "function" }>,
        );
      const constructSignatures = node.members
        .filter((member) => member.kind === MEMBER_CONSTRUCT_SIGNATURE && member.functionNodeId)
        .map(
          (member) =>
            typeExprToDescriptor(
              createGraphTypeExprRef(expr.graph, member.functionNodeId),
              nativeRegistry,
              visiting,
              graphVisiting,
            ) as Extract<TypeDescriptor, { kind: "function" }>,
        );
      return object(props, {
        ...(indexSignatures.length > 0 ? { indexSignatures } : {}),
        ...(callSignatures.length > 0 ? { callSignatures } : {}),
        ...(constructSignatures.length > 0 ? { constructSignatures } : {}),
      });
    }
    case NODE_FUNCTION: {
      const parameters = node.parameters.map((parameter) => ({
        name: parameter.nameId ? expr.graph.getString(parameter.nameId) : "",
        type: typeExprToDescriptor(
          createGraphTypeExprRef(expr.graph, parameter.typeNodeId),
          nativeRegistry,
          visiting,
          graphVisiting,
        ),
        optional: parameter.optional,
      }));
      const returnType = node.returnTypeNodeId
        ? typeExprToDescriptor(
            createGraphTypeExprRef(expr.graph, node.returnTypeNodeId),
            nativeRegistry,
            visiting,
            graphVisiting,
          )
        : primitive("void");
      const typeParameters = node.typeParameterNodeIds.map(
        (id) =>
          typeExprToDescriptor(
            createGraphTypeExprRef(expr.graph, id),
            nativeRegistry,
            visiting,
            graphVisiting,
          ) as Extract<TypeDescriptor, { kind: "typeParameter" }>,
      );
      return func(parameters, returnType, {
        ...(typeParameters.length > 0 ? { typeParameters } : {}),
      });
    }
    case NODE_REF: {
      const name = expr.graph.getString(node.nameId);
      if (node.typeArgumentNodeIds.length > 0) {
        const resolved = resolveRegistryRefDescriptor(
          name,
          node.typeArgumentNodeIds.map((id) => createGraphTypeExprRef(expr.graph, id)),
          nativeRegistry,
          visiting,
          graphVisiting,
        );
        if (resolved) {
          return resolved;
        }
      }
      const typeArguments = node.typeArgumentNodeIds.map((id) =>
        typeExprToDescriptor(
          createGraphTypeExprRef(expr.graph, id),
          nativeRegistry,
          visiting,
          graphVisiting,
        ),
      );
      return typeArguments.length > 0 ? typeRef(name, typeArguments) : typeRef(name);
    }
    case NODE_TYPE_PARAMETER:
      return typeParameter(expr.graph.getString(node.nameId), {
        ...(node.constraintNodeId
          ? {
              constraint: typeExprToDescriptor(
                createGraphTypeExprRef(expr.graph, node.constraintNodeId),
                nativeRegistry,
                visiting,
                graphVisiting,
              ),
            }
          : {}),
        ...(node.defaultNodeId
          ? {
              default: typeExprToDescriptor(
                createGraphTypeExprRef(expr.graph, node.defaultNodeId),
                nativeRegistry,
                visiting,
                graphVisiting,
              ),
            }
          : {}),
      });
    case NODE_KEY_OF: {
      const resolved = resolveKeyOfDescriptor(
        createGraphTypeExprRef(expr.graph, node.operandNodeId),
        nativeRegistry,
        visiting,
        graphVisiting,
      );
      return resolved ?? unknown(graphTypeExprToString(expr));
    }
    case NODE_TYPE_OF:
    case NODE_CONDITIONAL:
    case NODE_TEMPLATE_LITERAL:
    case NODE_INFER:
    case NODE_REST:
      return unknown(graphTypeExprToString(expr));
    case NODE_MAPPED: {
      const resolved = resolveMappedDescriptor(expr, nativeRegistry, visiting, graphVisiting);
      return resolved ?? unknown(graphTypeExprToString(expr));
    }
    case NODE_INDEXED_ACCESS: {
      const resolved = nativeRegistry
        ? resolveIndexedAccessDescriptor(
            createGraphTypeExprRef(expr.graph, node.objectNodeId),
            createGraphTypeExprRef(expr.graph, node.indexNodeId),
            nativeRegistry,
            visiting,
            graphVisiting,
          )
        : undefined;
      if (resolved) {
        return resolved;
      }
      return indexedAccess(
        typeExprToDescriptor(
          createGraphTypeExprRef(expr.graph, node.objectNodeId),
          nativeRegistry,
          visiting,
          graphVisiting,
        ),
        typeExprToDescriptor(
          createGraphTypeExprRef(expr.graph, node.indexNodeId),
          nativeRegistry,
          visiting,
          graphVisiting,
        ),
      );
    }
    case NODE_PARENTHESIZED:
      return typeExprToDescriptor(
        createGraphTypeExprRef(expr.graph, node.innerNodeId),
        nativeRegistry,
        visiting,
        graphVisiting,
      );
    case NODE_RECURSIVE_REF: {
      const name = expr.graph.getString(node.nameId);
      const typeArguments = node.typeArgumentNodeIds.map((id) =>
        typeExprToDescriptor(
          createGraphTypeExprRef(expr.graph, id),
          nativeRegistry,
          visiting,
          graphVisiting,
        ),
      );
      const conditionalContext = node.conditionalContext.map((frame) => ({
        branch: (frame.branch === 1 ? "true" : "false") as "true" | "false",
        decided: frame.decided,
        check: typeExprToDescriptor(
          createGraphTypeExprRef(expr.graph, frame.checkNodeId),
          nativeRegistry,
          visiting,
          graphVisiting,
        ),
        extends: typeExprToDescriptor(
          createGraphTypeExprRef(expr.graph, frame.extendsNodeId),
          nativeRegistry,
          visiting,
          graphVisiting,
        ),
      }));
      return recursiveRef(name, typeArguments, conditionalContext);
    }
    case NODE_UNKNOWN:
      return unknown(expr.graph.getString(node.rawId));
    case NODE_SYNTHETIC_SLOT_BINDING: {
      const surfaceKind: "slotBinding" | "binding" =
        node.surfaceKind === SYNTHETIC_CARRIER_SURFACE_BINDING ? "binding" : "slotBinding";
      const slotName = node.slotNameId ? expr.graph.getString(node.slotNameId) : undefined;
      return syntheticSlotBinding(
        expr.graph.getString(node.scopeCanonicalId),
        surfaceKind,
        expr.graph.getString(node.bindingNameId),
        node.valueNode,
        slotName,
      );
    }
    default:
      return unknown("unrecognized");
  }
}

function graphPrimitiveName(tag: number): Parameters<typeof primitive>[0] {
  switch (tag) {
    case 1:
      return "string";
    case 2:
      return "number";
    case 3:
      return "boolean";
    case 4:
      return "symbol";
    case 5:
      return "bigint";
    case 6:
      return "any";
    case 7:
      return "unknown";
    case 8:
      return "void";
    case 9:
      return "never";
    case 10:
      return "null";
    case 11:
      return "undefined";
    case 12:
      return "object";
    default:
      throw new Error(`component-meta graph payload has unknown primitive tag ${tag}`);
  }
}

function graphTypeExprToString(expr: GraphTypeExprRef): string {
  const node = expr.graph.getNode(expr.nodeId);
  // Every graph node kind renders to a
  // structural string form. The legacy `default: graphNode(N)`
  // fallback leaked operator-node kinds into compat-layer strings as
  // diagnostic placeholders (graphNode_leak bucket).
  // Now every kind has explicit structural rendering; the exhaustive
  // switch below replaces the fallback so future graph-node kinds
  // surface as TypeScript compile errors at this site rather than
  // silently leaking through.
  switch (node.kind) {
    case NODE_PRIMITIVE:
      return graphPrimitiveName(node.primitive);
    case NODE_LITERAL:
      if (node.literalKind === LITERAL_STRING || node.literalKind === LITERAL_BIG_INT) {
        return expr.graph.getString(node.stringId!);
      }
      if (node.literalKind === LITERAL_NUMBER) {
        return String(node.numberValue);
      }
      if (node.literalKind === LITERAL_BOOLEAN) {
        return String(node.booleanValue);
      }
      return "literal";
    case NODE_REF: {
      const name = expr.graph.getString(node.nameId);
      if (node.typeArgumentNodeIds.length === 0) {
        return name;
      }
      const args = node.typeArgumentNodeIds
        .map((id) => graphTypeExprToString(createGraphTypeExprRef(expr.graph, id)))
        .join(", ");
      return `${name}<${args}>`;
    }
    case NODE_TYPE_PARAMETER:
      return expr.graph.getString(node.nameId);
    case NODE_UNION:
      return node.typeNodeIds
        .map((id) => graphTypeExprToString(createGraphTypeExprRef(expr.graph, id)))
        .join(" | ");
    case NODE_INTERSECTION:
      return node.typeNodeIds
        .map((id) => graphTypeExprToString(createGraphTypeExprRef(expr.graph, id)))
        .join(" & ");
    case NODE_ARRAY: {
      const element = graphTypeExprToString(createGraphTypeExprRef(expr.graph, node.elementNodeId));
      return node.readonly ? `readonly ${element}[]` : `${element}[]`;
    }
    case NODE_TUPLE: {
      const items = node.elements
        .map((e) => graphTypeExprToString(createGraphTypeExprRef(expr.graph, e.typeNodeId)))
        .join(", ");
      return node.readonly ? `readonly [${items}]` : `[${items}]`;
    }
    case NODE_OBJECT:
      return "object";
    case NODE_FUNCTION:
      return "function";
    case NODE_KEY_OF:
      return `keyof ${graphTypeExprToString(createGraphTypeExprRef(expr.graph, node.operandNodeId))}`;
    case NODE_TYPE_OF:
      return `typeof ${node.pathIds.map((id) => expr.graph.getString(id)).join(".")}`;
    case NODE_INDEXED_ACCESS: {
      const obj = graphTypeExprToString(createGraphTypeExprRef(expr.graph, node.objectNodeId));
      const idx = graphTypeExprToString(createGraphTypeExprRef(expr.graph, node.indexNodeId));
      return `${obj}[${idx}]`;
    }
    case NODE_CONDITIONAL: {
      const check = graphTypeExprToString(createGraphTypeExprRef(expr.graph, node.checkNodeId));
      const ext = graphTypeExprToString(createGraphTypeExprRef(expr.graph, node.extendsNodeId));
      const tt = graphTypeExprToString(createGraphTypeExprRef(expr.graph, node.trueTypeNodeId));
      const ft = graphTypeExprToString(createGraphTypeExprRef(expr.graph, node.falseTypeNodeId));
      return `${check} extends ${ext} ? ${tt} : ${ft}`;
    }
    case NODE_MAPPED:
      return "mapped";
    case NODE_TEMPLATE_LITERAL: {
      const quasis = node.quasiIds.map((id) => expr.graph.getString(id));
      const exprs = node.expressionNodeIds.map((id) =>
        graphTypeExprToString(createGraphTypeExprRef(expr.graph, id)),
      );
      let out = "`";
      for (let i = 0; i < quasis.length; i += 1) {
        out += quasis[i];
        if (i < exprs.length) {
          out += "${" + exprs[i] + "}";
        }
      }
      out += "`";
      return out;
    }
    case NODE_PARENTHESIZED:
      return `(${graphTypeExprToString(createGraphTypeExprRef(expr.graph, node.innerNodeId))})`;
    case NODE_INFER:
      return `infer ${expr.graph.getString(node.nameId)}`;
    case NODE_REST:
      return `...${graphTypeExprToString(createGraphTypeExprRef(expr.graph, node.innerNodeId))}`;
    case NODE_RECURSIVE_REF:
      return expr.graph.getString(node.nameId);
    case NODE_UNKNOWN:
      return expr.graph.getString(node.rawId);
    case NODE_SYNTHETIC_SLOT_BINDING:
      // Synthetic carriers display as their `bindingName` (the user-visible
      // identity) — they MUST NOT resolve through `TypeRegistry`.
      return expr.graph.getString(node.bindingNameId);
    default: {
      const _exhaustive: never = node;
      throw new Error(
        `graphTypeExprToString: unhandled graph node kind ${(_exhaustive as { kind: number }).kind}`,
      );
    }
  }
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
    case "recursiveRef":
      return expr.typeArguments.length > 0
        ? `${expr.name}<${expr.typeArguments.map(nativeTypeExprToString).join(", ")}>`
        : expr.name;
    case "keyOf":
      return `keyof ${nativeTypeExprToString(expr.operand)}`;
    case "typeOf":
      return `typeof ${expr.path.join(".")}`;
    case "unknown":
      return expr.raw;
    case "syntheticSlotBinding":
      // Synthetic carriers render as their user-visible `bindingName` — they
      // MUST NOT route through `TypeRegistry` for display.
      return expr.bindingName;
    default:
      return expr.kind;
  }
}
