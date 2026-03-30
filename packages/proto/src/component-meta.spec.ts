/**
 * @ai-generated - Verifies the typed component-meta proto schema can round-trip a recursive graph payload.
 */

import { create, fromBinary, toBinary } from "@bufbuild/protobuf";
import { describe, expect, it } from "vitest";

import { ComponentMetaPayloadSchema, createTestComponentMetaPayload } from "./component-meta.js";

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
  });
});
