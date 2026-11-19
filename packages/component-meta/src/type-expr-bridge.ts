/**
 * Bridge between native TypeExpr (from Rust evaluator) and TypeDescriptor (JS type IR).
 *
 * Converts the evaluated type expressions returned by the native lightweight
 * evaluator into the public TypeDescriptor format used by adapters and consumers.
 */

import type { TupleType, TypeDescriptor } from "@verter/type-ir";
import {
  primitive,
  literal,
  array,
  tuple,
  object,
  func,
  typeParameter,
  ref as typeRef,
  intrinsicApplication,
  intrinsicDisplayName,
  recursiveRef,
  indexedAccess,
  unknown,
  syntheticSlotBinding,
} from "@verter/type-ir";
import {
  createGraphTypeExprRef,
  DecodedTypeGraph,
  graphPropertyKeyName,
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
  MEMBER_SPREAD,
  NODE_ARRAY,
  NODE_CONDITIONAL,
  NODE_FUNCTION,
  NODE_INDEXED_ACCESS,
  NODE_INFER,
  NODE_INTERSECTION,
  NODE_INTRINSIC_APPLICATION,
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
  intrinsicOpDisplayName,
  type GraphNodeRecord,
  type GraphObjectMemberRecord,
  type GraphPropertyKeyRecord,
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
      predicate?: NativeTypePredicate;
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
      abstract?: boolean;
    }
  | { kind: "ref"; name: string; typeArguments: NativeTypeExpr[] }
  // A compiler-native type OPERATION whose identity the checker has already
  // resolved (`Awaited<T>`). Never a `ref`: it names no declaration, so it
  // must not be resolved through any registry.
  | { kind: "intrinsicApplication"; op: string; arguments: NativeTypeExpr[] }
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
  | { kind: "typeOf"; path: string[]; typeArgs?: NativeTypeExpr[] }
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
      // optional on the wire; absence means no modifier or remap.
      optional?: MappedModifierName;
      readonly?: MappedModifierName;
      nameType?: NativeTypeExpr | null;
    }
  | { kind: "templateLiteral"; quasis: string[]; expressions: NativeTypeExpr[] }
  | { kind: "parenthesized"; inner: NativeTypeExpr }
  | { kind: "unknown"; raw: string }
  | { kind: "infer"; name: string; constraint?: NativeTypeExprLike }
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

interface NativeTupleElement {
  label?: string | null;
  ty: NativeTypeExpr;
  optional: boolean;
  rest: boolean;
}

interface NativeObjectMember {
  memberKind:
    | "property"
    | "indexSignature"
    | "callSignature"
    | "constructSignature"
    | "method"
    // Raised open surfaces reconstruct their ordered concrete writes and
    // residual operands as spread members. This bridge treats a spread as an
    // operand contribution rather than a named member; exact key and absence
    // decisions remain in the Rust surface domain.
    | "spread";
  // Property fields. The Rust producer keys a property or method by its
  // typed `key`; `name` is the older spelling.
  key?: NativePropertyKey;
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
  predicate?: NativeTypePredicate;
}

interface NativeTypeParameter {
  name: string;
  constraint?: NativeTypeExpr | null;
  default?: NativeTypeExpr | null;
  isConst?: boolean;
}

/** A signature's type predicate (`x is T`, `asserts x`, `this is T`). */
interface NativeTypePredicate {
  asserts: boolean;
  parameter?: string;
  this?: boolean;
  ty?: NativeTypeExpr;
}

/** A member's typed key, as the Rust producer serializes it. */
type NativePropertyKey =
  | { kind: "string"; value: string }
  | { kind: "number"; value: number }
  | { kind: "uniqueSymbol"; identity: { symbol: string } }
  | { kind: "computed"; expression: NativeTypeExpr };

/**
 * A member's property name as a string: its typed string or number key, or
 * the older `name` spelling. A symbol or computed key names no string
 * property.
 */
function nativeMemberStringName(member: NativeObjectMember): string | undefined {
  const key = member.key;
  if (!key) {
    return member.name;
  }
  switch (key.kind) {
    case "string":
      return key.value;
    case "number":
      return String(key.value);
    case "uniqueSymbol":
    case "computed":
      return undefined;
  }
}

// ── Converter ────────────────────────────────────────────────────

/**
 * Convert a native TypeExpr to a TypeDescriptor.
 *
 * This bridges the gap between the Rust evaluator output and the
 * JS type IR consumed by adapters (Storybook, Zod, JSON Schema, etc.).
 */
export function typeExprToDescriptor(expr: NativeTypeExprLike): TypeDescriptor {
  return transcribeTypeExpr(expr, new Set());
}

