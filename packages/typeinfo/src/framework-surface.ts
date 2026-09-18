/**
 * Typed decode of the framework-surface wire payload.
 *
 * The native binding `VerterHost.resolveFrameworkSurfaceWithAudit`
 * returns a protobuf-encoded `TypeInfoGraphResponse` (the
 * `framework_surface` arm on success, the `error` arm on a typed
 * rejection). This module decodes those bytes into an ergonomic
 * TypeScript surface:
 *
 * - {@link FrameworkSurface} — `framework` tag + a per-kind map carrying
 *   each kind's support status and resolved members (member names are
 *   resolved through the graph string table here, so consumers never
 *   touch the interned id space). Member types decode from the payload
 *   graph's `type_node_id`; an opaque/unknown descriptor is an explicit
 *   unsupported outcome, never a complete callback signature.
 * - {@link FrameworkSurfaceError} — the typed wire error arm.
 *
 * **Status semantics.** Per-kind status is surfaced VERBATIM from
 * the wire `FrameworkSurfaceKindStatus`: a SUPPORTED kind with zero
 * members is supported-empty (a real-but-empty surface), DISTINCT from
 * an UNSUPPORTED kind (which also carries zero members). The decoded
 * `isSupported` / `isUnsupported` flags preserve that distinction so a
 * consumer never has to infer support from member-count emptiness.
 *
 * Kept free of any `@verter/native` import so the decode can be
 * unit-tested without loading the native host binary — the input is raw
 * wire bytes, exactly as the binding emits them.
 */

import { fromBinary } from "@bufbuild/protobuf";
import {
  FrameworkSurfaceDeclarationKind,
  type FrameworkSurfaceKind,
  FrameworkSurfaceKindSupport,
  type FrameworkSurfaceMember as WireFrameworkSurfaceMember,
  type FrameworkSurfaceMemberOrigin as WireFrameworkSurfaceMemberOrigin,
  FrameworkSurfaceOriginHopKind,
  type FrameworkSurfacePayload,
  type FrameworkTag,
  GraphPrimitiveKind,
  type GraphSignature,
  type GraphTypeNode,
  type SemanticTypeGraph,
  TypeInfoGraphResponseSchema,
  type TypeInfoRequestError,
} from "@verter/proto";
import {
  func,
  literal,
  object,
  primitive,
  ref,
  unknown,
  type FunctionParameter,
  type ObjectProperty,
  type PrimitiveName,
  type TypeDescriptor,
} from "@verter/type-ir";

/**
 * One hop in a framework-surface member's declaration ORIGIN chain
 * (schema 4). `kind` selects which fields are populated.
 *
 * Every string field is PRESENCE-AWARE: it is `undefined` when the wire
 * hop did not set it (the `optional` has-bit is unset), distinct from a
 * field whose interned string happens to live at table index 0. A LOCAL
 * hop carries no string fields; an IMPORT hop carries `from` +
 * `importedName` (and `specifier` only when recorded); a REEXPORT hop
 * carries `from` / `to` / `exportedName` / `originalName`; an ALIAS hop
 * carries `aliasName`. An absent field is NEVER resolved through the
 * string table.
 */
export interface FrameworkSurfaceOriginHop {
  /** The hop kind (LOCAL / IMPORT / REEXPORT / ALIAS). */
  readonly kind: FrameworkSurfaceOriginHopKind;
  /** Import / Reexport: the source module canonical (`undefined` when N/A). */
  readonly from?: string;
  /** Import: the raw import specifier, when recorded (`undefined` when absent). */
  readonly specifier?: string;
  /** Import: the imported name in the source module (`undefined` when N/A). */
  readonly importedName?: string;
  /** Reexport: the module the symbol re-exports to (`undefined` when N/A). */
  readonly to?: string;
  /** Reexport: the name the symbol is re-exported under (`undefined` when N/A). */
  readonly exportedName?: string;
  /** Reexport: the original name before the rename (`undefined` when N/A). */
  readonly originalName?: string;
  /** Alias: the alias target name (`undefined` when N/A). */
  readonly aliasName?: string;
}

/** A framework-surface member's resolved declaration (schema 4). */
export interface FrameworkSurfaceMemberDeclaration {
  /** The requested name, resolved through the graph string table. */
  readonly requestedName: string;
  /** The resolved name, resolved through the graph string table. */
  readonly resolvedName: string;
  /** The declaring file canonical, resolved through the string table. */
  readonly canonicalSource: string;
  /** Declaration span start (byte offset in `canonicalSource`). */
  readonly spanStart: number;
  /** Declaration span end (byte offset in `canonicalSource`). */
  readonly spanEnd: number;
  /** The declaration kind. */
  readonly kind: FrameworkSurfaceDeclarationKind;
}

