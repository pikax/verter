/**
 * Typeinfo graph protocol operation DTOs — the consumer closure.
 *
 * `@verter/typeinfo` closes over the typeinfo graph protocol: the
 * typed request builder ({@link buildTypeInfoRequest}) over the wire
 * `TypeInfoGraphRequest` envelope, and the typed decode
 * ({@link decodeTypeInfoResult}) of every `TypeInfoGraphResponse` arm —
 * including the bounded `SemanticTypeGraph` export, decoded into the
 * public `TypeDescriptor` space with identity, provenance, and
 * deterministic ordering preserved.
 *
 * **Bounded export is structural.** The builder refuses an expanded
 * closure without explicit in-range budgets — the same contract the
 * host's envelope validator enforces — so an unbounded export request
 * is rejected client-side before it ever reaches the wire. The
 * producer-side walk is budget-bounded and fail-closed (a budget
 * marker is an `opaque` node, decoded here as `unknown(raw)`).
 *
 * **String-table identity.** The graph string table is zero-based: id
 * 0 is a REAL interned string, never an absent sentinel. Every decode
 * resolves ids through the table; only node id 0 is the wire
 * absent-sentinel on child refs (the producer reserves it).
 */

import { create, fromBinary } from "@bufbuild/protobuf";
import {
  GraphClosurePolicySchema,
  GraphPrimitiveKind,
  TypeInfoGraphRequestSchema,
  TypeInfoGraphResponseSchema,
  type GraphClosurePolicy,
  type GraphSignature,
  type GraphSymbolNode,
  type GraphTypeNode,
  type SemanticTypeGraph,
  type TypeInfoGraphRequest as WireTypeInfoGraphRequest,
  type TypeInfoRequestError,
  TYPEINFO_GRAPH_SCHEMA_VERSION,
} from "@verter/proto";
import {
  array,
  func,
  indexedAccess,
  intersection,
  literal,
  object,
  primitive,
  recursiveRef,
  ref,
  tuple,
  typeParameter,
  union,
  unknown,
  type FunctionParameter,
  type FunctionType,
  type ObjectIndexSignature,
  type ObjectProperty,
  type PrimitiveName,
  type TypeDescriptor,
} from "@verter/type-ir";

import {
  decodeFrameworkSurfacePayload,
  type FrameworkSurface,
  type FrameworkSurfaceError,
} from "./framework-surface.js";

/** Host-side cap on an expanded closure's node budget (mirrors the
 *  `verter_session` request validator's `MAX_EXPANSION_NODE_BUDGET`). */
export const MAX_EXPANSION_NODE_BUDGET = 16384;

/** Host-side cap on an expanded closure's depth budget (mirrors the
 *  `verter_session` request validator's `MAX_EXPANSION_DEPTH_BUDGET`). */
export const MAX_EXPANSION_DEPTH_BUDGET = 256;

/** Projection-mode tags accepted by {@link ResolveSymbolGraphQuery}. */
export type GraphProjectionModeTag = "identity" | "navigate" | "shallow" | "expanded" | "skeleton";

/** The bounded closure specification for a graph query. */
export type GraphClosureSpec =
  | { kind: "rootOnly" }
  | { kind: "oneLevel" }
  | { kind: "expanded"; nodeBudget: number; depthBudget: number };

/** The ergonomic query shape {@link buildTypeInfoRequest} lowers. */
export interface ResolveSymbolGraphQuery {
  /** Canonical id of the file whose top-level scope is queried. */
  canonicalId: string;
  /** The declaration name to resolve. */
  name: string;
  /** Projection mode; defaults to `"expanded"`. */
  mode?: GraphProjectionModeTag;
  /** Closure policy; defaults to the bounded one-level policy. */
  closure?: GraphClosureSpec;
}

const MODE_TAGS: Record<GraphProjectionModeTag, number> = {
  identity: 0,
  navigate: 1,
  shallow: 2,
  expanded: 3,
  skeleton: 4,
};

/**
 * Build the typed resolve-symbol `TypeInfoGraphRequest` envelope.
 *
 * Throws when the query is structurally unbounded (an expanded closure
 * without explicit in-range budgets) or incomplete (empty canonical /
 * name) — the client-side half of the bounded-export contract.
 */
