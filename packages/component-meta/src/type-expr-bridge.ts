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
  NODE_REF,
  NODE_REST,
  NODE_TEMPLATE_LITERAL,
  NODE_TUPLE,
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

export type NativeTypeExprLike = NativeTypeExpr | GraphTypeExprRef;

type NativeTypeRegistry = Map<string, NativeTypeExprLike>;

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
      return intersection(
        expr.types.map((type) =>
          typeExprToDescriptor(type, nativeRegistry, visiting, graphVisiting),
        ),
      );

    case "array":
      return array(typeExprToDescriptor(expr.element, nativeRegistry, visiting, graphVisiting));

    case "tuple":
      return tuple(
        expr.elements.map((e) =>
          typeExprToDescriptor(e.ty, nativeRegistry, visiting, graphVisiting),
        ),
      );

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

    case "function": {
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
        return typeRef(
          expr.name,
          expr.typeArguments.map((typeArgument) =>
            typeExprToDescriptor(typeArgument, nativeRegistry, visiting, graphVisiting),
          ),
        );
      }
      return typeRef(expr.name);

    case "typeParameter":
      return nativeTypeParameterToDescriptor(expr, nativeRegistry, visiting, graphVisiting);

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
      return unknown(nativeTypeExprToString(expr));
    }

    case "parenthesized":
      return typeExprToDescriptor(expr.inner, nativeRegistry, visiting, graphVisiting);

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

  const property = resolveObjectProperty(resolvedObject, propertyName);
  if (!property) {
    return undefined;
  }

  if (!property.optional) {
    return typeExprToDescriptor(property.ty, nativeRegistry, visiting, graphVisiting);
  }

  return union([
    typeExprToDescriptor(property.ty, nativeRegistry, visiting, graphVisiting),
    primitive("undefined"),
  ]);
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

function resolveObjectProperty(
  expr: NativeTypeExprLike,
  propertyName: string,
): { ty: NativeTypeExprLike; optional: boolean } | undefined {
  if (isGraphTypeExprRef(expr)) {
    const node = expr.graph.getNode(expr.nodeId);
    if (node.kind !== NODE_OBJECT) {
      return undefined;
    }

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
      ty: createGraphTypeExprRef(expr.graph, member.typeNodeId),
      optional: member.optional,
    };
  }

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
      return intersection(
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
    case NODE_TUPLE:
      return tuple(
        node.elements.map((element) =>
          typeExprToDescriptor(
            createGraphTypeExprRef(expr.graph, element.typeNodeId),
            nativeRegistry,
            visiting,
            graphVisiting,
          ),
        ),
      );
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
    case NODE_KEY_OF:
    case NODE_TYPE_OF:
    case NODE_CONDITIONAL:
    case NODE_MAPPED:
    case NODE_TEMPLATE_LITERAL:
    case NODE_INFER:
    case NODE_REST:
      return unknown(graphTypeExprToString(expr));
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
      return resolved ?? unknown(graphTypeExprToString(expr));
    }
    case NODE_PARENTHESIZED:
      return typeExprToDescriptor(
        createGraphTypeExprRef(expr.graph, node.innerNodeId),
        nativeRegistry,
        visiting,
        graphVisiting,
      );
    case NODE_UNKNOWN:
      return unknown(expr.graph.getString(node.rawId));
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
    case NODE_REF:
      return expr.graph.getString(node.nameId);
    case NODE_KEY_OF:
      return `keyof ${graphTypeExprToString(createGraphTypeExprRef(expr.graph, node.operandNodeId))}`;
    case NODE_TYPE_OF:
      return `typeof ${node.pathIds.map((id) => expr.graph.getString(id)).join(".")}`;
    case NODE_UNKNOWN:
      return expr.graph.getString(node.rawId);
    default:
      return `graphNode(${node.kind})`;
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
