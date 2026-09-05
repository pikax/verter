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
  GraphObjectMemberKind,
  GraphOperation,
  GraphPrimitiveKind,
  GraphProjectionMode,
  GraphReductionDemand,
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
    // String id 0 is deliberately a REAL member name — the graph table
    // is zero-based, so a 0-sentinel bug would resolve it to "".
    const strings = ["b", "a", "msg"];
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
              members: [member(0, 1, false), member(1, 2, true), member(2, 1, false)],
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
    // identity preserved (one node, referenced twice).
    expect(root.properties[0].type).toEqual(root.properties[2].type);
    expect(root.properties[0].type.kind).toBe("primitive");
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