export function buildTypeInfoRequest(query: ResolveSymbolGraphQuery): WireTypeInfoGraphRequest {
  if (!query.canonicalId) {
    throw new TypeError("buildTypeInfoRequest: canonicalId is required");
  }
  if (!query.name) {
    throw new TypeError("buildTypeInfoRequest: name is required");
  }
  const closure = closureInit(query.closure ?? { kind: "oneLevel" });
  return create(TypeInfoGraphRequestSchema, {
    schemaVersion: TYPEINFO_GRAPH_SCHEMA_VERSION,
    operation: 0, // GraphOperation.RESOLVE_SYMBOL
    payload: {
      case: "resolveSymbol",
      value: {
        canonicalId: query.canonicalId,
        name: query.name,
        context: { mode: MODE_TAGS[query.mode ?? "expanded"], demand: 0 },
        closure,
        displayPolicy: {
          qualification: 1,
          branding: 1,
          budgets: { maxStringLength: 4096, maxDepth: 16 },
        },
        includeProvenance: false,
        includeDiagnostics: true,
        includeProjection: [],
        includeDegraded: false,
      },
    },
  });
}

function closureInit(closure: GraphClosureSpec): GraphClosurePolicy {
  switch (closure.kind) {
    case "rootOnly":
      return create(GraphClosurePolicySchema, { kind: { case: "rootOnly", value: {} } });
    case "oneLevel":
      return create(GraphClosurePolicySchema, { kind: { case: "oneLevel", value: {} } });
    case "expanded": {
      const { nodeBudget, depthBudget } = closure;
      if (
        !Number.isInteger(nodeBudget) ||
        !Number.isInteger(depthBudget) ||
        nodeBudget < 0 ||
        depthBudget < 0 ||
        nodeBudget > MAX_EXPANSION_NODE_BUDGET ||
        depthBudget > MAX_EXPANSION_DEPTH_BUDGET ||
        (nodeBudget === 0 && depthBudget === 0)
      ) {
        throw new RangeError(
          `buildTypeInfoRequest: expanded closure requires in-range budgets ` +
            `(0..${MAX_EXPANSION_NODE_BUDGET} nodes, 0..${MAX_EXPANSION_DEPTH_BUDGET} depth, ` +
            `at least one non-zero), got node=${nodeBudget} depth=${depthBudget}`,
        );
      }
      return create(GraphClosurePolicySchema, {
        kind: { case: "expanded", value: { nodeBudget, depthBudget } },
      });
    }
  }
}

/** The decoded read-only view over a wire `SemanticTypeGraph`. */
export interface SemanticTypeGraphView {
  readonly schemaVersion: number;
  readonly strings: readonly string[];
  readonly nodes: readonly GraphTypeNode[];
  readonly symbols: readonly GraphSymbolNode[];
  readonly signatures: readonly GraphSignature[];
  readonly rootIds: readonly number[];
}

/** The decoded `TypeInfoGraphResponse` — one variant per wire arm. */
export type TypeInfoResult =
  | { kind: "graph"; graph: SemanticTypeGraphView; root: TypeDescriptor }
  | { kind: "error"; error: TypeInfoRequestError["kind"] }
  | { kind: "frameworkSurface"; surface: FrameworkSurface | FrameworkSurfaceError };

/**
 * Decode a protobuf-encoded `TypeInfoGraphResponse` into the typed
 * {@link TypeInfoResult}. The `graph` arm carries the bounded
 * `SemanticTypeGraph` view plus the root descriptor; the `error` arm
 * carries the TYPED wire error (never a stringified display); the
 * `framework_surface` arm decodes through the framework-surface
 * decoder.
 */
export function decodeTypeInfoResult(bytes: Uint8Array): TypeInfoResult {
  const response = fromBinary(TypeInfoGraphResponseSchema, bytes);
  const kind = response.kind;
  if (kind.case === "graph" && kind.value) {
    const graph = kind.value as SemanticTypeGraph;
    const view: SemanticTypeGraphView = {
      schemaVersion: graph.schemaVersion,
      strings: graph.strings?.entries ?? [],
      nodes: graph.nodes,
      symbols: graph.symbols,
      signatures: graph.signatures,
      rootIds: graph.rootIds,
    };
    const rootId = view.rootIds[0];
    const root =
      rootId === undefined ? unknown("graph without a root") : graphNodeToDescriptor(view, rootId);
    return { kind: "graph", graph: view, root };
  }
  if (kind.case === "error" && kind.value) {
    return { kind: "error", error: kind.value.kind };
  }
  if (kind.case === "frameworkSurface" && kind.value) {
    return { kind: "frameworkSurface", surface: decodeFrameworkSurfacePayload(kind.value) };
  }
  // An empty `kind` is malformed — surface a typed-unspecified error
  // variant rather than a fabricated string.
  return {
    kind: "error",
    error: { case: undefined } as TypeInfoRequestError["kind"],
  };
}

