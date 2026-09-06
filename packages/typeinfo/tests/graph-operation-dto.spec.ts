/**
 * Operation-DTO specs for the typeinfo graph protocol consumer closure.
 *
 * `@verter/typeinfo` closes over the graph protocol operation DTOs: a
 * typed request builder (`buildTypeInfoRequest`) over the wire
 * `TypeInfoGraphRequest` envelope, and a typed decode
 * (`decodeTypeInfoResult`) of every `TypeInfoGraphResponse` arm —
 * including the bounded `SemanticTypeGraph` export, decoded into the
 * public `TypeDescriptor` space with identity, provenance, and
 * deterministic ordering preserved.
 *
 * REGRESSION — discriminates the DTO surface from a stub:
 * - an expanded closure without explicit in-range budgets is REFUSED
 *   client-side (unbounded export is structurally rejected, mirroring
 *   the host validator);
 * - the `graph` arm decodes root descriptors with member order and
 *   interned-string identity preserved (string id 0 is a REAL string);
 * - the `error` arm decodes to the typed wire error, never a string.
 */

import { create, toBinary } from "@bufbuild/protobuf";
import {
  GraphDiagnosticSeverity,
  GraphExactness,
  GraphObjectMemberKind,
  GraphOperation,
  GraphOriginEdgeKind,
  GraphPrimitiveKind,
  GraphProjectionMode,
  GraphReductionDemand,
  GraphSymbolNamespace,
  TYPEINFO_GRAPH_SCHEMA_VERSION,
  TypeInfoGraphRequestSchema,
  TypeInfoGraphResponseSchema,
} from "@verter/proto";
import { describe, expect, it } from "vitest";

import {
  buildTypeInfoRequest,
  decodeTypeInfoResult,
  MAX_EXPANSION_DEPTH_BUDGET,
  MAX_EXPANSION_NODE_BUDGET,
} from "../src/graph.js";

describe("buildTypeInfoRequest", () => {
  it("builds the typed resolve-symbol envelope", () => {
    const request = buildTypeInfoRequest({
      canonicalId: "/types.ts",
      name: "Foo",
      mode: "expanded",
    });
    expect(request.schemaVersion).toBe(TYPEINFO_GRAPH_SCHEMA_VERSION);
    expect(request.operation).toBe(GraphOperation.RESOLVE_SYMBOL);
    const payload = request.payload;
    expect(payload.case).toBe("resolveSymbol");
    if (payload.case === "resolveSymbol") {
      expect(payload.value.canonicalId).toBe("/types.ts");
      expect(payload.value.name).toBe("Foo");
      expect(payload.value.context?.mode).toBe(GraphProjectionMode.EXPANDED);
      expect(payload.value.context?.demand).toBe(GraphReductionDemand.PUBLISHED);
      // The default closure is the bounded one-level policy.
      expect(payload.value.closure?.kind?.case).toBe("oneLevel");
    }
    // The envelope must be valid wire: it round-trips through the
    // generated schema.
    const bytes = toBinary(TypeInfoGraphRequestSchema, create(TypeInfoGraphRequestSchema, request));
    expect(bytes.length).toBeGreaterThan(0);
  });

  it("accepts an expanded closure with explicit in-range budgets", () => {
    const request = buildTypeInfoRequest({
      canonicalId: "/types.ts",
      name: "Foo",
      closure: { kind: "expanded", nodeBudget: 100, depthBudget: 8 },
    });
    const payload = request.payload;
    if (payload.case !== "resolveSymbol") throw new Error("resolve payload expected");
    expect(payload.value.closure?.kind?.case).toBe("expanded");
  });

  it("structurally rejects an expanded closure without budgets (unbounded export)", () => {
    expect(() =>
      buildTypeInfoRequest({
        canonicalId: "/types.ts",
        name: "Foo",
        // @ts-expect-error — the unbounded shape is intentionally invalid
        closure: { kind: "expanded" },
      }),
    ).toThrow(/budget/);
  });

  it("structurally rejects out-of-range budgets", () => {
    expect(() =>
      buildTypeInfoRequest({
        canonicalId: "/types.ts",
        name: "Foo",
        closure: { kind: "expanded", nodeBudget: MAX_EXPANSION_NODE_BUDGET + 1, depthBudget: 8 },
      }),
    ).toThrow(/budget/);
    expect(() =>
      buildTypeInfoRequest({
        canonicalId: "/types.ts",
        name: "Foo",
        closure: { kind: "expanded", nodeBudget: 8, depthBudget: MAX_EXPANSION_DEPTH_BUDGET + 1 },
      }),
    ).toThrow(/budget/);
  });

  it("refuses a query without a canonical or name", () => {
    expect(() => buildTypeInfoRequest({ canonicalId: "", name: "Foo" })).toThrow();
    expect(() => buildTypeInfoRequest({ canonicalId: "/types.ts", name: "" })).toThrow();
  });

  // @ai-generated - Pins rejection of provenance that this operation cannot populate.
  it("rejects unavailable provenance requests", () => {
    // Default: no provenance maps are requested.
    const off = buildTypeInfoRequest({ canonicalId: "/types.ts", name: "Foo" });
    const offPayload = off.payload;
    if (offPayload.case !== "resolveSymbol") throw new Error("resolve payload expected");
    expect(offPayload.value.includeProvenance).toBe(false);
    expect(() =>
      buildTypeInfoRequest({
        canonicalId: "/types.ts",
        name: "Foo",
        includeProvenance: true,
      }),
    ).toThrow(/provenance/i);
  });
});

