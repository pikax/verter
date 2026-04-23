import { create, toBinary } from "@bufbuild/protobuf";
import { describe, expect, it } from "vitest";
import {
  ComponentMetaPayloadSchema,
  createTestComponentMetaPayload,
  OriginGraphSchema,
} from "@verter/proto";
import { decodeTypedComponentMetaPayload } from "./type-graph-proto-decode.js";

describe("decodeTypedComponentMetaPayload", () => {
  it("decodes origin graph from protobuf binary", () => {
    const base = createTestComponentMetaPayload();
    const originGraph = create(OriginGraphSchema, {
      nodes: [
        { id: 0, kindId: 0, labelId: 1 },
        { id: 1, kindId: 2, labelId: 0 },
      ],
      edges: [{ source: 1, target: 0, kindId: 3, metaIndex: 0, hasMeta: false }],
      metaStrings: ["Object", "{...}", "Primitive", "instantiate"],
    });
    const payload = create(ComponentMetaPayloadSchema, { ...base, originGraph });
    const bytes = toBinary(ComponentMetaPayloadSchema, payload);

    const result = decodeTypedComponentMetaPayload(bytes);

    expect(result.origin).toBeDefined();
    expect(result.origin!.nodes).toHaveLength(2);
    expect(result.origin!.edges).toHaveLength(1);
    expect(result.origin!.nodes[0].kind).toBe("Object");
    expect(result.origin!.nodes[0].label).toBe("{...}");
    expect(result.origin!.nodes[1].kind).toBe("Primitive");
    expect(result.origin!.nodes[1].label).toBeUndefined();
    expect(result.origin!.edges[0].kind).toBe("instantiate");
    expect(result.origin!.edges[0].source).toBe(1);
    expect(result.origin!.edges[0].target).toBe(0);
    expect(result.origin!.edges[0].metaIndex).toBeUndefined();
  });

  it("returns undefined origin when originGraph is absent", () => {
    const base = createTestComponentMetaPayload();
    const payload = create(ComponentMetaPayloadSchema, base);
    const bytes = toBinary(ComponentMetaPayloadSchema, payload);

    const result = decodeTypedComponentMetaPayload(bytes);
    expect(result.origin).toBeUndefined();
  });

  it("decodes edge meta index when hasMeta is true", () => {
    const base = createTestComponentMetaPayload();
    const originGraph = create(OriginGraphSchema, {
      nodes: [
        { id: 0, kindId: 1, labelId: 0 },
        { id: 1, kindId: 1, labelId: 0 },
      ],
      edges: [{ source: 0, target: 1, kindId: 2, metaIndex: 0, hasMeta: true }],
      metaStrings: ['SubstitutedParam("T")', "Object", "substituteTypeParam"],
    });
    const payload = create(ComponentMetaPayloadSchema, { ...base, originGraph });
    const bytes = toBinary(ComponentMetaPayloadSchema, payload);

    const result = decodeTypedComponentMetaPayload(bytes);
    expect(result.origin!.edges[0].metaIndex).toBe(0);
    expect(result.origin!.metaStrings[0]).toBe('SubstitutedParam("T")');
  });
});