/** Resolve an interned string id; "" when out of range (never throws). */
function strAt(view: SemanticTypeGraphView, id: number): string {
  return view.strings[id] ?? "";
}

/** Resolve a symbol id to its interned name ("" when unresolvable). */
function symbolName(view: SemanticTypeGraphView, symbolId: number): string {
  const symbol = view.symbols[symbolId];
  return symbol ? strAt(view, symbol.nameId) : "";
}

/** Rebuild an f64 from its wire bit pattern (fixed64 → bigint). */
function f64FromBits(bits: bigint): number {
  const buffer = new ArrayBuffer(8);
  new BigUint64Array(buffer)[0] = bits;
  return new Float64Array(buffer)[0];
}

/**
 * Decode one graph node into the public `TypeDescriptor` space.
 *
 * Total (every wire node kind maps); shapes without a `TypeDescriptor`
 * counterpart decode to `unknown(raw)` shells — the same terminal
 * compatibility spelling the legacy `TypeExpr` JSON decode uses.
 * Cycle-guarded: a node visited twice on one walk decodes to a
 * `recursiveRef`-free `unknown("[cycle]")` stop, never infinite
 * recursion.
 */
export function graphNodeToDescriptor(
  view: SemanticTypeGraphView,
  nodeId: number,
  visited: ReadonlySet<number> = new Set(),
): TypeDescriptor {
  if (visited.has(nodeId)) {
    return unknown("[cycle]");
  }
  const node = view.nodes[nodeId];
  const kind = node?.kind;
  if (!kind || kind.case === undefined) {
    // Node id 0 is the reserved absent-sentinel slot; an unset kind
    // anywhere else is a malformed payload.
    return unknown(kind ? "graph node without kind" : "absent graph node");
  }
  const next = new Set(visited);
  next.add(nodeId);
  const walk = (id: number): TypeDescriptor =>
    id === 0 ? unknown("absent") : graphNodeToDescriptor(view, id, next);

  switch (kind.case) {
    case "primitive":
      return primitive(primitiveName(kind.value.kind));
    case "literal": {
      const value = kind.value.value;
      const inner = value?.kind;
      if (!inner || inner.case === undefined) return unknown("literal");
      switch (inner.case) {
        case "stringNameId":
          return literal(strAt(view, inner.value));
        case "numberBits":
          return literal(f64FromBits(inner.value));
        case "booleanValue":
          return literal(inner.value);
        case "bigintNameId":
          return unknown(`${strAt(view, inner.value)}n`);
      }
      return unknown("literal");
    }
    case "union":
      return union(kind.value.memberNodeIds.map(walk));
    case "intersection":
      return intersection(kind.value.memberNodeIds.map(walk));
    case "array":
      return array(walk(kind.value.elementNodeId));
    case "tuple":
      return tuple(kind.value.elements.map((element) => walk(element.valueNodeId)));
    case "reference":
      return ref(symbolName(view, kind.value.symbolId));
    case "aliasInstantiation":
      return ref(
        symbolName(view, kind.value.aliasSymbolId),
        kind.value.typeArgumentNodeIds.map(walk),
      );
    case "typeParameter": {
      const name = strAt(view, kind.value.nameId);
      return typeParameter(name, {
        constraint:
          kind.value.constraintNodeId !== 0 ? walk(kind.value.constraintNodeId) : undefined,
        default: kind.value.defaultNodeId !== 0 ? walk(kind.value.defaultNodeId) : undefined,
      });
    }
    case "keyOf":
      return unknown(`keyof ${brief(walk(kind.value.baseNodeId))}`);
    case "typeofNode":
      return unknown(`typeof ${kind.value.pathNameIds.map((id) => strAt(view, id)).join(".")}`);
    case "indexedAccess":
      return indexedAccess(walk(kind.value.objectNodeId), walk(kind.value.indexNodeId));
    case "conditional":
      return unknown(
        `${brief(walk(kind.value.checkNodeId))} extends ${brief(
          walk(kind.value.extendsNodeId),
        )} ? ${brief(walk(kind.value.trueBranchNodeId))} : ${brief(
          walk(kind.value.falseBranchNodeId),
        )}`,
      );
    case "mapped":
      return unknown(
        `{ [key in ${brief(walk(kind.value.sourceNodeId))}]: ${brief(
          walk(kind.value.valueTypeNodeId),
        )} }`,
      );
    case "templateLiteral":
      return unknown(`templateLiteral(${kind.value.quasiNameIds.length})`);
    case "inferNode":
      return unknown(`infer ${strAt(view, kind.value.nameId)}`);
    case "cycle": {
      const name =
        kind.value.participants.length > 0 ? symbolName(view, kind.value.participants[0]) : "";
      const args: TypeDescriptor[] = [];
      return recursiveRef(name, args, []);
    }
    case "object":
      return objectDescriptor(view, kind.value, walk);
    case "objectSpreadProgram":
      return spreadProgramDescriptor(view, kind.value.effects, walk);
    case "opaque":
      return unknown(opaqueMessage(view, kind.value.error));
    // Node kinds the bounded export never produces today decode to a
    // named shell — total mapping, never a crash.
    case "uniqueSymbol":
      return unknown("uniqueSymbol");
    case "satisfiesNode":
      return unknown("satisfies");
    default:
      return unknown(String(kind.case));
  }
}