/** Encode a `TypeInfoGraphResponse` carrying a `graph` arm. */
function encodeGraphResponse(graph: Record<string, unknown>): Uint8Array {
  const response = create(TypeInfoGraphResponseSchema, {
    kind: { case: "graph", value: graph },
  });
  return toBinary(TypeInfoGraphResponseSchema, response);
}

describe("decodeTypeInfoResult", () => {
  it("decodes the graph arm with order + interned-string identity preserved", () => {
    // The producer reserves string id 0 as the absent sentinel, so real
    // member names intern from id 1 — a 0-sentinel bug in either
    // direction (producer aliasing a real name to 0, or decode refusing
    // to resolve a table id) would collapse a name to "".
    const strings = ["", "b", "a", "msg"];
    const response = encodeGraphResponse({
      schemaVersion: TYPEINFO_GRAPH_SCHEMA_VERSION,
      strings: { entries: strings },
      // Node id 0 is the wire absent-sentinel; real nodes start at 1.
      nodes: [
        {},
        { kind: { case: "primitive", value: { kind: GraphPrimitiveKind.STRING } } },
        { kind: { case: "primitive", value: { kind: GraphPrimitiveKind.NUMBER } } },
        {
          kind: {
            case: "object",
            value: {
              members: [member(1, 1, false), member(2, 2, true), member(3, 1, false)],
              indexSignatures: [{ keyKind: 3, valueNodeId: 1, readonly: false }],
            },
          },
        },
      ],
      symbols: [],
      signatures: [],
      rootIds: [3],
    });

    const result = decodeTypeInfoResult(response);
    expect(result.kind).toBe("graph");
    if (result.kind !== "graph") throw new Error("graph arm expected");
    const root = result.root;
    expect(root.kind).toBe("object");
    if (root.kind !== "object") throw new Error("object root expected");
    expect(root.properties.map((p) => p.name)).toEqual(["b", "a", "msg"]);
    // Member order preserved; the optional flag rides each member.
    expect(root.properties[1].optional).toBe(true);
    expect(root.properties[0].optional).toBe(false);
    // Shared value node: the two `string` members share node id 1 —
    // identity preserved at the WIRE level (ONE node in the arena,
    // referenced twice), which `toEqual` on the projected descriptors
    // alone cannot discriminate from two duplicated equal descriptors.
    const members = root.properties;
    const wireMembers = result.graph.nodes[3]?.kind;
    if (wireMembers?.case !== "object") throw new Error("wire object root expected");
    expect(wireMembers.value.members[0].valueNodeId).toBe(1);
    expect(wireMembers.value.members[2].valueNodeId).toBe(1);
    expect(
      result.graph.nodes.filter(
        (n) => n.kind?.case === "primitive" && n.kind.value.kind === GraphPrimitiveKind.STRING,
      ),
    ).toHaveLength(1);
    expect(members[0].type).toEqual(members[2].type);
    expect(members[0].type.kind).toBe("primitive");
    // A template-pattern key domain is NOT a flat primitive — it decodes
    // to a named unknown, never a fabricated `string` domain.
    expect(root.indexSignatures?.[0]?.keyType.kind).toBe("unknown");
  });

  it("preserves the complete wire graph — identity, provenance, completeness", () => {
    // REGRESSION — the view must carry EVERY wire field the graph
    // declares: query identity (operation, context, provenance flags,
    // env hashes, resolver version), origin edges, per-node exactness,
    // diagnostics, both provenance id maps (with their stable decl-slot
    // identities and whole-hash bytes), and the relation-proof table.
    // A decode that drops any of them would silently strip identity or
    // provenance from the consumer surface.
    const wholeHash = new Uint8Array([9, 9, 9]);
    const solverOptionsHash = new Uint8Array([1, 2, 3]);
    const response = encodeGraphResponse({
      schemaVersion: TYPEINFO_GRAPH_SCHEMA_VERSION,
      query: {
        operation: GraphOperation.RESOLVE_SYMBOL,
        path: [],
        context: { mode: GraphProjectionMode.EXPANDED, demand: GraphReductionDemand.PUBLISHED },
        substitutions: [],
        solverOptionsHash,
        resolverVersion: 7,
        includeProvenance: true,
        includeDiagnostics: true,
        includeProjection: [],
        resolvedRoots: [],
      },
      strings: { entries: ["", "Foo", "/types.ts", "elided context"] },
      nodes: [
        {},
        { kind: { case: "primitive", value: { kind: GraphPrimitiveKind.STRING } } },
        { kind: { case: "reference", value: { symbolId: 0 } } },
      ],
      symbols: [
        { nameId: 1, canonicalNameId: 2, namespace: GraphSymbolNamespace.TYPE, declSlotRef: 0 },
      ],
      signatures: [],
      edges: [
        {
          sourceNodeId: 2,
          targetNodeId: 1,
          kind: GraphOriginEdgeKind.REFERENCES,
          metaNameId: 3,
          hasMeta: true,
        },
      ],
      rootIds: [2],
      exactness: [{ nodeId: 2, exactness: GraphExactness.EXACT_RESOLVED }],
      diagnostics: [
        {
          severity: GraphDiagnosticSeverity.INFO,
          messageNameId: 3,
          spanCanonicalNameId: 0,
          spanStart: 0,
          spanEnd: 0,
          hasSpan: false,
        },
      ],
      nodeIdMap: [
        {
          nodeId: 2,
          identity: {
            canonicalNameId: 2,
            declNameId: 1,
            wholeHash,
            namespace: GraphSymbolNamespace.TYPE,
          },
        },
      ],
      symbolIdMap: [
        {
          symbolId: 0,
          identity: {
            canonicalNameId: 2,
            declNameId: 1,
            wholeHash,
            namespace: GraphSymbolNamespace.TYPE,
          },
        },
      ],
      relationProofs: [{ kind: { case: "assignable", value: { subDerivations: [] } } }],
    });

    const result = decodeTypeInfoResult(response);
    expect(result.kind).toBe("graph");
    if (result.kind !== "graph") throw new Error("graph arm expected");
    const { graph } = result;

    // Query identity — echoed whole.
    expect(graph.query?.operation).toBe(GraphOperation.RESOLVE_SYMBOL);
    expect(graph.query?.context?.mode).toBe(GraphProjectionMode.EXPANDED);
    expect(graph.query?.context?.demand).toBe(GraphReductionDemand.PUBLISHED);
    expect(graph.query?.includeProvenance).toBe(true);
    expect(graph.query?.includeDiagnostics).toBe(true);
    expect(graph.query?.resolverVersion).toBe(7);
    expect(Array.from(graph.query?.solverOptionsHash ?? [])).toEqual([1, 2, 3]);

    // Completeness — edges, exactness, diagnostics survive with order.
    expect(graph.edges).toHaveLength(1);
    expect(graph.edges[0]?.sourceNodeId).toBe(2);
    expect(graph.edges[0]?.targetNodeId).toBe(1);
    expect(graph.edges[0]?.kind).toBe(GraphOriginEdgeKind.REFERENCES);
    expect(graph.edges[0]?.hasMeta).toBe(true);
    expect(graph.edges[0]?.metaNameId).toBe(3);
    expect(graph.exactness).toHaveLength(1);
    expect(graph.exactness[0]?.nodeId).toBe(2);
    expect(graph.exactness[0]?.exactness).toBe(GraphExactness.EXACT_RESOLVED);
    expect(graph.diagnostics).toHaveLength(1);
    expect(graph.diagnostics[0]?.severity).toBe(GraphDiagnosticSeverity.INFO);
    expect(graph.strings[graph.diagnostics[0]?.messageNameId ?? 0]).toBe("elided context");

    // Provenance — both id maps, stable identities, whole-hash bytes.
    expect(graph.nodeIdMap).toHaveLength(1);
    expect(graph.nodeIdMap[0]?.nodeId).toBe(2);
    expect(graph.nodeIdMap[0]?.identity?.declNameId).toBe(1);
    expect(Array.from(graph.nodeIdMap[0]?.identity?.wholeHash ?? [])).toEqual([9, 9, 9]);
    expect(graph.nodeIdMap[0]?.identity?.namespace).toBe(GraphSymbolNamespace.TYPE);
    expect(graph.symbolIdMap).toHaveLength(1);
    expect(graph.symbolIdMap[0]?.symbolId).toBe(0);
    expect(graph.symbolIdMap[0]?.identity?.canonicalNameId).toBe(2);

    // Relation proofs ride the payload-side table.
    expect(graph.relationProofs).toHaveLength(1);
    expect(graph.relationProofs[0]?.kind?.case).toBe("assignable");
  });

  it("decodes a reference root through the symbol table", () => {
    const response = encodeGraphResponse({
      schemaVersion: TYPEINFO_GRAPH_SCHEMA_VERSION,
      strings: { entries: ["IFoo"] },
      nodes: [{}, { kind: { case: "reference", value: { symbolId: 0 } } }],
      symbols: [{ nameId: 0, canonicalNameId: 0, namespace: 0, declSlotRef: 0 }],
      signatures: [],
      rootIds: [1],
    });
    const result = decodeTypeInfoResult(response);
    if (result.kind !== "graph") throw new Error("graph arm expected");
    expect(result.root.kind).toBe("ref");
    if (result.root.kind === "ref") {
      expect(result.root.name).toBe("IFoo");
    }
  });

  it("decodes the error arm as the TYPED wire error", () => {
    const response = toBinary(
      TypeInfoGraphResponseSchema,
      create(TypeInfoGraphResponseSchema, {
        kind: {
          case: "error",
          value: { kind: { case: "missingClosurePolicy", value: {} } },
        },
      }),
    );
    const result = decodeTypeInfoResult(response);
    expect(result.kind).toBe("error");
    if (result.kind === "error") {
      expect(result.error.case).toBe("missingClosurePolicy");
    }
  });

  it("decodes an alias instantiation root with type arguments", () => {
    const response = encodeGraphResponse({
      schemaVersion: TYPEINFO_GRAPH_SCHEMA_VERSION,
      strings: { entries: ["Partial", "T"] },
      nodes: [
        {},
        { kind: { case: "primitive", value: { kind: GraphPrimitiveKind.STRING } } },
        { kind: { case: "typeParameter", value: { symbolId: 0, nameId: 1, paramIndex: 0 } } },
        {
          kind: {
            case: "aliasInstantiation",
            value: { aliasSymbolId: 0, typeArgumentNodeIds: [1] },
          },
        },
      ],
      symbols: [{ nameId: 0, canonicalNameId: 0, namespace: 0, declSlotRef: 0 }],
      signatures: [],
      rootIds: [3],
    });
    const result = decodeTypeInfoResult(response);
    if (result.kind !== "graph") throw new Error("graph arm expected");
    expect(result.root.kind).toBe("ref");
    if (result.root.kind === "ref") {
      expect(result.root.name).toBe("Partial");
      expect(result.root.typeArguments).toHaveLength(1);
      expect(result.root.typeArguments[0].kind).toBe("primitive");
    }
  });

  it("decodes an opaque budget marker as an unknown descriptor (fail-closed)", () => {
    const response = encodeGraphResponse({
      schemaVersion: TYPEINFO_GRAPH_SCHEMA_VERSION,
      strings: { entries: [] },
      nodes: [
        {},
        {
          kind: {
            case: "opaque",
            value: {
              error: {
                kind: {
                  case: "budgetExceeded",
                  value: { domain: 2, limit: 2, actual: 0, contextNameId: 0 },
                },
              },
            },
          },
        },
      ],
      symbols: [],
      signatures: [],
      rootIds: [1],
    });
    const result = decodeTypeInfoResult(response);
    if (result.kind !== "graph") throw new Error("graph arm expected");
    expect(result.root.kind).toBe("unknown");
  });

  it("resolves signature names through the table; 0 is the absent sentinel", () => {
    // A named parameter resolves through the table; an unnamed parameter
    // (nameId 0, the reserved sentinel) falls back to the positional
    // spelling; a missing return annotation (node id 0) is absent.
    const response = encodeGraphResponse({
      schemaVersion: TYPEINFO_GRAPH_SCHEMA_VERSION,
      strings: { entries: ["", "x"] },
      nodes: [
        {},
        { kind: { case: "primitive", value: { kind: GraphPrimitiveKind.NUMBER } } },
        {
          kind: {
            case: "object",
            value: { callSignatureRefs: [0] },
          },
        },
      ],
      symbols: [],
      signatures: [
        {
          typeParameterNodeIds: [],
          parameters: [
            { nameId: 1, typeNodeId: 1, optional: false, rest: false, inferencePolicy: 0 },
            { nameId: 0, typeNodeId: 1, optional: false, rest: false, inferencePolicy: 0 },
          ],
          returnTypeNodeId: 0,
          overloadIndex: 0,
          isConstruct: false,
          isImplementation: false,
          isAbstract: false,
          flags: 0,
        },
      ],
      rootIds: [2],
    });
    const result = decodeTypeInfoResult(response);
    if (result.kind !== "graph") throw new Error("graph arm expected");
    expect(result.root.kind).toBe("object");
    if (result.root.kind !== "object") throw new Error("object root expected");
    const signature = result.root.callSignatures?.[0];
    expect(signature?.parameters.map((p) => p.name)).toEqual(["x", "arg1"]);
    expect(signature?.parameters[0].type.kind).toBe("primitive");
    expect(signature?.returnType.kind).toBe("primitive");
    if (signature?.returnType.kind !== "primitive") throw new Error("primitive expected");
    expect(signature?.returnType.name).toBe("void");
  });

  it("terminates on a cyclic method-value signature (guarded walk)", () => {
    // Node 1 (object) has a method whose value node 2 (callable object)
    // carries a signature whose parameter type points BACK at node 1.
    // The signature walk must continue under the same visited set —
    // unguarded, this graph recurses without bound.
    const response = encodeGraphResponse({
      schemaVersion: TYPEINFO_GRAPH_SCHEMA_VERSION,
      strings: { entries: ["", "m"] },
      nodes: [
        {},
        {
          kind: {
            case: "object",
            value: { members: [methodMember(1, 2)] },
          },
        },
        {
          kind: {
            case: "object",
            value: { callSignatureRefs: [0] },
          },
        },
      ],
      symbols: [],
      signatures: [
        {
          typeParameterNodeIds: [],
          parameters: [
            { nameId: 0, typeNodeId: 1, optional: false, rest: false, inferencePolicy: 0 },
          ],
          returnTypeNodeId: 0,
          overloadIndex: 0,
          isConstruct: false,
          isImplementation: false,
          isAbstract: false,
          flags: 0,
        },
      ],
      rootIds: [1],
    });
    const result = decodeTypeInfoResult(response);
    if (result.kind !== "graph") throw new Error("graph arm expected");
    expect(result.root.kind).toBe("object");
    if (result.root.kind !== "object") throw new Error("object root expected");
    const method = result.root.properties[0]?.type;
    expect(method?.kind).toBe("function");
    if (method?.kind !== "function") throw new Error("method decodes to a function");
    expect(method.parameters[0].type.kind).toBe("unknown");
  });

  // @ai-generated - Pins one shared monotonic decode budget across callable walks.
  it("bounds repeated DAG decode work across callable signatures", () => {
    const methodCount = 9_000;
    const response = encodeGraphResponse({
      schemaVersion: TYPEINFO_GRAPH_SCHEMA_VERSION,
      strings: { entries: ["", "m"] },
      nodes: [
        {},
        {
          kind: {
            case: "object",
            value: {
              members: Array.from({ length: methodCount }, () => methodMember(1, 2)),
            },
          },
        },
        { kind: { case: "object", value: { callSignatureRefs: [0] } } },
        { kind: { case: "primitive", value: { kind: GraphPrimitiveKind.STRING } } },
      ],
      symbols: [],
      signatures: [
        {
          typeParameterNodeIds: [],
          parameters: [
            { nameId: 0, typeNodeId: 3, optional: false, rest: false, inferencePolicy: 0 },
          ],
          returnTypeNodeId: 3,
          overloadIndex: 0,
          isConstruct: false,
          isImplementation: false,
          isAbstract: false,
          flags: 0,
        },
      ],
      rootIds: [1],
    });
    const result = decodeTypeInfoResult(response);
    if (result.kind !== "graph") throw new Error("graph arm expected");
    if (result.root.kind !== "object") throw new Error("object root expected");
    expect(
      result.root.properties.some(
        (property) =>
          property.type.kind === "unknown" && property.type.rawType === "decode budget exceeded",
      ),
    ).toBe(true);
  });

  it("decodes an object spread program as a named shell, not a closed member list", () => {
    // The program's spread operands and bare call/construct effects have
    // no flat member form; publishing only the direct properties would
    // present a fabricated closed surface. The wire view stays the
    // authority; the descriptor says what it is.
    const response = encodeGraphResponse({
      schemaVersion: TYPEINFO_GRAPH_SCHEMA_VERSION,
      strings: { entries: [""] },
      nodes: [
        {},
        {
          kind: {
            case: "objectSpreadProgram",
            value: {
              effects: [
                { kind: { case: "spread", value: { operandNodeId: 0 } } },
                {
                  kind: {
                    case: "directProperty",
                    value: { propertyKey: { key: { case: "stringId", value: 0 } }, valueNodeId: 0 },
                  },
                },
              ],
            },
          },
        },
      ],
      symbols: [],
      signatures: [],
      rootIds: [1],
    });
    const result = decodeTypeInfoResult(response);
    if (result.kind !== "graph") throw new Error("graph arm expected");
    expect(result.root.kind).toBe("unknown");
    if (result.root.kind !== "unknown") throw new Error("unknown shell expected");
    expect(result.root.rawType).toBe("objectSpreadProgram");
    // The wire view remains available to consumers that need the program.
    expect(result.graph.nodes[1]?.kind?.case).toBe("objectSpreadProgram");
  });
});

function member(nameId: number, valueNodeId: number, optional: boolean): Record<string, unknown> {
  return {
    valueNodeId,
    optional,
    readonly: false,
    accessibility: 0,
    staticSide: false,
    declarationSymbolId: 0,
    propertyKey: { key: { case: "stringId", value: nameId } },
    memberKind: GraphObjectMemberKind.PROPERTY,
    hasImplementationBody: false,
  };
}

function methodMember(nameId: number, valueNodeId: number): Record<string, unknown> {
  return {
    ...member(nameId, valueNodeId, false),
    memberKind: GraphObjectMemberKind.METHOD,
  };
}