/**
 * A framework-surface member's declaration ORIGIN (schema 4): the
 * resolver-known per-member declaration plus the ordered hop chain to
 * it. Populated only from routes the shared resolver traversed.
 */
export interface FrameworkSurfaceMemberOrigin {
  /**
   * The per-member declaration, when one was resolved. `undefined` for an
   * inline/local member with a declaration file but no separately-named
   * declaration.
   */
  readonly declaration?: FrameworkSurfaceMemberDeclaration;
  /** The ordered hop chain from the requesting file to the declaration. */
  readonly chain: readonly FrameworkSurfaceOriginHop[];
}

/** One resolved member of a framework surface (props/emits/slots/…). */
export interface FrameworkSurfaceMember {
  /** The member name, resolved through the graph string table. */
  readonly name: string;
  /** Whether the member is required (non-optional). */
  readonly required: boolean;
  /** Whether the member is readonly. */
  readonly readonly: boolean;
  /**
   * The member's runtime DEFAULT value source text (schema 4), resolved
   * through the graph string table. `undefined` when the member has no
   * default. Defaults are runtime expressions, not types.
   */
  readonly default?: string;
  /**
   * The member's resolver-known declaration ORIGIN (schema 4).
   * `undefined` when no origin was resolver-known (a synthetic /
   * multi-origin member, or an adapter that does not derive origins).
   */
  readonly origin?: FrameworkSurfaceMemberOrigin;
  /**
   * Member type decoded from the payload graph. Absent when the wire
   * `type_node_id` is the 0 sentinel. An `unknown` / opaque descriptor
   * is an explicit unsupported outcome — it cannot claim a complete
   * callback signature. A `function` descriptor carries parameter and
   * return structure; a `ref` is the shallow-by-default alias.
   */
  readonly type?: TypeDescriptor;
}

/** A single framework-surface kind's resolved status and members. */
export interface FrameworkSurfaceKindResult {
  /** The wire support status (SUPPORTED / UNSUPPORTED / PARTIAL / …). */
  readonly support: FrameworkSurfaceKindSupport;
  /**
   * `true` when the kind is SUPPORTED. A SUPPORTED kind with zero
   * {@link members} is supported-empty — a real surface that is empty,
   * NOT unsupport.
   */
  readonly isSupported: boolean;
  /** `true` when the kind is UNSUPPORTED. */
  readonly isUnsupported: boolean;
  /** `true` when the kind is PARTIAL (a usable subset). */
  readonly isPartial: boolean;
  /** The resolved members (empty for supported-empty / unsupported). */
  readonly members: readonly FrameworkSurfaceMember[];
  /** Per-kind diagnostics, resolved through the graph string table. */
  readonly diagnostics: readonly string[];
}

/** The decoded `framework_surface` response arm. */
export interface FrameworkSurface {
  /** The wire framework tag (e.g. `FrameworkTag.VUE`). */
  readonly framework: FrameworkTag;
  /**
   * Per-kind resolved surfaces. A v3 payload carries exactly one entry
   * per known {@link FrameworkSurfaceKind}.
   */
  readonly kinds: ReadonlyMap<FrameworkSurfaceKind, FrameworkSurfaceKindResult>;
}

/** The decoded `error` response arm — the TYPED wire error variant. */
export interface FrameworkSurfaceError {
  /**
   * The typed error discriminant. `error.case` is the wire
   * `TypeInfoRequestError` oneof variant name (e.g. `"malformedPayload"`);
   * `error.value` is its typed payload (e.g. `{ detail: string }`). This is
   * the structural error, never a stringified display.
   */
  readonly error: TypeInfoRequestError["kind"];
}

/**
 * Decode the protobuf-encoded `TypeInfoGraphResponse` bytes returned by
 * `VerterHost.resolveFrameworkSurfaceWithAudit` into a
 * {@link FrameworkSurface} (the `framework_surface` arm) or a
 * {@link FrameworkSurfaceError} (the `error` arm).
 *
 * The native binding always produces a typed response (validation-first
 * executor), so this never throws on a well-formed buffer; a malformed
 * buffer surfaces as a `fromBinary` decode error.
 */
export function decodeFrameworkSurfaceResponse(
  bytes: Uint8Array,
): FrameworkSurface | FrameworkSurfaceError {
  const response = fromBinary(TypeInfoGraphResponseSchema, bytes);
  const kind = response.kind;

  if (kind.case === "frameworkSurface") {
    return decodeFrameworkSurfacePayload(kind.value);
  }
  if (kind.case === "error") {
    // Surface the TYPED error oneof (`{ case, value }`) verbatim — never a
    // stringified display. The framework-surface operation never produces
    // the `graph` arm, so this is the only error path.
    return { error: kind.value.kind };
  }
  // The `graph` arm is never produced for a framework-surface request, and
  // an empty `kind` is malformed — both surface as a typed-unspecified
  // error variant rather than a fabricated string.
  return { error: { case: undefined } as TypeInfoRequestError["kind"] };
}

