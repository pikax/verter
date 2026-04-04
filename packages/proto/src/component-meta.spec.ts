/**
 * @ai-generated - Verifies the typed component-meta proto schema can round-trip a recursive graph payload.
 */

import { create, fromBinary, toBinary } from "@bufbuild/protobuf";
import { describe, expect, it } from "vitest";

import { ComponentMetaPayloadSchema, createTestComponentMetaPayload } from "./component-meta.js";
import {
  MacroExpansionDiagnosticEntrySchema,
  ExpansionExactness,
  ExpansionExecutionStatus,
  ExpansionStopReason,
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
    const withDiags = create(ComponentMetaPayloadSchema, {
      ...base,
      body: {
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
      },
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
});