/** Structural shape of a decoded wire `GraphObject`. */
interface WireObjectShape {
  members: ReadonlyArray<WireObjectMemberShape>;
  indexSignatures: ReadonlyArray<{ keyKind: number; valueNodeId: number; readonly: boolean }>;
  callSignatureRefs: number[];
  constructSignatureRefs: number[];
}

interface WireObjectMemberShape {
  valueNodeId: number;
  optional: boolean;
  readonly: boolean;
  memberKind: number;
  propertyKey?: { key?: { case?: string; value?: unknown } };
}

function objectDescriptor(
  view: SemanticTypeGraphView,
  value: WireObjectShape,
  walk: (id: number) => TypeDescriptor,
): TypeDescriptor {
  const properties: ObjectProperty[] = [];
  const indexSignatures: ObjectIndexSignature[] = [];
  for (const member of value.members) {
    const name = keyNameFromOneof(view, member.propertyKey, walk);
    const type = walk(member.valueNodeId);
    if (member.memberKind === 1 || member.memberKind === 2 || member.memberKind === 3) {
      // method / get / set — the value node is the callable-object
      // carrying the signature; `func`-decode it.
      properties.push({
        name,
        type: callableDescriptor(view, member.valueNodeId),
        optional: member.optional,
      });
    } else {
      properties.push({ name, type, optional: member.optional });
    }
  }
  for (const index of value.indexSignatures) {
    indexSignatures.push({
      keyName: "key",
      keyType: primitive(indexKeyName(index.keyKind)),
      valueType: walk(index.valueNodeId),
      readonly: index.readonly,
    });
  }
  const callSignatures = value.callSignatureRefs.map((sigRef) =>
    signatureDescriptor(view, view.signatures[sigRef], walk),
  );
  const constructSignatures = value.constructSignatureRefs.map((sigRef) =>
    signatureDescriptor(view, view.signatures[sigRef], walk),
  );
  return object(properties, {
    ...(indexSignatures.length > 0 ? { indexSignatures } : {}),
    ...(callSignatures.length > 0 ? { callSignatures } : {}),
    ...(constructSignatures.length > 0 ? { constructSignatures } : {}),
  });
}

function spreadProgramDescriptor(
  view: SemanticTypeGraphView,
  effects: ReadonlyArray<{
    kind?: { case?: string; value?: Record<string, unknown> };
  }>,
  walk: (id: number) => TypeDescriptor,
): TypeDescriptor {
  const properties: ObjectProperty[] = [];
  const indexSignatures: ObjectIndexSignature[] = [];
  for (const effect of effects) {
    const caseName = effect.kind?.case;
    const payload = (effect.kind?.value ?? {}) as Record<string, unknown>;
    switch (caseName) {
      case "directProperty":
      case "directMethod":
      case "directGet":
      case "directSet": {
        const key = payload.propertyKey as { key?: { case?: string; value?: unknown } } | undefined;
        const name = keyNameFromOneof(view, key, walk);
        const valueNodeId = Number(payload.valueNodeId ?? 0);
        properties.push({
          name,
          type:
            caseName === "directProperty"
              ? walk(valueNodeId)
              : callableDescriptor(view, valueNodeId),
          optional: Boolean(payload.optional),
        });
        break;
      }
      case "directIndex": {
        indexSignatures.push({
          keyName: "key",
          keyType: primitive("string"),
          valueType: walk(Number(payload.valueTypeNodeId ?? 0)),
          readonly: Boolean(payload.readonly),
        });
        break;
      }
      // Spread operands and bare call/construct effects have no flat
      // TypeDescriptor member surface — the ordered program stays the
      // canonical authority on the wire.
      default:
        break;
    }
  }
  return object(properties, indexSignatures.length > 0 ? { indexSignatures } : {});
}