/**
 * Decode an already-decoded `framework_surface` payload arm into the
 * public {@link FrameworkSurface}. Exported so the graph operation-DTO
 * decode (`graph.ts`) can handle the arm without duplicating this
 * projection.
 */
export function decodeFrameworkSurfacePayload(payload: FrameworkSurfacePayload): FrameworkSurface {
  const strings = payload.graph?.strings?.entries ?? [];
  const graph = payload.graph;

  const kinds = new Map<FrameworkSurfaceKind, FrameworkSurfaceKindResult>();
  for (const entry of payload.surfaces) {
    const support = entry.status?.support ?? FrameworkSurfaceKindSupport.UNSPECIFIED;
    const members: FrameworkSurfaceMember[] = entry.members.map((m) =>
      decodeMember(strings, graph, m),
    );
    const diagnostics: string[] =
      entry.status?.diagnostics.map((d) => resolveString(strings, d.messageNameId)) ?? [];

    kinds.set(entry.kind, {
      support,
      isSupported: support === FrameworkSurfaceKindSupport.SUPPORTED,
      isUnsupported: support === FrameworkSurfaceKindSupport.UNSUPPORTED,
      isPartial: support === FrameworkSurfaceKindSupport.PARTIAL,
      members,
      diagnostics,
    });
  }

  return { framework: payload.framework, kinds };
}

/**
 * Decode one wire member into the public {@link FrameworkSurfaceMember},
 * resolving the name + (schema 4) the runtime default source text and the
 * declaration origin through the graph string table.
 *
 * `default` is presence-aware: the wire `default_value_id` is `optional`,
 * so an absent default (no field) decodes to `undefined`, distinct from a
 * default whose interned string id is 0. `origin` is `undefined` unless the
 * member carried a resolver-known origin.
 */
function decodeMember(
  strings: readonly string[],
  graph: SemanticTypeGraph | undefined,
  m: WireFrameworkSurfaceMember,
): FrameworkSurfaceMember {
  const member: FrameworkSurfaceMember = {
    name: resolveString(strings, m.nameId),
    required: m.required,
    readonly: m.readonly,
    default: m.defaultValueId === undefined ? undefined : resolveString(strings, m.defaultValueId),
    origin: m.origin === undefined ? undefined : decodeMemberOrigin(strings, m.origin),
  };
  // Node id 0 is the absent sentinel — no type field, not an opaque claim.
  if (graph === undefined || m.typeNodeId === 0) {
    return member;
  }
  return { ...member, type: memberTypeFromGraph(graph, m.typeNodeId) };
}

/**
 * Project one payload-graph node into the public `TypeDescriptor` space.
 *
 * Bounded and total: a cycle or a kind outside the member-type vocabulary
 * becomes `unknown(...)`. An opaque node stays opaque — callers must not
 * treat it as a complete callback signature.
 */
function memberTypeFromGraph(graph: SemanticTypeGraph, nodeId: number): TypeDescriptor {
  return walkMemberType(graph, nodeId, new Set());
}

function walkMemberType(
  graph: SemanticTypeGraph,
  nodeId: number,
  visited: ReadonlySet<number>,
): TypeDescriptor {
  if (nodeId === 0) {
    return unknown("absent");
  }
  if (visited.has(nodeId)) {
    return unknown("[cycle]");
  }
  const node: GraphTypeNode | undefined = graph.nodes[nodeId];
  const kind = node?.kind;
  if (!kind || kind.case === undefined) {
    return unknown(kind ? "graph node without kind" : "absent graph node");
  }
  const next = new Set(visited);
  next.add(nodeId);
  const walk = (id: number): TypeDescriptor => walkMemberType(graph, id, next);
  const strings = graph.strings?.entries ?? [];

  switch (kind.case) {
    case "primitive":
      return primitive(primitiveName(kind.value.kind));
    case "literal": {
      const inner = kind.value.value?.kind;
      if (!inner || inner.case === undefined) return unknown("literal");
      switch (inner.case) {
        case "stringNameId":
          return literal(strings[inner.value] ?? "");
        case "numberBits":
          return literal(f64FromBits(inner.value));
        case "booleanValue":
          return literal(inner.value);
        default:
          return unknown("literal");
      }
    }
    case "reference": {
      const symbol = graph.symbols[kind.value.symbolId];
      const name = symbol ? (strings[symbol.nameId] ?? "") : "";
      return ref(name);
    }
    case "object": {
      if (kind.value.callSignatureRefs.length > 0) {
        return signatureDescriptor(graph, strings, walk, kind.value.callSignatureRefs[0]);
      }
      if (kind.value.constructSignatureRefs.length > 0) {
        return signatureDescriptor(graph, strings, walk, kind.value.constructSignatureRefs[0]);
      }
      const properties: ObjectProperty[] = kind.value.members.map((member) => {
        const key = member.propertyKey?.key;
        const name = key?.case === "stringId" ? (strings[Number(key.value ?? 0)] ?? "") : "";
        return {
          name,
          type: walk(member.valueNodeId),
          optional: member.optional,
        };
      });
      return object(properties);
    }
    case "opaque":
      return unknown(opaqueMessage(strings, kind.value.error));
    default:
      return unknown(String(kind.case));
  }
}

