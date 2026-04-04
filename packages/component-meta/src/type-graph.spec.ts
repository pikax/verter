/**
 * @ai-generated - Verifies component-meta graph payload decode, hard-failure behavior, and graph-backed descriptor conversion.
 */

import { create, toBinary } from "@bufbuild/protobuf";
import { ComponentMetaPayloadSchema } from "@verter/proto";
import {
  ExpansionExactness,
  ExpansionExecutionStatus,
  ExpansionStopReason,
  MacroExpansionDiagnosticEntrySchema,
} from "../../proto/src/gen/verter/v1/component_meta_pb.js";
import { describe, expect, it } from "vitest";

import {
  nativeComponentMetaToComponentMeta,
  nativeTypeRegistryToMap,
} from "./native-component-meta.js";
import {
  buildTestComponentMetaProtoPayload,
  encodeTestComponentMetaPayload,
} from "./type-graph.test-utils.js";
import { decodeComponentMetaPayload, isGraphTypeExprRef } from "./type-graph.js";
import { typeExprToDescriptor } from "./type-expr-bridge.js";

describe("decodeComponentMetaPayload", () => {
  it("decodes graph-backed payloads and keeps slot returnType data", () => {
    const payload = encodeTestComponentMetaPayload({
      filePath: "/project/src/Button.vue",
      props: [
        {
          name: "item",
          type: { kind: "ref", name: "Item" },
          rawType: "Item",
          required: true,
        },
      ],
      slots: [
        {
          name: "default",
          isScoped: true,
          returnType: "VNode[]",
          bindings: [{ name: "item", type: { kind: "ref", name: "Item" } }],
        },
      ],
      typeRegistry: [
        {
          name: "Item",
          type: {
            kind: "object",
            properties: [{ name: "label", type: { kind: "primitive", name: "string" } }],
          },
        },
      ],
    });

    const native = decodeComponentMetaPayload(payload);

    expect(isGraphTypeExprRef(native.props[0]?.type)).toBe(true);
    expect(isGraphTypeExprRef(native.slots[0]?.bindings[0]?.type)).toBe(true);

    const registry = nativeTypeRegistryToMap(native);
    expect(registry?.get("Item")).toEqual({
      kind: "object",
      properties: [{ name: "label", type: { kind: "primitive", name: "string" }, optional: false }],
    });

    const compat = nativeComponentMetaToComponentMeta(native);
    expect(compat.props[0]?.type).toEqual({ kind: "ref", name: "Item" });
    expect(compat.slots[0]?.returnType).toBe("VNode[]");
  });

  it("converts graph roots to descriptors through the bridge", () => {
    const payload = encodeTestComponentMetaPayload({
      filePath: "/project/src/Tree.vue",
      typeRegistry: [
        {
          name: "TreeNode",
          type: {
            kind: "object",
            properties: [
              { name: "label", type: { kind: "primitive", name: "string" } },
              {
                name: "next",
                type: {
                  kind: "union",
                  types: [
                    { kind: "ref", name: "TreeNode" },
                    { kind: "primitive", name: "undefined" },
                  ],
                },
                optional: true,
              },
            ],
          },
        },
      ],
      props: [{ name: "root", type: { kind: "ref", name: "TreeNode" } }],
    });

    const native = decodeComponentMetaPayload(payload);
    const root = native.props[0]?.type;
    const registry = new Map((native.typeRegistry ?? []).map((entry) => [entry.name, entry.type]));

    expect(typeExprToDescriptor(root!, registry)).toEqual({ kind: "ref", name: "TreeNode" });
    expect(nativeTypeRegistryToMap(native)?.get("TreeNode")).toEqual({
      kind: "object",
      properties: [
        { name: "label", type: { kind: "primitive", name: "string" }, optional: false },
        {
          name: "next",
          type: {
            kind: "union",
            types: [
              { kind: "ref", name: "TreeNode" },
              { kind: "primitive", name: "undefined" },
            ],
          },
          optional: true,
        },
      ],
    });
  });

  it("preserves recursiveRef through proto decode and bridge", () => {
    const childrenType: import("./type-graph.test-utils.js").TestTypeExpr = {
      kind: "recursiveRef",
      name: "TreeNode",
      typeArguments: [{ kind: "primitive", name: "string" }],
      conditionalContext: [
        {
          branch: "true" as const,
          decided: true,
          check: { kind: "primitive" as const, name: "string" as const },
          extends: { kind: "primitive" as const, name: "number" as const },
        },
      ],
    };
    const payload = encodeTestComponentMetaPayload({
      filePath: "/project/src/Recursive.vue",
      typeRegistry: [
        {
          name: "TreeNode",
          type: {
            kind: "object",
            properties: [
              { name: "label", type: { kind: "primitive", name: "string" } },
              { name: "children", type: childrenType },
            ],
          },
        },
      ],
      props: [{ name: "root", type: { kind: "ref", name: "TreeNode" } }],
    });

    const native = decodeComponentMetaPayload(payload);
    const registry = nativeTypeRegistryToMap(native);
    const treeNode = registry?.get("TreeNode");

    expect(treeNode).toBeDefined();
    expect(treeNode!.kind).toBe("object");

    if (treeNode!.kind === "object") {
      const children = treeNode!.properties.find((p) => p.name === "children");
      expect(children).toBeDefined();

      // Must be recursiveRef, NOT unknown
      expect(children!.type.kind).toBe("recursiveRef");
      expect(children!.type.kind).not.toBe("unknown");

      if (children!.type.kind === "recursiveRef") {
        expect(children!.type.name).toBe("TreeNode");
        expect(children!.type.typeArguments).toHaveLength(1);
        expect(children!.type.typeArguments[0]).toEqual({
          kind: "primitive",
          name: "string",
        });
        expect(children!.type.conditionalContext).toHaveLength(1);
        expect(children!.type.conditionalContext[0]!.branch).toBe("true");
        expect(children!.type.conditionalContext[0]!.decided).toBe(true);
        expect(children!.type.conditionalContext[0]!.check).toEqual({
          kind: "primitive",
          name: "string",
        });
        expect(children!.type.conditionalContext[0]!.extends).toEqual({
          kind: "primitive",
          name: "number",
        });
      }
    }
  });

  it("decodes expansion exactness and execution status from graph metadata", () => {
    const payload = encodeTestComponentMetaPayload({
      filePath: "/project/src/Button.vue",
      props: [
        {
          name: "item",
          type: { kind: "ref", name: "Item" },
          typeExpansion: {
            exactness: "exactSymbolic",
            executionStatus: "completed",
            diagnostics: [
              {
                reason: "mappedDepthExceeded",
                context: "mapped type stayed symbolic",
              },
            ],
          },
        },
      ],
      slots: [
        {
          name: "default",
          isScoped: true,
          bindings: [
            {
              name: "item",
              type: { kind: "ref", name: "Item" },
              typeExpansion: {
                exactness: "incomplete",
                executionStatus: "cancelled",
                diagnostics: [
                  {
                    reason: "budgetExceeded",
                    context: "work budget exceeded",
                  },
                ],
              },
            },
          ],
        },
      ],
      typeRegistry: [
        {
          name: "Item",
          type: {
            kind: "object",
            properties: [{ name: "label", type: { kind: "primitive", name: "string" } }],
          },
        },
      ],
    });

    const native = decodeComponentMetaPayload(payload);

    expect(native.props[0]?.typeExpansion).toEqual({
      exactness: "exactSymbolic",
      executionStatus: "completed",
      diagnostics: [
        {
          reason: "mappedDepthExceeded",
          context: "mapped type stayed symbolic",
        },
      ],
    });
    expect(native.slots[0]?.bindings[0]?.typeExpansion).toEqual({
      exactness: "incomplete",
      executionStatus: "cancelled",
      diagnostics: [
        {
          reason: "budgetExceeded",
          context: "work budget exceeded",
        },
      ],
    });
  });

  it("decodes conditionalContextTruncated diagnostics and keeps enum numbering stable", () => {
    expect(ExpansionStopReason.UNSUPPORTED_OPERATOR).toBe(6);
    expect(ExpansionStopReason.CONDITIONAL_CONTEXT_TRUNCATED).toBe(7);

    const payload = encodeTestComponentMetaPayload({
      filePath: "/project/src/Button.vue",
      props: [
        {
          name: "item",
          type: { kind: "ref", name: "Item" },
          typeExpansion: {
            exactness: "exactConcrete",
            executionStatus: "completed",
            diagnostics: [
              {
                reason: "conditionalContextTruncated",
                context: "12 available, 8 captured",
              },
            ],
          },
        },
      ],
      typeRegistry: [
        {
          name: "Item",
          type: {
            kind: "object",
            properties: [{ name: "label", type: { kind: "primitive", name: "string" } }],
          },
        },
      ],
    });

    const native = decodeComponentMetaPayload(payload);

    expect(native.props[0]?.typeExpansion).toEqual({
      exactness: "exactConcrete",
      executionStatus: "completed",
      diagnostics: [
        {
          reason: "conditionalContextTruncated",
          context: "12 available, 8 captured",
        },
      ],
    });
  });

  it("does not leak descriptor memoization across payload instances", () => {
    const stringPayload = encodeTestComponentMetaPayload({
      filePath: "/project/src/Node.vue",
      typeRegistry: [
        {
          name: "Node",
          type: {
            kind: "object",
            properties: [{ name: "value", type: { kind: "primitive", name: "string" } }],
          },
        },
      ],
    });
    const numberPayload = encodeTestComponentMetaPayload({
      filePath: "/project/src/Node.vue",
      typeRegistry: [
        {
          name: "Node",
          type: {
            kind: "object",
            properties: [{ name: "value", type: { kind: "primitive", name: "number" } }],
          },
        },
      ],
    });

    const first = nativeTypeRegistryToMap(decodeComponentMetaPayload(stringPayload));
    const second = nativeTypeRegistryToMap(decodeComponentMetaPayload(numberPayload));

    expect(first?.get("Node")).toEqual({
      kind: "object",
      properties: [{ name: "value", type: { kind: "primitive", name: "string" }, optional: false }],
    });
    expect(second?.get("Node")).toEqual({
      kind: "object",
      properties: [{ name: "value", type: { kind: "primitive", name: "number" }, optional: false }],
    });
  });

  it("does not reuse no-registry graph descriptors after a registry-backed conversion", () => {
    const payload = buildTestComponentMetaProtoPayload({
      filePath: "/project/src/Button.vue",
      props: [
        {
          name: "label",
          type: {
            kind: "indexedAccess",
            object: { kind: "ref", name: "Fields" },
            index: { kind: "literal", literalKind: "string", value: "label" },
          },
          rawType: 'Fields["label"]',
          required: true,
        },
      ],
      typeRegistry: [
        {
          name: "Fields",
          type: {
            kind: "object",
            properties: [{ name: "label", type: { kind: "primitive", name: "string" } }],
          },
        },
      ],
    });

    const native = decodeComponentMetaPayload(
      toBinary(ComponentMetaPayloadSchema, create(ComponentMetaPayloadSchema, payload)),
    );
    const propType = native.props[0]!.type;
    const registry = new Map((native.typeRegistry ?? []).map((entry) => [entry.name, entry.type]));

    expect(typeExprToDescriptor(propType)).toEqual({
      kind: "unknown",
      rawType: "graphNode(13)",
    });
    expect(typeExprToDescriptor(propType, registry)).toEqual({ kind: "primitive", name: "string" });
  });

  it("hard-fails on version mismatches and bad node ids", () => {
    const payload = encodeTestComponentMetaPayload({
      filePath: "/project/src/Button.vue",
      props: [{ name: "label", type: { kind: "primitive", name: "string" } }],
    });

    const wrongVersion = Buffer.from(payload);
    wrongVersion[1] = 99;
    expect(() => decodeComponentMetaPayload(wrongVersion)).toThrow(/version/i);

    const badNodePayload = buildTestComponentMetaProtoPayload({
      filePath: "/project/src/Button.vue",
      props: [{ name: "label", type: { kind: "primitive", name: "string" } }],
    });
    badNodePayload.body!.props[0]!.typeNodeId = 999;

    expect(() =>
      decodeComponentMetaPayload(
        toBinary(ComponentMetaPayloadSchema, create(ComponentMetaPayloadSchema, badNodePayload)),
      ),
    ).toThrow(/node id/i);
  });

  it("decodes macroExpansionDiagnostics correctly from protobuf", () => {
    const base = buildTestComponentMetaProtoPayload({
      filePath: "/project/src/Button.vue",
      props: [{ name: "label", type: { kind: "primitive", name: "string" } }],
      typeRegistry: [
        {
          name: "Item",
          type: {
            kind: "object",
            properties: [{ name: "value", type: { kind: "primitive", name: "string" } }],
          },
        },
      ],
    });

    // Inject macroExpansionDiagnostics with one entry using graph string ids
    // "defineProps" needs a string id — add it to the string table
    const strings = base.typeGraph!.strings as string[];
    const definePropsId = strings.length + 1;
    strings.push("defineProps");
    const budgetContextId = strings.length + 1;
    strings.push("work budget exceeded");

    (base.body as Record<string, unknown>).macroExpansionDiagnostics = [
      create(MacroExpansionDiagnosticEntrySchema, {
        macroKindId: definePropsId,
        macroIndex: 0,
        exactness: ExpansionExactness.EXACT_SYMBOLIC,
        executionStatus: ExpansionExecutionStatus.COMPLETED,
        diagnostics: [
          {
            reason: ExpansionStopReason.BUDGET_EXCEEDED,
            contextId: budgetContextId,
            propertyNameId: 0,
          },
        ],
      }),
    ];

    const bytes = toBinary(ComponentMetaPayloadSchema, create(ComponentMetaPayloadSchema, base));
    const result = decodeComponentMetaPayload(bytes);

    expect(result.macroExpansionDiagnostics).toBeDefined();
    expect(result.macroExpansionDiagnostics).toHaveLength(1);

    const entry = result.macroExpansionDiagnostics![0]!;
    expect(entry.macroKind).toBe("defineProps");
    expect(entry.macroIndex).toBe(0);
    expect(entry.exactness).toBe("exactSymbolic");
    expect(entry.executionStatus).toBe("completed");
    expect(entry.diagnostics).toHaveLength(1);
    expect(entry.diagnostics[0]!.reason).toBe("budgetExceeded");
    expect(entry.diagnostics[0]!.context).toBe("work budget exceeded");
  });
});