function keyNameFromOneof(
  view: SemanticTypeGraphView,
  key: { key?: { case?: string; value?: unknown } } | undefined,
  walk: (id: number) => TypeDescriptor,
): string {
  const inner = key?.key;
  switch (inner?.case) {
    case "stringId":
      return strAt(view, Number(inner.value ?? 0));
    case "canonicalNumber":
      return String(inner.value ?? 0);
    case "uniqueSymbolDeclId":
      return "[Symbol]";
    case "computedNodeId":
      return `[${brief(walk(Number(inner.value ?? 0)))}]`;
    default:
      return "";
  }
}

/** Decode a callable-object node (function / method value) to `func`. */
function callableDescriptor(view: SemanticTypeGraphView, nodeId: number): TypeDescriptor {
  const node = view.nodes[nodeId];
  const kind = node?.kind;
  if (kind?.case === "object" && kind.value.callSignatureRefs.length > 0) {
    return signatureDescriptor(view, view.signatures[kind.value.callSignatureRefs[0]], (id) =>
      graphNodeToDescriptor(view, id),
    );
  }
  if (kind?.case === "object" && kind.value.constructSignatureRefs.length > 0) {
    return signatureDescriptor(view, view.signatures[kind.value.constructSignatureRefs[0]], (id) =>
      graphNodeToDescriptor(view, id),
    );
  }
  return unknown("callable without a signature");
}

function signatureDescriptor(
  view: SemanticTypeGraphView,
  signature: GraphSignature | undefined,
  walk: (id: number) => TypeDescriptor,
): FunctionType {
  if (!signature) {
    // An absent signature is a malformed payload — surface it as a
    // parameter-less `() => unknown` shell that still reads as a
    // function (the descriptor space has no error function shape).
    return func([], primitive("unknown"));
  }
  const parameters: FunctionParameter[] = signature.parameters.map(
    (param, idx): FunctionParameter => ({
      name: param.nameId !== 0 ? strAt(view, param.nameId) : `arg${idx}`,
      type: walk(param.typeNodeId),
      optional: param.optional,
    }),
  );
  const returnType =
    signature.returnTypeNodeId !== 0 ? walk(signature.returnTypeNodeId) : primitive("void");
  return func(parameters, returnType);
}

function opaqueMessage(
  view: SemanticTypeGraphView,
  error: { kind?: { case?: string; value?: Record<string, unknown> } } | undefined,
): string {
  const kind = error?.kind;
  switch (kind?.case) {
    case "miss":
      return "miss";
    case "budgetExceeded": {
      const payload = (kind.value ?? {}) as Record<string, unknown>;
      return `budgetExceeded(${Number(payload.limit ?? 0)})`;
    }
    case "unstableState":
      return "unstableState";
    case "aliasCycle":
      return "aliasCycle";
    case "recursiveRef":
      return "recursiveRef";
    case "unsupportedIntrinsic":
      return "unsupportedIntrinsic";
    case "declPlaceholder":
      return "declPlaceholder";
    case "other": {
      const payload = (kind.value ?? {}) as Record<string, unknown>;
      return strAt(view, Number(payload.messageNameId ?? 0));
    }
    default:
      return "opaque";
  }
}

function primitiveName(kind: GraphPrimitiveKind): PrimitiveName {
  switch (kind) {
    case GraphPrimitiveKind.STRING:
      return "string";
    case GraphPrimitiveKind.NUMBER:
      return "number";
    case GraphPrimitiveKind.BOOLEAN:
      return "boolean";
    case GraphPrimitiveKind.SYMBOL:
      return "symbol";
    case GraphPrimitiveKind.BIGINT:
      return "bigint";
    case GraphPrimitiveKind.ANY:
      return "any";
    case GraphPrimitiveKind.UNKNOWN:
      return "unknown";
    case GraphPrimitiveKind.VOID:
      return "void";
    case GraphPrimitiveKind.NEVER:
      return "never";
    case GraphPrimitiveKind.NULL:
      return "null";
    case GraphPrimitiveKind.UNDEFINED:
      return "undefined";
    case GraphPrimitiveKind.OBJECT:
      return "object";
    default:
      return "unknown";
  }
}

function indexKeyName(keyKind: number): "string" | "number" | "symbol" {
  switch (keyKind) {
    case 1:
      return "number";
    case 2:
      return "symbol";
    default:
      return "string";
  }
}

function brief(descriptor: TypeDescriptor): string {
  switch (descriptor.kind) {
    case "primitive":
      return descriptor.name;
    case "literal":
      return JSON.stringify(descriptor.value);
    case "ref":
      return descriptor.name;
    default:
      return descriptor.kind;
  }
}
