/**
 * @ai-generated - Verifies component-meta graph payload decode, hard-failure behavior, and graph-backed descriptor conversion.
 */

import { create, toBinary } from "@bufbuild/protobuf";
import { ComponentMetaPayloadSchema } from "@verter/proto";
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
});