function transcribeTypeExpr(expr: NativeTypeExprLike, graphVisiting: Set<number>): TypeDescriptor {
  if (isGraphTypeExprRef(expr)) {
    return graphTypeExprToDescriptor(expr, graphVisiting);
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
      return {
        kind: "union",
        types: expr.types.map((type) => transcribeTypeExpr(type, graphVisiting)),
      };

    case "intersection":
      return {
        kind: "intersection",
        types: expr.types.map((type) => transcribeTypeExpr(type, graphVisiting)),
      };

    case "array":
      return array(transcribeTypeExpr(expr.element, graphVisiting));

    case "tuple": {
      const elements = expr.elements.map((e) => transcribeTypeExpr(e.ty, graphVisiting));
      return tuple(
        elements,
        expr.elements.map((e) => ({
          ...(e.label != null ? { label: e.label } : {}),
          ...(e.optional ? { optional: true } : {}),
          ...(e.rest ? { rest: true } : {}),
        })),
        expr.readonly,
      );
    }

    case "object": {
      const props = expr.properties
        .filter((m) => m.memberKind === "property" || m.memberKind === "method")
        .map((m) => ({
          name: nativeMemberStringName(m) ?? "",
          type:
            m.memberKind === "method" && m.function
              ? nativeFunctionToDescriptor(m.function, graphVisiting)
              : m.ty
                ? transcribeTypeExpr(m.ty, graphVisiting)
                : primitive("any"),
          optional: m.optional ?? false,
        }));
      const indexSignatures = expr.properties
        .filter((m) => m.memberKind === "indexSignature")
        .map((m) => ({
          keyName: m.keyName ?? "key",
          keyType: m.keyType ? transcribeTypeExpr(m.keyType, graphVisiting) : primitive("string"),
          valueType: m.valueType
            ? transcribeTypeExpr(m.valueType, graphVisiting)
            : primitive("any"),
          ...(m.readonly ? { readonly: true } : {}),
        }));
      const callSignatures = expr.properties
        .filter((m) => m.memberKind === "callSignature" && m.function)
        .map((m) => nativeFunctionToDescriptor(m.function!, graphVisiting));
      const constructSignatures = expr.properties
        .filter((m) => m.memberKind === "constructSignature" && m.function)
        .map((m) => nativeFunctionToDescriptor(m.function!, graphVisiting));
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
        type: transcribeTypeExpr(p.ty, graphVisiting),
        optional: p.optional,
      }));
      const returnType = expr.returnType
        ? transcribeTypeExpr(expr.returnType, graphVisiting)
        : primitive("void");
      return func(params, returnType, {
        typeParameters: expr.typeParameters?.map((typeParam) =>
          nativeTypeParameterToDescriptor(typeParam, graphVisiting),
        ),
      });
    }

    case "ref":
      if (expr.typeArguments.length > 0) {
        return typeRef(
          expr.name,
          expr.typeArguments.map((typeArgument) => transcribeTypeExpr(typeArgument, graphVisiting)),
        );
      }
      return typeRef(expr.name);

    // Compiler intrinsics retain their operation identity and transcribed operands.
    case "intrinsicApplication":
      return intrinsicApplication(
        expr.op,
        expr.arguments.map((argument) => transcribeTypeExpr(argument, graphVisiting)),
      );

    case "recursiveRef":
      return recursiveRef(
        expr.name,
        expr.typeArguments.map((typeArgument) => transcribeTypeExpr(typeArgument, graphVisiting)),
        expr.conditionalContext.map((frame) => ({
          branch: frame.branch,
          decided: frame.decided,
          check: transcribeTypeExpr(frame.check, graphVisiting),
          extends: transcribeTypeExpr(frame.extends, graphVisiting),
        })),
      );

    case "typeParameter":
      return nativeTypeParameterToDescriptor(expr, graphVisiting);

    case "keyOf":
    case "mapped":
    case "typeOf":
    case "conditional":
    case "templateLiteral":
    case "infer":
    case "rest":
      // These operator forms should be evaluated by the native evaluator.
      // If they reach here, they couldn't be reduced — fall back to unknown.
      return unknown(nativeTypeExprToString(expr));

    case "indexedAccess": {
      return indexedAccess(
        transcribeTypeExpr(expr.object, graphVisiting),
        transcribeTypeExpr(expr.index, graphVisiting),
      );
    }

    case "parenthesized":
      return transcribeTypeExpr(expr.inner, graphVisiting);

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
  graphVisiting: Set<number> = new Set(),
) {
  return func(
    (expr.parameters ?? []).map((p) => ({
      name: p.name ?? "",
      type: transcribeTypeExpr(p.ty, graphVisiting),
      optional: p.optional,
    })),
    expr.returnType ? transcribeTypeExpr(expr.returnType, graphVisiting) : primitive("void"),
    {
      typeParameters: expr.typeParameters?.map((typeParam) =>
        nativeTypeParameterToDescriptor(typeParam, graphVisiting),
      ),
    },
  );
}

function nativeTypeParameterToDescriptor(
  expr: NativeTypeParameter,
  graphVisiting: Set<number> = new Set(),
) {
  return typeParameter(expr.name, {
    ...(expr.constraint
      ? {
          constraint: transcribeTypeExpr(expr.constraint, graphVisiting),
        }
      : {}),
    ...(expr.default ? { default: transcribeTypeExpr(expr.default, graphVisiting) } : {}),
  });
}

function graphTypeExprToDescriptor(
  expr: GraphTypeExprRef,
  graphVisiting: Set<number> = new Set(),
): TypeDescriptor {
  const memo = expr.graph.descriptorMemo;
  const cached = memo.get(expr.nodeId);
  if (cached) {
    return cached;
  }
  if (graphVisiting.has(expr.nodeId)) {
    return unknown(graphTypeExprToString(expr));
  }

  graphVisiting.add(expr.nodeId);
  try {
    const descriptor = graphNodeToDescriptor(expr.graph.getNode(expr.nodeId), expr, graphVisiting);
    memo.set(expr.nodeId, descriptor);
    return descriptor;
  } finally {
    graphVisiting.delete(expr.nodeId);
  }
}

