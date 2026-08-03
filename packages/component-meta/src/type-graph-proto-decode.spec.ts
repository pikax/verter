import { create, toBinary } from "@bufbuild/protobuf";
import { describe, expect, it } from "vitest";
import {
  ComponentMetaPayloadSchema,
  createTestComponentMetaPayload,
  OriginGraphSchema,
} from "@verter/proto";
import { decodeTypedComponentMetaPayload } from "./type-graph-proto-decode.js";

describe("decodeTypedComponentMetaPayload", () => {
  it("accepts the current schema version and rejects an older response", () => {
    const current = createTestComponentMetaPayload();
    expect(current.schemaVersion).toBe(7);
    expect(() =>
      decodeTypedComponentMetaPayload(
        toBinary(
          ComponentMetaPayloadSchema,
          create(ComponentMetaPayloadSchema, { ...current, schemaVersion: 6 }),
        ),
      ),
    ).toThrow(/expected 7, found 6/);
  });

  it("decodes supported contract type references from the shared graph", () => {
    const current = createTestComponentMetaPayload();
    const result = decodeTypedComponentMetaPayload(
      toBinary(ComponentMetaPayloadSchema, create(ComponentMetaPayloadSchema, current)),
    );
    expect(result.componentPublicContract.kind).toBe("supported");
    if (result.componentPublicContract.kind !== "supported") return;
    expect(result.componentPublicContract.contract.adapterId).toBe("vue");
    expect(result.componentPublicContract.contract.props[0]?.name).toBe("root");
    expect(result.componentPublicContract.contract.props[0]?.type.type).toBeDefined();
  });

  it.each([
    [1, "unrepresentableRequiredMemberValue"],
    [2, "unrepresentableRequiredPayload"],
  ] as const)(
    "decodes required-source output failure with publication subreason %s",
    (publicationFailure, expected) => {
      const base = createTestComponentMetaPayload();
      const current = base.body!.componentPublicContract!.availability;
      if (current.case !== "supported") throw new Error("fixture contract must be supported");
      const payload = create(ComponentMetaPayloadSchema, {
        ...base,
        body: {
          ...base.body!,
          componentPublicContract: {
            availability: {
              case: "unsupported",
              value: {
                adapterId: current.value.adapterId,
                reason: 3,
                outputLane: 13,
                index: 2,
                outputFailure: 2,
                publicationFailure,
                diagnostics: [],
              },
            },
          },
        },
      });

      const result = decodeTypedComponentMetaPayload(toBinary(ComponentMetaPayloadSchema, payload));

      expect(result.componentPublicContract).toMatchObject({
        kind: "unsupported",
        reason: {
          kind: "outputMaterializationFailed",
          lane: "eventReturn",
          index: 2,
          failure: {
            kind: "requiredSourceUnavailable",
            publicationFailure: expected,
          },
        },
      });
    },
  );

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

    expect(result.props[0]?.publication).toEqual({
      kind: "published",
      semanticAuthority: "resolved",
      exactness: "exactSymbolic",
      reason: { kind: "resolvedExactSymbolic" },
      provenance: { kind: "resolved", value: "semanticEvaluator" },
    });
    expect(result.props[0]?.terminalDisplay).toEqual({ text: "TreeNode" });
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

  it.each([
    {
      label: "Absent",
      publication: { kind: 2, absence: 1, provenance: 1 },
      expected: {
        kind: "absent",
        absence: "unannotated",
        provenance: "semanticEvaluator",
      },
      typeNodeId: 1,
      displayId: 2,
      expectedDisplay: { text: "TreeNode" },
      hasType: true,
    },
    {
      label: "Failed",
      publication: { kind: 1, failure: 1, provenance: 1 },
      expected: {
        kind: "failed",
        failure: "unrepresentableRequiredMemberValue",
        provenance: "semanticEvaluator",
      },
      typeNodeId: 0,
      displayId: 0,
      expectedDisplay: {},
      hasType: false,
    },
  ])(
    "decodes $label without changing its structured outcome",
    ({ publication, expected, typeNodeId, displayId, expectedDisplay, hasType }) => {
      const base = createTestComponentMetaPayload();
      const payload = create(ComponentMetaPayloadSchema, {
        ...base,
        body: {
          ...base.body!,
          props: [
            {
              nameId: 3,
              typeNodeId,
              publication,
              terminalDisplay: { textId: displayId },
              required: true,
              hasDefault: false,
              tags: [],
            },
          ],
        },
      });

      const result = decodeTypedComponentMetaPayload(toBinary(ComponentMetaPayloadSchema, payload));
      expect(result.props[0]?.publication).toEqual(expected);
      expect(result.props[0]?.type !== undefined).toBe(hasType);
      expect(result.props[0]?.terminalDisplay).toEqual(expectedDisplay);
    },
  );

  it("rejects Failed rows carrying a type or terminal display", () => {
    const base = createTestComponentMetaPayload();
    const payload = create(ComponentMetaPayloadSchema, {
      ...base,
      body: {
        ...base.body!,
        props: [
          {
            nameId: 3,
            typeNodeId: 1,
            publication: { kind: 1, failure: 1, provenance: 1 },
            terminalDisplay: { textId: 2 },
            required: true,
            hasDefault: false,
            tags: [],
          },
        ],
      },
    });

    expect(() =>
      decodeTypedComponentMetaPayload(toBinary(ComponentMetaPayloadSchema, payload)),
    ).toThrow(/carries success output/i);
  });

  it("rejects dropped publication and Published rows without a type", () => {
    const base = createTestComponentMetaPayload();
    const encodeWithProp = (prop: Record<string, unknown>) =>
      toBinary(
        ComponentMetaPayloadSchema,
        create(ComponentMetaPayloadSchema, {
          ...base,
          body: { ...base.body!, props: [prop] },
        }),
      );

    expect(() =>
      decodeTypedComponentMetaPayload(
        encodeWithProp({
          nameId: 3,
          typeNodeId: 1,
          terminalDisplay: { textId: 2 },
          required: true,
          tags: [],
        }),
      ),
    ).toThrow(/publication/i);
    expect(() =>
      decodeTypedComponentMetaPayload(
        encodeWithProp({
          nameId: 3,
          typeNodeId: 0,
          publication: {
            kind: 3,
            provenance: 1,
            semanticAuthority: 1,
            exactness: 1,
            reason: 1,
          },
          terminalDisplay: { textId: 2 },
          required: true,
          tags: [],
        }),
      ),
    ).toThrow(/missing its type/i);
  });

  it("decodes the binding return-wrapper role and its typed reason, keeping absence absent", () => {
    const base = createTestComponentMetaPayload();
    const strings = [...base.typeGraph!.strings, "const", "ref", "maybeRef", "unresolved", "cycle"];
    const id = (text: string) => strings.indexOf(text) + 1;
    const payload = create(ComponentMetaPayloadSchema, {
      ...base,
      typeGraph: { ...base.typeGraph!, strings },
      body: {
        ...base.body!,
        bindings: [
          {
            nameId: id("root"),
            kindId: id("const"),
            reactivityKindId: id("ref"),
            returnWrapperRoleId: id("ref"),
            usedInTemplate: true,
          },
          {
            nameId: id("next"),
            kindId: id("const"),
            reactivityKindId: id("maybeRef"),
            returnWrapperRoleId: id("unresolved"),
            returnWrapperUnresolvedReasonId: id("cycle"),
          },
          {
            nameId: id("label"),
            kindId: id("const"),
            reactivityKindId: id("maybeRef"),
            // Ids left at 0 — the undemanded case.
          },
        ],
      },
    });

    const result = decodeTypedComponentMetaPayload(toBinary(ComponentMetaPayloadSchema, payload));

    // EXACT: the role decodes; the reason stays absent (never `""`).
    expect(result.bindings[0]).toMatchObject({
      name: "root",
      reactivityKind: "ref",
      returnWrapperRole: "ref",
    });
    expect("returnWrapperUnresolvedReason" in result.bindings[0]!).toBe(false);
    // TYPED DEGRADATION: both ids decode, so a reason cannot collapse onto the
    // bare `"unresolved"` discriminant.
    expect(result.bindings[1]).toMatchObject({
      name: "next",
      reactivityKind: "maybeRef",
      returnWrapperRole: "unresolved",
      returnWrapperUnresolvedReason: "cycle",
    });
    // UNDEMANDED: id 0 means the key is ABSENT — never `""`, never `"none"`.
    expect("returnWrapperRole" in result.bindings[2]!).toBe(false);
    expect("returnWrapperUnresolvedReason" in result.bindings[2]!).toBe(false);
  });
});