function signatureDescriptor(
  graph: SemanticTypeGraph,
  strings: readonly string[],
  walk: (id: number) => TypeDescriptor,
  sigRef: number | undefined,
): TypeDescriptor {
  const signature: GraphSignature | undefined =
    sigRef === undefined ? undefined : graph.signatures[sigRef];
  if (!signature) {
    return unknown("callable without a signature");
  }
  const parameters: FunctionParameter[] = signature.parameters.map((param, idx) => {
    const name = strings[param.nameId] ?? "";
    return {
      name: name !== "" ? name : `arg${idx}`,
      type: walk(param.typeNodeId),
      optional: param.optional,
    };
  });
  const returnType =
    signature.returnTypeNodeId !== 0 ? walk(signature.returnTypeNodeId) : primitive("void");
  return func(parameters, returnType);
}

function opaqueMessage(
  strings: readonly string[],
  error: { kind?: { case?: string; value?: Record<string, unknown> } } | undefined,
): string {
  const kind = error?.kind;
  if (kind?.case === "other") {
    const payload = (kind.value ?? {}) as Record<string, unknown>;
    return strings[Number(payload.messageNameId ?? 0)] ?? "opaque";
  }
  return kind?.case ?? "opaque";
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

function f64FromBits(bits: bigint): number {
  const buffer = new ArrayBuffer(8);
  new BigUint64Array(buffer)[0] = bits;
  return new Float64Array(buffer)[0];
}

/** Decode a wire member origin into the public shape (string ids resolved). */
function decodeMemberOrigin(
  strings: readonly string[],
  origin: WireFrameworkSurfaceMemberOrigin,
): FrameworkSurfaceMemberOrigin {
  const declaration =
    origin.declaration === undefined
      ? undefined
      : {
          requestedName: resolveString(strings, origin.declaration.requestedNameId),
          resolvedName: resolveString(strings, origin.declaration.resolvedNameId),
          canonicalSource: resolveString(strings, origin.declaration.canonicalSourceId),
          spanStart: origin.declaration.spanStart,
          spanEnd: origin.declaration.spanEnd,
          kind: origin.declaration.kind,
        };
  const chain: FrameworkSurfaceOriginHop[] = origin.chain.map((hop) => ({
    kind: hop.kind,
    // PRESENCE-AWARE: each hop string id is `optional` on the wire, so an
    // unset field is `undefined` here — NEVER resolved through the string
    // table (the graph table is zero-based, so id 0 is a real entry, not an
    // absent sentinel). Only a present id resolves to its interned string.
    from: resolveOptionalString(strings, hop.fromId),
    specifier: resolveOptionalString(strings, hop.specifierId),
    importedName: resolveOptionalString(strings, hop.importedNameId),
    to: resolveOptionalString(strings, hop.toId),
    exportedName: resolveOptionalString(strings, hop.exportedNameId),
    originalName: resolveOptionalString(strings, hop.originalNameId),
    aliasName: resolveOptionalString(strings, hop.aliasNameId),
  }));
  return { declaration, chain };
}

/**
 * Resolve an interned string-table index to its string, or the empty
 * string when out of range — the decode never throws on a malformed
 * index (a structurally-broken payload).
 */
function resolveString(strings: readonly string[], id: number): string {
  return strings[id] ?? "";
}

/**
 * Resolve a PRESENCE-AWARE (wire-`optional`) string id: `undefined` when
 * the field was not set on the wire (genuinely absent), otherwise the
 * interned string. An absent field is never resolved through the string
 * table — this is the guard against the zero-based-table-vs-0-sentinel
 * collision (the graph table's entry 0 is a real interned string, so a
 * plain id-0 absent sentinel would fabricate it).
 */
function resolveOptionalString(
  strings: readonly string[],
  id: number | undefined,
): string | undefined {
  return id === undefined ? undefined : resolveString(strings, id);
}