function graphNodeToDescriptor(
  node: GraphNodeRecord,
  expr: GraphTypeExprRef,
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
      return {
        kind: "union",
        types: node.typeNodeIds.map((id) =>
          transcribeTypeExpr(createGraphTypeExprRef(expr.graph, id), graphVisiting),
        ),
      };
    case NODE_INTERSECTION:
      return {
        kind: "intersection",
        types: node.typeNodeIds.map((id) =>
          transcribeTypeExpr(createGraphTypeExprRef(expr.graph, id), graphVisiting),
        ),
      };
    case NODE_ARRAY:
      return array(
        transcribeTypeExpr(createGraphTypeExprRef(expr.graph, node.elementNodeId), graphVisiting),
      );
    case NODE_TUPLE: {
      const els = node.elements.map((element) =>
        transcribeTypeExpr(createGraphTypeExprRef(expr.graph, element.typeNodeId), graphVisiting),
      );
      return tuple(
        els,
        node.elements.map((element) => ({
          ...(element.labelId ? { label: expr.graph.getString(element.labelId) } : {}),
          ...(element.optional ? { optional: true } : {}),
          ...(element.rest ? { rest: true } : {}),
        })),
        node.readonly,
      );
    }
    case NODE_OBJECT: {
      const props = node.members
        .filter((member) => member.kind === MEMBER_PROPERTY || member.kind === MEMBER_METHOD)
        .map((member) => ({
          name: graphPropertyKeyName(expr.graph.getString.bind(expr.graph), member.key) ?? "",
          type:
            member.kind === MEMBER_METHOD && member.functionNodeId
              ? transcribeTypeExpr(
                  createGraphTypeExprRef(expr.graph, member.functionNodeId),
                  graphVisiting,
                )
              : member.typeNodeId
                ? transcribeTypeExpr(
                    createGraphTypeExprRef(expr.graph, member.typeNodeId),
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
            ? transcribeTypeExpr(
                createGraphTypeExprRef(expr.graph, member.keyTypeNodeId),
                graphVisiting,
              )
            : primitive("string"),
          valueType: member.valueTypeNodeId
            ? transcribeTypeExpr(
                createGraphTypeExprRef(expr.graph, member.valueTypeNodeId),
                graphVisiting,
              )
            : primitive("any"),
          ...(member.readonly ? { readonly: true } : {}),
        }));
      const callSignatures = node.members
        .filter((member) => member.kind === MEMBER_CALL_SIGNATURE && member.functionNodeId)
        .map(
          (member) =>
            transcribeTypeExpr(
              createGraphTypeExprRef(expr.graph, member.functionNodeId),
              graphVisiting,
            ) as Extract<TypeDescriptor, { kind: "function" }>,
        );
      const constructSignatures = node.members
        .filter((member) => member.kind === MEMBER_CONSTRUCT_SIGNATURE && member.functionNodeId)
        .map(
          (member) =>
            transcribeTypeExpr(
              createGraphTypeExprRef(expr.graph, member.functionNodeId),
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
        type: transcribeTypeExpr(
          createGraphTypeExprRef(expr.graph, parameter.typeNodeId),
          graphVisiting,
        ),
        optional: parameter.optional,
      }));
      const returnType = node.returnTypeNodeId
        ? transcribeTypeExpr(
            createGraphTypeExprRef(expr.graph, node.returnTypeNodeId),
            graphVisiting,
          )
        : primitive("void");
      const typeParameters = node.typeParameterNodeIds.map(
        (id) =>
          transcribeTypeExpr(createGraphTypeExprRef(expr.graph, id), graphVisiting) as Extract<
            TypeDescriptor,
            { kind: "typeParameter" }
          >,
      );
      return func(parameters, returnType, {
        ...(typeParameters.length > 0 ? { typeParameters } : {}),
      });
    }
    case NODE_REF: {
      const name = expr.graph.getString(node.nameId);
      const typeArguments = node.typeArgumentNodeIds.map((id) =>
        transcribeTypeExpr(createGraphTypeExprRef(expr.graph, id), graphVisiting),
      );
      return typeArguments.length > 0 ? typeRef(name, typeArguments) : typeRef(name);
    }
    case NODE_TYPE_PARAMETER:
      return typeParameter(expr.graph.getString(node.nameId), {
        ...(node.constraintNodeId
          ? {
              constraint: transcribeTypeExpr(
                createGraphTypeExprRef(expr.graph, node.constraintNodeId),
                graphVisiting,
              ),
            }
          : {}),
        ...(node.defaultNodeId
          ? {
              default: transcribeTypeExpr(
                createGraphTypeExprRef(expr.graph, node.defaultNodeId),
                graphVisiting,
              ),
            }
          : {}),
      });
    case NODE_KEY_OF:
    case NODE_MAPPED:
    case NODE_TYPE_OF:
    case NODE_CONDITIONAL:
    case NODE_TEMPLATE_LITERAL:
    case NODE_INFER:
    case NODE_REST:
      return unknown(graphTypeExprToString(expr));
    case NODE_INDEXED_ACCESS: {
      return indexedAccess(
        transcribeTypeExpr(createGraphTypeExprRef(expr.graph, node.objectNodeId), graphVisiting),
        transcribeTypeExpr(createGraphTypeExprRef(expr.graph, node.indexNodeId), graphVisiting),
      );
    }
    case NODE_PARENTHESIZED:
      return transcribeTypeExpr(
        createGraphTypeExprRef(expr.graph, node.innerNodeId),
        graphVisiting,
      );
    case NODE_RECURSIVE_REF: {
      const name = expr.graph.getString(node.nameId);
      const typeArguments = node.typeArgumentNodeIds.map((id) =>
        transcribeTypeExpr(createGraphTypeExprRef(expr.graph, id), graphVisiting),
      );
      const conditionalContext = node.conditionalContext.map((frame) => ({
        branch: (frame.branch === 1 ? "true" : "false") as "true" | "false",
        decided: frame.decided,
        check: transcribeTypeExpr(
          createGraphTypeExprRef(expr.graph, frame.checkNodeId),
          graphVisiting,
        ),
        extends: transcribeTypeExpr(
          createGraphTypeExprRef(expr.graph, frame.extendsNodeId),
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

/** Map native mapped modifiers to the numeric form shared by the display printers. */
function mappedModifierToCode(modifier: MappedModifierName | undefined): number {
  if (modifier === "add") return 2;
  if (modifier === "remove") return 3;
  return 1;
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

/**
 * A template literal type's quasi is its cooked text; printed back into
 * template source it is escaped as the checker prints the type (a
 * backslash, a backtick and `${` escaped, every control character but the
 * line feed and U+2028 / U+2029 / U+0085 as an escape).
 */
export function escapeTemplateQuasi(cooked: string): string {
  let out = "";
  for (let i = 0; i < cooked.length; i += 1) {
    const ch = cooked[i];
    const next = cooked[i + 1];
    switch (ch) {
      case "\\":
        out += "\\\\";
        break;
      case "`":
        out += "\\`";
        break;
      case "$":
        out += next === "{" ? "\\$" : "$";
        break;
      case "\n":
        out += "\n";
        break;
      case "\r":
        if (next === "\n") {
          out += "\\r\\n";
          i += 1;
        } else {
          out += "\\r";
        }
        break;
      case "\t":
        out += "\\t";
        break;
      case "\b":
        out += "\\b";
        break;
      case "\f":
        out += "\\f";
        break;
      case "\v":
        out += "\\v";
        break;
      case "\0":
        out += next !== undefined && next >= "0" && next <= "9" ? "\\x00" : "\\0";
        break;
      default: {
        const code = ch.charCodeAt(0);
        if (code < 0x20 || code === 0x2028 || code === 0x2029 || code === 0x85) {
          out += "\\u" + code.toString(16).toUpperCase().padStart(4, "0");
        } else {
          out += ch;
        }
      }
    }
  }
  return out;
}

// ── Display text ─────────────────────────────────────────────────
//
// An unreduced operator form keeps its type as display text. The text is
// TypeScript type syntax that reparses to the printed tree: each operand
// binds at least as tightly as its position requires, or is parenthesized,
// and literals, keys and modifiers keep their spelling. The precedence
// model is the Rust display renderer's (`verter_type_expr::display`).

/** Type-operator binding strength, loosest first. */
const PREC_LOWEST = 0;
const PREC_CONDITIONAL_OR_FUNCTION = 1;
const PREC_UNION = 2;
const PREC_INTERSECTION = 3;
const PREC_PREFIX = 4;
const PREC_POSTFIX = 5;
const PREC_PRIMARY = 6;

/** A printed form and how tightly it binds. */
interface Printed {
  text: string;
  precedence: number;
}

function printed(text: string, precedence: number): Printed {
  return { text, precedence };
}

/** `form` in a position that requires at least `minimum` binding. */
function operand(form: Printed, minimum: number): string {
  return form.precedence < minimum ? `(${form.text})` : form.text;
}

/** `name` applied to already printed type arguments. */
function applied(name: string, args: string[]): Printed {
  return printed(args.length > 0 ? `${name}<${args.join(", ")}>` : name, PREC_PRIMARY);
}

/** A union or intersection; with no arm it is its identity type. */
function joined(arms: Printed[], separator: string, precedence: number, empty: string): Printed {
  if (arms.length === 0) {
    return printed(empty, PREC_PRIMARY);
  }
  return printed(arms.map((arm) => operand(arm, precedence)).join(separator), precedence);
}

function arrayForm(element: Printed, readonly: boolean): Printed {
  const text = `${operand(element, PREC_POSTFIX)}[]`;
  return readonly ? printed(`readonly ${text}`, PREC_PREFIX) : printed(text, PREC_POSTFIX);
}

interface PrintedTupleElement {
  label: string | undefined;
  type: Printed;
  optional: boolean;
  rest: boolean;
}

function tupleForm(elements: PrintedTupleElement[], readonly: boolean): Printed {
  const items = elements.map(({ label, type, optional, rest }) => {
    if (label) {
      const marker = optional && !rest ? "?" : "";
      return `${rest ? "..." : ""}${label}${marker}: ${operand(type, PREC_LOWEST)}`;
    }
    if (rest) {
      return `...${operand(type, PREC_LOWEST)}`;
    }
    // An unlabeled optional element is its operand and a postfix `?`:
    // `[A | B?]` would make `B?` the optional operand.
    return optional ? `${operand(type, PREC_POSTFIX)}?` : operand(type, PREC_LOWEST);
  });
  const text = `[${items.join(", ")}]`;
  return readonly ? printed(`readonly ${text}`, PREC_PREFIX) : printed(text, PREC_PRIMARY);
}

/** Print descriptor tuple syntax using the same modifier rules as the wire printer. */
export function tupleDescriptorToString(
  descriptor: TupleType,
  render: (element: TypeDescriptor, index: number) => string,
): string {
  return tupleForm(
    descriptor.elements.map((element, index) => ({
      label: descriptor.modifiers?.[index]?.label,
      optional: descriptor.modifiers?.[index]?.optional === true,
      rest: descriptor.modifiers?.[index]?.rest === true,
      type: printed(
        render(element, index),
        element.kind === "union"
          ? PREC_UNION
          : element.kind === "intersection"
            ? PREC_INTERSECTION
            : element.kind === "function"
              ? PREC_LOWEST
              : PREC_PRIMARY,
      ),
    })),
    descriptor.readonly === true,
  ).text;
}

function objectForm(members: string[]): Printed {
  return printed(members.length > 0 ? `{ ${members.join("; ")} }` : "{}", PREC_PRIMARY);
}

function conditionalForm(
  check: Printed,
  extendsType: Printed,
  trueType: Printed,
  falseType: Printed,
): Printed {
  return printed(
    `${operand(check, PREC_UNION)} extends ${operand(extendsType, PREC_UNION)} ? ${operand(trueType, PREC_LOWEST)} : ${operand(falseType, PREC_LOWEST)}`,
    PREC_CONDITIONAL_OR_FUNCTION,
  );
}

function inferForm(name: string, constraint: Printed | undefined): Printed {
  return constraint
    ? printed(
        `infer ${name} extends ${operand(constraint, PREC_UNION)}`,
        PREC_CONDITIONAL_OR_FUNCTION,
      )
    : printed(`infer ${name}`, PREC_PREFIX);
}

/**
 * Printing state. As in the Rust display renderer, an `infer` inside an
 * `extends` clause declares its binder (a reference to a binder is a type
 * reference), so a nested clause redeclaring an enclosing binder's name keeps
 * its declaration. `graphActive` holds the graph nodes being printed, so a
 * cyclic node table cannot recurse forever.
 */
interface PrintScope {
  inExtends: boolean;
  graphActive: Set<number>;
}

function newPrintScope(): PrintScope {
  return { inExtends: false, graphActive: new Set() };
}

/** A conditional: an `infer` in its `extends` clause declares a binder. */
function printConditional<N>(
  scope: PrintScope,
  print: (node: N) => Printed,
  check: N,
  extendsType: N,
  trueType: N,
  falseType: N,
): Printed {
  const enclosing = scope.inExtends;
  const checkText = print(check);
  scope.inExtends = true;
  const extendsText = print(extendsType);
  scope.inExtends = enclosing;
  const trueText = print(trueType);
  const falseText = print(falseType);
  return conditionalForm(checkText, extendsText, trueText, falseText);
}

/** An `infer`: a declaration inside an `extends` clause; outside one, its name. */
function printInfer(
  scope: PrintScope,
  name: string,
  constraint: () => Printed | undefined,
): Printed {
  if (!scope.inExtends) {
    return printed(name, PREC_PRIMARY);
  }
  return inferForm(name, constraint());
}

function mappedForm(
  parameter: string,
  source: Printed,
  nameType: Printed | undefined,
  value: Printed,
  optionalToken: string,
  readonlyPrefix: string,
): Printed {
  const remap = nameType ? ` as ${operand(nameType, PREC_LOWEST)}` : "";
  return printed(
    `{ ${readonlyPrefix}[${parameter} in ${operand(source, PREC_LOWEST)}${remap}]${optionalToken}: ${operand(value, PREC_LOWEST)} }`,
    PREC_PRIMARY,
  );
}

function templateForm(quasis: string[], expressions: Printed[]): Printed {
  let text = "`";
  for (let i = 0; i < quasis.length; i += 1) {
    text += escapeTemplateQuasi(quasis[i]!);
    if (i < expressions.length) {
      text += "${" + operand(expressions[i]!, PREC_LOWEST) + "}";
    }
  }
  return printed(`${text}\``, PREC_PRIMARY);
}

/** A parameter; an unnamed one takes a generated name, as `(_arg0: T)`. */
function parameterText(
  name: string | null | undefined,
  index: number,
  optional: boolean,
  rest: boolean,
  type: Printed,
): string {
  const marker = optional && !rest ? "?" : "";
  return `${rest ? "..." : ""}${name || `_arg${index}`}${marker}: ${operand(type, PREC_LOWEST)}`;
}

function typeParameterText(
  name: string,
  isConst: boolean,
  constraint: Printed | undefined,
  defaultType: Printed | undefined,
): string {
  let text = `${isConst ? "const " : ""}${name}`;
  if (constraint) {
    text += ` extends ${operand(constraint, PREC_LOWEST)}`;
  }
  if (defaultType) {
    text += ` = ${operand(defaultType, PREC_LOWEST)}`;
  }
  return text;
}

/** `<T>(a: A) => R` with `separator` " => ", or the member form with ": ". */
function signatureText(
  typeParameters: string[],
  parameters: string[],
  returnText: string,
  separator: " => " | ": ",
): string {
  const generics = typeParameters.length > 0 ? `<${typeParameters.join(", ")}>` : "";
  return `${generics}(${parameters.join(", ")})${separator}${returnText}`;
}

/**
 * A string-keyed member name: bare when it is an identifier, quoted otherwise.
 * A digit spelling stays quoted too: `{ "7": A }` and `{ 7: A }` address one
 * property but differ in type identity (`keyof` is `"7"` against `7`), so only
 * a number key prints bare.
 */
function memberName(name: string): string {
  if (/^[\p{ID_Start}$_][\p{ID_Continue}$‌‍]*$/u.test(name)) {
    return name;
  }
  return quoteStringLiteral(name);
}

/**
 * A method's printed key: a bare `new(): T` member is a construct
 * signature, so a method keyed `new` prints its key quoted.
 */
function methodKey(key: string): string {
  return key === "new" ? quoteStringLiteral(key) : key;
}

/**
 * A string literal type as the checker prints it: double-quoted, with a
 * backslash, a double quote, every control character and U+2028 / U+2029 /
 * U+0085 escaped.
 */
export function quoteStringLiteral(value: string): string {
  let out = '"';
  for (let i = 0; i < value.length; i += 1) {
    const ch = value[i]!;
    const next = value[i + 1];
    switch (ch) {
      case "\\":
        out += "\\\\";
        break;
      case '"':
        out += '\\"';
        break;
      case "\n":
        out += "\\n";
        break;
      case "\t":
        out += "\\t";
        break;
      case "\r":
        out += "\\r";
        break;
      case "\b":
        out += "\\b";
        break;
      case "\f":
        out += "\\f";
        break;
      case "\v":
        out += "\\v";
        break;
      case "\0":
        out += next !== undefined && next >= "0" && next <= "9" ? "\\x00" : "\\0";
        break;
      default: {
        const code = ch.charCodeAt(0);
        if (code < 0x20 || code === 0x2028 || code === 0x2029 || code === 0x85) {
          out += "\\u" + code.toString(16).toUpperCase().padStart(4, "0");
        } else {
          out += ch;
        }
      }
    }
  }
  return `${out}"`;
}

function mappedOptionalToken(code: number): string {
  return code === 2 ? "?" : code === 3 ? "-?" : "";
}

function mappedReadonlyPrefix(code: number): string {
  return code === 2 ? "readonly " : code === 3 ? "-readonly " : "";
}

function graphTypeExprToString(expr: GraphTypeExprRef): string {
  return printGraph(expr.graph, expr.nodeId, newPrintScope()).text;
}

function printGraph(graph: DecodedTypeGraph, nodeId: number, scope: PrintScope): Printed {
  // A cyclic node table has no finite spelling: its back-edge prints as
  // `unknown` rather than recursing.
  if (scope.graphActive.has(nodeId)) {
    return printed("unknown", PREC_PRIMARY);
  }
  scope.graphActive.add(nodeId);
  try {
    return printGraphNode(graph, nodeId, scope);
  } finally {
    scope.graphActive.delete(nodeId);
  }
}

function printGraphNode(graph: DecodedTypeGraph, nodeId: number, scope: PrintScope): Printed {
  const node = graph.getNode(nodeId);
  const at = (id: number) => printGraph(graph, id, scope);
  const atLowest = (id: number) => operand(at(id), PREC_LOWEST);
  // Every graph node kind has a structural spelling; the exhaustive switch
  // makes a new kind a compile error here instead of a leaked placeholder.
  switch (node.kind) {
    case NODE_PRIMITIVE:
      return printed(graphPrimitiveName(node.primitive), PREC_PRIMARY);
    case NODE_LITERAL:
      switch (node.literalKind) {
        case LITERAL_STRING:
          return printed(quoteStringLiteral(graph.getString(node.stringId!)), PREC_PRIMARY);
        case LITERAL_BIG_INT:
          return printed(`${graph.getString(node.stringId!)}n`, PREC_PRIMARY);
        case LITERAL_NUMBER:
          return printed(String(node.numberValue), PREC_PRIMARY);
        case LITERAL_BOOLEAN:
          return printed(String(Boolean(node.booleanValue)), PREC_PRIMARY);
        default:
          return printed("literal", PREC_PRIMARY);
      }
    case NODE_REF:
      return applied(graph.getString(node.nameId), node.typeArgumentNodeIds.map(atLowest));
    case NODE_RECURSIVE_REF:
      return applied(graph.getString(node.nameId), node.typeArgumentNodeIds.map(atLowest));
    case NODE_INTRINSIC_APPLICATION:
      // Renders like a named application but resolves NOTHING: the op is a
      // closed enum, so there is no string id and no registry lookup.
      return applied(intrinsicOpDisplayName(node.op), node.argumentNodeIds.map(atLowest));
    case NODE_TYPE_PARAMETER:
      return printed(graph.getString(node.nameId), PREC_PRIMARY);
    case NODE_UNION:
      return joined(node.typeNodeIds.map(at), " | ", PREC_UNION, "never");
    case NODE_INTERSECTION:
      return joined(node.typeNodeIds.map(at), " & ", PREC_INTERSECTION, "unknown");
    case NODE_ARRAY:
      return arrayForm(at(node.elementNodeId), node.readonly);
    case NODE_TUPLE:
      return tupleForm(
        node.elements.map((element) => {
          const type = graph.getNode(element.typeNodeId);
          const nestedRest = type.kind === NODE_REST ? type.innerNodeId : undefined;
          return {
            label: graph.getStringMaybe(element.labelId),
            type: at(nestedRest ?? element.typeNodeId),
            optional: element.optional,
            rest: element.rest || nestedRest !== undefined,
          };
        }),
        node.readonly,
      );
    case NODE_OBJECT:
      return objectForm(node.members.flatMap((member) => graphMemberText(graph, member, scope)));
    case NODE_FUNCTION:
      return printed(graphSignatureText(graph, node, " => ", scope), PREC_CONDITIONAL_OR_FUNCTION);
    case NODE_KEY_OF:
      return printed(`keyof ${operand(at(node.operandNodeId), PREC_PREFIX)}`, PREC_PREFIX);
    case NODE_TYPE_OF:
      return printed(
        `typeof ${node.pathIds.map((id) => graph.getString(id)).join(".")}`,
        PREC_PREFIX,
      );
    case NODE_INDEXED_ACCESS:
      return printed(
        `${operand(at(node.objectNodeId), PREC_POSTFIX)}[${atLowest(node.indexNodeId)}]`,
        PREC_POSTFIX,
      );
    case NODE_CONDITIONAL:
      return printConditional(
        scope,
        at,
        node.checkNodeId,
        node.extendsNodeId,
        node.trueTypeNodeId,
        node.falseTypeNodeId,
      );
    case NODE_MAPPED:
      return mappedForm(
        graph.getString(node.parameterId),
        at(node.sourceNodeId),
        node.nameTypeNodeId ? at(node.nameTypeNodeId) : undefined,
        at(node.valueNodeId),
        mappedOptionalToken(node.optionalModifier),
        mappedReadonlyPrefix(node.readonlyModifier),
      );
    case NODE_TEMPLATE_LITERAL:
      return templateForm(
        node.quasiIds.map((id) => graph.getString(id)),
        node.expressionNodeIds.map(at),
      );
    case NODE_PARENTHESIZED:
      return printed(`(${atLowest(node.innerNodeId)})`, PREC_PRIMARY);
    case NODE_INFER:
      return printInfer(scope, graph.getString(node.nameId), () =>
        node.constraintNodeId ? at(node.constraintNodeId) : undefined,
      );
    case NODE_REST:
      return printed(`...${atLowest(node.innerNodeId)}`, PREC_PRIMARY);
    case NODE_UNKNOWN:
      // Raw authored syntax of any shape: the loosest form, so an
      // enclosing operator keeps its own grouping.
      return printed(graph.getString(node.rawId), PREC_LOWEST);
    case NODE_SYNTHETIC_SLOT_BINDING:
      // Synthetic carriers display as their `bindingName` (the user-visible
      // identity) — they MUST NOT resolve through `TypeRegistry`.
      return printed(graph.getString(node.bindingNameId), PREC_PRIMARY);
    default: {
      const _exhaustive: never = node;
      throw new Error(
        `graphTypeExprToString: unhandled graph node kind ${(_exhaustive as { kind: number }).kind}`,
      );
    }
  }
}

function graphSignatureText(
  graph: DecodedTypeGraph,
  node: Extract<GraphNodeRecord, { kind: typeof NODE_FUNCTION }>,
  separator: " => " | ": ",
  scope: PrintScope,
): string {
  const typeParameters = node.typeParameterNodeIds.map((id) => {
    const parameter = graph.getNode(id);
    if (parameter.kind !== NODE_TYPE_PARAMETER) {
      return printGraph(graph, id, scope).text;
    }
    return typeParameterText(
      graph.getString(parameter.nameId),
      false,
      parameter.constraintNodeId ? printGraph(graph, parameter.constraintNodeId, scope) : undefined,
      parameter.defaultNodeId ? printGraph(graph, parameter.defaultNodeId, scope) : undefined,
    );
  });
  const parameters = node.parameters.map((parameter, index) =>
    parameterText(
      graph.getStringMaybe(parameter.nameId),
      index,
      parameter.optional,
      parameter.rest,
      printGraph(graph, parameter.typeNodeId, scope),
    ),
  );
  const returnText = node.returnTypeNodeId
    ? operand(printGraph(graph, node.returnTypeNodeId, scope), PREC_LOWEST)
    : "unknown";
  return signatureText(typeParameters, parameters, returnText, separator);
}

function graphMemberKey(
  graph: DecodedTypeGraph,
  key: GraphPropertyKeyRecord,
  scope: PrintScope,
): string {
  switch (key.kind) {
    case "string":
      return memberName(graph.getString(key.nameId));
    case "number":
      return String(key.value);
    case "uniqueSymbol":
      return `[${graph.getString(key.nameId)}]`;
    case "computed":
      return `[${operand(printGraph(graph, key.nodeId, scope), PREC_LOWEST)}]`;
  }
}

/** One member's text, or none for a member kind with no spelling. */
function graphMemberText(
  graph: DecodedTypeGraph,
  member: GraphObjectMemberRecord,
  scope: PrintScope,
): string[] {
  const readonly = member.readonly ? "readonly " : "";
  const optional = member.optional ? "?" : "";
  const signature = (separator: " => " | ": ") => {
    const fn = member.functionNodeId ? graph.getNode(member.functionNodeId) : undefined;
    return fn?.kind === NODE_FUNCTION ? graphSignatureText(graph, fn, separator, scope) : undefined;
  };
  switch (member.kind) {
    case MEMBER_PROPERTY:
      return [
        `${readonly}${graphMemberKey(graph, member.key, scope)}${optional}: ${operand(printGraph(graph, member.typeNodeId, scope), PREC_LOWEST)}`,
      ];
    case MEMBER_INDEX_SIGNATURE:
      return [
        `${readonly}[${graph.getStringMaybe(member.keyNameId) || "key"}: ${operand(printGraph(graph, member.keyTypeNodeId, scope), PREC_LOWEST)}]: ${operand(printGraph(graph, member.valueTypeNodeId, scope), PREC_LOWEST)}`,
      ];
    case MEMBER_CALL_SIGNATURE: {
      const text = signature(": ");
      return text === undefined ? [] : [text];
    }
    case MEMBER_CONSTRUCT_SIGNATURE: {
      const text = signature(": ");
      return text === undefined ? [] : [`new ${text}`];
    }
    case MEMBER_METHOD: {
      const text = signature(": ");
      return text === undefined
        ? []
        : [`${methodKey(graphMemberKey(graph, member.key, scope))}${optional}${text}`];
    }
    case MEMBER_SPREAD:
      // Type syntax has no spread member; the pre-fold operand prints as
      // `...(T)`, as the Rust renderer spells it.
      return [`...(${operand(printGraph(graph, member.typeNodeId, scope), PREC_LOWEST)})`];
    default:
      return [];
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

function nativeTypeExprToString(expr: NativeTypeExpr): string {
  return printNative(expr, newPrintScope()).text;
}

/** The display text of a type expression: TypeScript type syntax that reparses to it. */
export function typeExprDisplayText(expr: NativeTypeExprLike): string {
  return printNativeLike(expr, newPrintScope()).text;
}

function printNativeLike(expr: NativeTypeExprLike, scope: PrintScope): Printed {
  return isGraphTypeExprRef(expr)
    ? printGraph(expr.graph, expr.nodeId, scope)
    : printNative(expr, scope);
}

function printNative(expr: NativeTypeExpr, scope: PrintScope): Printed {
  const print = (child: NativeTypeExpr) => printNative(child, scope);
  const atLowest = (child: NativeTypeExpr) => operand(print(child), PREC_LOWEST);
  switch (expr.kind) {
    case "primitive":
      return printed(expr.name, PREC_PRIMARY);
    case "literal":
      if (typeof expr.value === "string") {
        return printed(
          expr.literalKind === "bigInt" ? `${expr.value}n` : quoteStringLiteral(expr.value),
          PREC_PRIMARY,
        );
      }
      return printed(String(expr.value), PREC_PRIMARY);
    case "ref":
      return applied(expr.name, expr.typeArguments.map(atLowest));
    case "intrinsicApplication":
      return applied(intrinsicDisplayName(expr.op), expr.arguments.map(atLowest));
    case "recursiveRef":
      return applied(expr.name, expr.typeArguments.map(atLowest));
    case "typeParameter":
      return printed(expr.name, PREC_PRIMARY);
    case "keyOf":
      return printed(`keyof ${operand(print(expr.operand), PREC_PREFIX)}`, PREC_PREFIX);
    case "typeOf": {
      const args = expr.typeArgs ?? [];
      const instantiation = args.length > 0 ? `<${args.map(atLowest).join(", ")}>` : "";
      return printed(`typeof ${expr.path.join(".")}${instantiation}`, PREC_PREFIX);
    }
    case "union":
      return joined(expr.types.map(print), " | ", PREC_UNION, "never");
    case "intersection":
      return joined(expr.types.map(print), " & ", PREC_INTERSECTION, "unknown");
    case "array":
      return arrayForm(print(expr.element), expr.readonly);
    case "tuple":
      return tupleForm(
        expr.elements.map((element) => {
          const nestedRest = element.ty.kind === "rest" ? element.ty.inner : undefined;
          return {
            label: element.label ?? undefined,
            type: print(nestedRest ?? element.ty),
            optional: element.optional,
            rest: element.rest || nestedRest !== undefined,
          };
        }),
        expr.readonly,
      );
    case "object":
      return objectForm(expr.properties.flatMap((member) => nativeMemberText(member, scope)));
    case "function":
      return printed(nativeSignatureText(expr, " => ", scope), PREC_CONDITIONAL_OR_FUNCTION);
    case "constructorType":
      return printed(
        `${expr.abstract ? "abstract " : ""}new ${nativeSignatureText(expr, " => ", scope)}`,
        PREC_CONDITIONAL_OR_FUNCTION,
      );
    case "indexedAccess":
      return printed(
        `${operand(print(expr.object), PREC_POSTFIX)}[${atLowest(expr.index)}]`,
        PREC_POSTFIX,
      );
    case "conditional":
      return printConditional(
        scope,
        print,
        expr.check,
        expr.extends,
        expr.trueType,
        expr.falseType,
      );
    case "mapped":
      return mappedForm(
        expr.parameter,
        print(expr.source),
        expr.nameType ? print(expr.nameType) : undefined,
        print(expr.value),
        mappedOptionalToken(mappedModifierToCode(expr.optional)),
        mappedReadonlyPrefix(mappedModifierToCode(expr.readonly)),
      );
    case "templateLiteral":
      return templateForm(expr.quasis, expr.expressions.map(print));
    case "parenthesized":
      return printed(`(${atLowest(expr.inner)})`, PREC_PRIMARY);
    case "infer":
      return printInfer(scope, expr.name, () =>
        expr.constraint ? printNativeLike(expr.constraint, scope) : undefined,
      );
    case "rest":
      return printed(`...${atLowest(expr.inner)}`, PREC_PRIMARY);
    case "unknown":
      // Raw authored syntax of any shape: the loosest form, so an
      // enclosing operator keeps its own grouping.
      return printed(expr.raw, PREC_LOWEST);
    case "syntheticSlotBinding":
      // Synthetic carriers render as their user-visible `bindingName` — they
      // MUST NOT route through `TypeRegistry` for display.
      return printed(expr.bindingName, PREC_PRIMARY);
    default: {
      const _exhaustive: never = expr;
      return printed((_exhaustive as { kind: string }).kind, PREC_PRIMARY);
    }
  }
}

function nativeSignatureText(
  fn: NativeFunctionExpr,
  separator: " => " | ": ",
  scope: PrintScope,
): string {
  const print = (child: NativeTypeExpr) => printNative(child, scope);
  const typeParameters = (fn.typeParameters ?? []).map((parameter) =>
    typeParameterText(
      parameter.name,
      parameter.isConst === true,
      parameter.constraint ? print(parameter.constraint) : undefined,
      parameter.default ? print(parameter.default) : undefined,
    ),
  );
  const parameters = fn.parameters.map((parameter, index) =>
    parameterText(parameter.name, index, parameter.optional, parameter.rest, print(parameter.ty)),
  );
  // A predicate signature prints its predicate in the return position
  // (`x is T`, `asserts this`), never the `boolean` / `void` it denotes.
  const predicate = fn.predicate;
  let returnText: string;
  if (predicate) {
    const subject = predicate.this ? "this" : (predicate.parameter ?? "this");
    const target = predicate.ty ? ` is ${operand(print(predicate.ty), PREC_LOWEST)}` : "";
    returnText = `${predicate.asserts ? "asserts " : ""}${subject}${target}`;
  } else {
    returnText = fn.returnType ? operand(print(fn.returnType), PREC_LOWEST) : "unknown";
  }
  return signatureText(typeParameters, parameters, returnText, separator);
}

function nativeMemberKey(member: NativeObjectMember, scope: PrintScope): string | undefined {
  const print = (child: NativeTypeExpr) => printNative(child, scope);
  const key = member.key;
  if (!key) {
    return member.name === undefined ? undefined : memberName(member.name);
  }
  switch (key.kind) {
    case "string":
      return memberName(key.value);
    case "number":
      return String(key.value);
    case "uniqueSymbol":
      return `[${key.identity.symbol}]`;
    case "computed":
      return `[${operand(print(key.expression), PREC_LOWEST)}]`;
  }
}

/** One member's text, or none for a member missing what it prints. */
function nativeMemberText(member: NativeObjectMember, scope: PrintScope): string[] {
  const print = (child: NativeTypeExpr) => printNative(child, scope);
  const readonly = member.readonly ? "readonly " : "";
  const optional = member.optional ? "?" : "";
  switch (member.memberKind) {
    case "property": {
      const key = nativeMemberKey(member, scope);
      return key === undefined || member.ty === undefined
        ? []
        : [`${readonly}${key}${optional}: ${operand(print(member.ty), PREC_LOWEST)}`];
    }
    case "indexSignature":
      return member.keyType === undefined || member.valueType === undefined
        ? []
        : [
            `${readonly}[${member.keyName || "key"}: ${operand(print(member.keyType), PREC_LOWEST)}]: ${operand(print(member.valueType), PREC_LOWEST)}`,
          ];
    case "callSignature":
      return member.function ? [nativeSignatureText(member.function, ": ", scope)] : [];
    case "constructSignature":
      return member.function ? [`new ${nativeSignatureText(member.function, ": ", scope)}`] : [];
    case "method": {
      const key = nativeMemberKey(member, scope);
      return key === undefined || !member.function
        ? []
        : [`${methodKey(key)}${optional}${nativeSignatureText(member.function, ": ", scope)}`];
    }
    case "spread":
      // Type syntax has no spread member; the pre-fold operand prints as
      // `...(T)`, as the Rust renderer spells it.
      return member.ty === undefined ? [] : [`...(${operand(print(member.ty), PREC_LOWEST)})`];
    default:
      return [];
  }
}
