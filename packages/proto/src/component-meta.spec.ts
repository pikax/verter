/**
 * @ai-generated - Verifies the typed component-meta proto schema can round-trip a recursive graph payload.
 */

import { create, fromBinary, toBinary } from "@bufbuild/protobuf";
import { describe, expect, it } from "vitest";

import { ComponentMetaPayloadSchema, createTestComponentMetaPayload } from "./component-meta.js";
import {
  ComponentMetaBodySchema,
  MacroExpansionDiagnosticEntrySchema,
  ExpansionExactness,
  ExpansionExecutionStatus,
  ExpansionStopReason,
  OriginGraphSchema,
} from "./gen/verter/v1/component_meta_pb.js";

describe("ComponentMetaPayloadSchema", () => {
  it("round-trips a recursive graph payload", () => {
    const payload = create(ComponentMetaPayloadSchema, createTestComponentMetaPayload());
    const bytes = toBinary(ComponentMetaPayloadSchema, payload);
    const decoded = fromBinary(ComponentMetaPayloadSchema, bytes);
    const graph = decoded.typeGraph;
    const body = decoded.body;

    expect(graph?.strings[(body?.filePathId ?? 0) - 1]).toBe("/src/Tree.vue");
    expect(decoded.typeRegistry).toHaveLength(1);
    expect(graph?.strings[(body?.props[0]?.nameId ?? 0) - 1]).toBe("root");
    expect(graph?.nodes.length).toBeGreaterThan(0);
    expect(body?.slots).toHaveLength(1);
    expect(graph?.nodes[2]?.kind.case).toBe("recursiveRef");
    if (graph?.nodes[2]?.kind.case === "recursiveRef") {
      expect(graph.nodes[2].kind.value.typeArgumentNodeIds).toEqual([1]);
      expect(graph.nodes[2].kind.value.conditionalContext).toHaveLength(1);
    }
  });

  it("round-trips MacroExpansionDiagnosticEntry and is backward-compatible without it", () => {
    // With macroExpansionDiagnostics present
    const base = createTestComponentMetaPayload();
    const bodyWithDiags = create(ComponentMetaBodySchema, {
      ...base.body,
      macroExpansionDiagnostics: [
        create(MacroExpansionDiagnosticEntrySchema, {
          macroKindId: 1,
          macroIndex: 0,
          exactness: ExpansionExactness.EXACT_CONCRETE,
          executionStatus: ExpansionExecutionStatus.COMPLETED,
          diagnostics: [
            {
              reason: ExpansionStopReason.BUDGET_EXCEEDED,
              contextId: 1,
              propertyNameId: 0,
            },
          ],
        }),
      ],
    });
    const withDiags = create(ComponentMetaPayloadSchema, {
      ...base,
      body: bodyWithDiags,
    });
    const bytesWithDiags = toBinary(ComponentMetaPayloadSchema, withDiags);
    const decodedWithDiags = fromBinary(ComponentMetaPayloadSchema, bytesWithDiags);
    expect(decodedWithDiags.body?.macroExpansionDiagnostics).toHaveLength(1);
    expect(decodedWithDiags.body?.macroExpansionDiagnostics[0]?.exactness).toBe(
      ExpansionExactness.EXACT_CONCRETE,
    );
    expect(decodedWithDiags.body?.macroExpansionDiagnostics[0]?.executionStatus).toBe(
      ExpansionExecutionStatus.COMPLETED,
    );
    expect(decodedWithDiags.body?.macroExpansionDiagnostics[0]?.diagnostics).toHaveLength(1);

    // Without macroExpansionDiagnostics — backward compat (defaults to empty)
    const withoutDiags = create(ComponentMetaPayloadSchema, createTestComponentMetaPayload());
    const bytesWithout = toBinary(ComponentMetaPayloadSchema, withoutDiags);
    const decodedWithout = fromBinary(ComponentMetaPayloadSchema, bytesWithout);
    expect(decodedWithout.body?.macroExpansionDiagnostics).toHaveLength(0);
  });

  it("round-trips origin graph with nodes, edges, and meta strings", () => {
    const base = createTestComponentMetaPayload();
    const originGraph = create(OriginGraphSchema, {
      nodes: [
        { id: 0, kindId: 0, labelId: 1 },
        { id: 1, kindId: 2, labelId: 3 },
      ],
      edges: [
        { source: 0, target: 1, kindId: 4, metaIndex: 0, hasMeta: false },
        { source: 1, target: 0, kindId: 5, metaIndex: 0, hasMeta: true },
      ],
      metaStrings: [
        'SubstitutedParam("T")',
        "Object",
        "{...}",
        "Primitive",
        "string",
        "instantiate",
        "substituteTypeParam",
      ],
    });
    const withOrigin = create(ComponentMetaPayloadSchema, {
      ...base,
      originGraph,
    });
    const bytes = toBinary(ComponentMetaPayloadSchema, withOrigin);
    const decoded = fromBinary(ComponentMetaPayloadSchema, bytes);

    expect(decoded.originGraph).toBeDefined();
    expect(decoded.originGraph!.nodes).toHaveLength(2);
    expect(decoded.originGraph!.edges).toHaveLength(2);
    expect(decoded.originGraph!.metaStrings).toContain("instantiate");
    expect(decoded.originGraph!.metaStrings).toContain("substituteTypeParam");
    expect(decoded.originGraph!.metaStrings).toContain("Object");

    const edge0 = decoded.originGraph!.edges[0];
    expect(edge0.hasMeta).toBe(false);
    const edge1 = decoded.originGraph!.edges[1];
    expect(edge1.hasMeta).toBe(true);
    expect(decoded.originGraph!.metaStrings[edge1.metaIndex]).toBe('SubstitutedParam("T")');
  });

  it("omits origin graph when not provided (backward compat)", () => {
    const base = createTestComponentMetaPayload();
    const payload = create(ComponentMetaPayloadSchema, base);
    const bytes = toBinary(ComponentMetaPayloadSchema, payload);
    const decoded = fromBinary(ComponentMetaPayloadSchema, bytes);
    expect(decoded.originGraph).toBeUndefined();
  });
});
