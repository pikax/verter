import { create, toBinary } from "@bufbuild/protobuf";
import { describe, expect, it } from "vitest";
import {
  ComponentMetaPayloadSchema,
  createTestComponentMetaPayload,
  OriginGraphSchema,
  SurfacePartialReasonSchema,
} from "@verter/proto";
import { decodeTypedComponentMetaPayload } from "./type-graph-proto-decode.js";

describe("decodeTypedComponentMetaPayload", () => {
  it("accepts the current schema version and rejects an older response", () => {
    const current = createTestComponentMetaPayload();
    expect(current.schemaVersion).toBe(11);
    expect(() =>
      decodeTypedComponentMetaPayload(
        toBinary(
          ComponentMetaPayloadSchema,
          create(ComponentMetaPayloadSchema, { ...current, schemaVersion: 7 }),
        ),
      ),
    ).toThrow(/expected 11, found 7/);
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

describe("object-literal spread member wire decode", () => {
  it("decodes a spread member (kind 6) instead of throwing", async () => {
    const { buildTestComponentMetaProtoPayload } = await import("./type-graph.test-utils.js");
    const init = buildTestComponentMetaProtoPayload({
      filePath: "/spread.vue",
      props: [
        {
          name: "p",
          type: {
            kind: "object",
            properties: [{ name: "a", type: { kind: "primitive", name: "number" } }],
          },
        },
      ],
    });
    // Append a pre-fold SPREAD entry (kind 6) to the object node's members —
    // its operand rides the member's typeNodeId slot (reuse the existing
    // property's type node).
    const nodes = (init.typeGraph!.nodes ?? []) as Array<{
      kind?: { case?: string; value?: { members?: Array<Record<string, unknown>> } };
    }>;
    const objectNode = nodes.find((node) => node.kind?.case === "object");
    expect(objectNode).toBeDefined();
    const members = objectNode!.kind!.value!.members!;
    const operandNodeId = members[0].typeNodeId as number;
    members.push({
      kind: 6,
      nameId: 0,
      typeNodeId: operandNodeId,
      optional: false,
      readonly: false,
      keyNameId: 0,
      keyTypeNodeId: 0,
      valueTypeNodeId: 0,
      functionNodeId: 0,
    });

    const payload = create(ComponentMetaPayloadSchema, init);
    const bytes = toBinary(ComponentMetaPayloadSchema, payload);

    const result = decodeTypedComponentMetaPayload(bytes);
    expect(result.props).toHaveLength(1);
  });
});

describe("typed property keys", () => {
  const MEMBER_PROPERTY = 1;

  function payloadWithMembers(members: unknown[]) {
    return {
      schemaVersion: 11,
      typeGraph: {
        strings: ["/x.ts", "alpha", "tag", "Obj"],
        nodes: [
          { kind: { case: "primitive", value: { primitive: 1 } } },
          { kind: { case: "primitive", value: { primitive: 6 } } },
          { kind: { case: "ref", value: { nameId: 4, typeArgumentNodeIds: [] } } },
          {
            kind: {
              case: "object",
              value: { members },
            },
          },
        ],
      },
      typeRegistry: [{ nameId: 4, typeNodeId: 4, rawTypeId: 4 }],
      body: {
        filePathId: 1,
        optionsApi: false,
        props: [],
        events: [],
        slots: [],
        acceptedProps: [],
        acceptedEvents: [],
        acceptedSurfaceCompleteness: 1,
        resultCompleteness: { kind: 1, partialReasons: [] },
        rootReachability: { kind: 1, reason: 5, branches: [] },
        fallthroughSurface: { kind: 1, reason: 5, branches: [] },
        orderedSfcStructure: { schemaVersion: 1, artifactToken: "", blocks: [], markupNodes: [] },
        componentPublicContract: {
          availability: {
            case: "unsupported",
            value: { adapterId: 1, reason: 1, diagnostics: [] },
          },
        },
        models: [],
        exposed: [],
        components: [],
        templateRefs: [],
        imports: [],
        bindings: [],
        vueApiCalls: [],
        styles: [],
        flags: {},
      },
    };
  }

  function member(key: unknown, typeNodeId: number) {
    return {
      kind: MEMBER_PROPERTY,
      propertyKey: key,
      typeNodeId,
      optional: false,
      readonly: false,
      keyNameId: 0,
      keyTypeNodeId: 0,
      valueTypeNodeId: 0,
      functionNodeId: 0,
      methodKind: 0,
      hasImplementationBody: false,
    };
  }

  it("decodes every property-key kind", async () => {
    const { create, toBinary } = await import("@bufbuild/protobuf");
    const { ComponentMetaPayloadSchema } = await import("@verter/proto");
    const payload = create(
      ComponentMetaPayloadSchema,
      payloadWithMembers([
        member({ key: { case: "stringId", value: 2 } }, 1),
        member({ key: { case: "canonicalNumber", value: 7n } }, 2),
        member(
          {
            key: {
              case: "uniqueSymbol",
              value: { canonicalId: 1, ownerKind: 0, ownerOrdinal: 0, symbol: 3, memberPath: [] },
            },
          },
          1,
        ),
        member({ key: { case: "computedNodeId", value: 3 } }, 2),
      ]),
    );
    const bytes = toBinary(ComponentMetaPayloadSchema, payload);
    const result = decodeTypedComponentMetaPayload(bytes);

    const entry = (result.typeRegistry as { name: string; type: unknown }[])[0];
    expect(entry.name).toBe("Obj");
    const ref = entry.type as { graph: { getNode(id: number): any }; nodeId: number };
    const node = ref.graph.getNode(ref.nodeId);
    const [stringKey, numberKey, symbolKey, computedKey] = node.members.map(
      (m: { key: unknown }) => m.key,
    );
    expect(stringKey).toEqual({ kind: "string", nameId: 2 });
    expect(numberKey).toEqual({ kind: "number", value: 7 });
    expect(symbolKey).toEqual({
      kind: "uniqueSymbol",
      nameId: 3,
      canonicalNameId: 1,
      ownerKind: 0,
      ownerOrdinal: 0,
      memberPathNameIds: [],
    });
    expect(computedKey).toEqual({ kind: "computed", nodeId: 3 });
  });

  it("resolves display names for string, number, and symbol keys through the bridge", async () => {
    const { create, toBinary } = await import("@bufbuild/protobuf");
    const { ComponentMetaPayloadSchema } = await import("@verter/proto");
    const { nativeTypeRegistryToMap } = await import("./native-component-meta.js");
    const { decodeComponentMetaPayload } = await import("./type-graph.js");
    const payload = create(
      ComponentMetaPayloadSchema,
      payloadWithMembers([
        member({ key: { case: "stringId", value: 2 } }, 1),
        member({ key: { case: "canonicalNumber", value: 7n } }, 2),
        member(
          {
            key: {
              case: "uniqueSymbol",
              value: { canonicalId: 1, ownerKind: 0, ownerOrdinal: 0, symbol: 3, memberPath: [] },
            },
          },
          1,
        ),
        member({ key: { case: "computedNodeId", value: 3 } }, 2),
      ]),
    );
    const bytes = toBinary(ComponentMetaPayloadSchema, payload);
    const native = decodeComponentMetaPayload(bytes);
    const obj = nativeTypeRegistryToMap(native)?.get("Obj");
    expect(obj).toBeDefined();
    const names = (obj as { properties: { name: string }[] }).properties.map((p) => p.name);
    expect(names).toEqual(["alpha", "7", "tag", ""]);
  });

  it("rejects a schema-4 payload with the typed version error", async () => {
    const { create, toBinary } = await import("@bufbuild/protobuf");
    const { ComponentMetaPayloadSchema } = await import("@verter/proto");
    const payload = create(ComponentMetaPayloadSchema, {
      ...payloadWithMembers([]),
      schemaVersion: 4,
    });
    const bytes = toBinary(ComponentMetaPayloadSchema, payload);
    expect(() => decodeTypedComponentMetaPayload(bytes)).toThrow(/expected 11, found 4/);
  });
});

/**
 * Parity between the closed `SurfacePartialReason` wire taxonomy and the
 * native reason names the decoder publishes.
 *
 * The reason set has three hand-written mirrors (Rust `PartialReason`, the
 * proto enum, the native string union) and only one of them is mechanically
 * derived from another. Nothing pinned the decoder's mapping to the generated
 * enum, so a type-valid edit — swapping two rows, or appending a proto value
 * while grouping the native spelling logically — relabelled reasons with zero
 * test movement: a budget trip reached consumers as `cancelled`. The typeinfo
 * surface has this guard (`typeinfo_graph_taxonomy`); this is the
 * component-meta analogue.
 *
 * The oracle is the GENERATED descriptor, never a hand-listed set: every
 * value the schema declares is driven through the real decoder, and the
 * expected native name is derived mechanically from that value's own proto
 * name. Appending a reason to the proto without a decoder row fails here (and
 * at `tsc`, because the decoder's map is a total `Record` over the enum);
 * misspelling or transposing a row fails here.
 */
describe("surface partial reason taxonomy parity", () => {
  const values = SurfacePartialReasonSchema.values;
  const unspecified = values.find((value) => value.number === 0);
  if (!unspecified) throw new Error("the closed taxonomy must declare an unspecified zero value");
  // Derived, not hardcoded: the shared prefix is whatever the zero value
  // carries in front of `UNSPECIFIED`.
  const prefix = unspecified.name.replace(/UNSPECIFIED$/, "");
  const named = values.filter((value) => value.number !== 0);

  /** `SURFACE_PARTIAL_REASON_BUDGET_EXCEEDED` -> `budgetExceeded`. */
  const nativeNameOf = (protoName: string): string =>
    protoName
      .slice(prefix.length)
      .toLowerCase()
      .replace(/_(.)/g, (_, c: string) => c.toUpperCase());

  const decodeReasonsFor = (reasons: number[]): string[] => {
    const base = createTestComponentMetaPayload();
    const payload = create(ComponentMetaPayloadSchema, {
      ...base,
      body: { ...base.body!, resultCompleteness: { kind: 2, partialReasons: reasons } },
    });
    const decoded = decodeTypedComponentMetaPayload(
      toBinary(ComponentMetaPayloadSchema, payload),
    ).resultCompleteness;
    if (decoded.kind !== "partial") throw new Error("fixture must decode as partial");
    return decoded.reasons;
  };

  it("declares every reason the producer can emit", () => {
    // A non-empty, contiguous 1..n taxonomy — so a reserved/removed tag shows
    // up here rather than as a silently skipped decoder row.
    expect(named.length).toBeGreaterThan(0);
    expect(named.map((value) => value.number)).toEqual(
      Array.from({ length: named.length }, (_, i) => i + 1),
    );
  });

  it.each(named.map((value) => [value.number, value.name, nativeNameOf(value.name)] as const))(
    "decodes wire reason %d (%s) as %s",
    (number, _protoName, expected) => {
      expect(decodeReasonsFor([number])).toEqual([expected]);
    },
  );

  it("decodes the whole taxonomy in one payload without transposing a reason", () => {
    // The per-value cases above would still pass under a mapping that is
    // correct one-at-a-time; this drives all of them together in declaration
    // order so a positional/offset regression shows as a shifted list.
    expect(decodeReasonsFor(named.map((value) => value.number))).toEqual(
      named.map((value) => nativeNameOf(value.name)),
    );
  });

  it("fails closed on a reason tag the taxonomy does not declare", () => {
    expect(() => decodeReasonsFor([named.length + 1])).toThrow(/unknown surface partial reason/);
    expect(() => decodeReasonsFor([0])).toThrow(/unknown surface partial reason/);
  });
});

/**
 * `decodeResultCompleteness`'s fail-closed branches. The field exists so a
 * degraded payload cannot read as whole, so every way a payload can fail to
 * state its completeness must throw rather than default to complete.
 *
 * RED proof for the first case: simplify the function to
 * `kind === PARTIAL ? { kind: "partial", ... } : { kind: "complete" }` and an
 * UNSET kind reads as COMPLETE — the exact wrong-complete outcome — while
 * every other test in this file still passes.
 */
describe("result completeness fail-closed branches", () => {
  const decodeCompletenessOf = (completeness: Record<string, unknown>) => {
    const base = createTestComponentMetaPayload();
    const payload = create(ComponentMetaPayloadSchema, {
      ...base,
      body: { ...base.body!, resultCompleteness: completeness as never },
    });
    return decodeTypedComponentMetaPayload(toBinary(ComponentMetaPayloadSchema, payload))
      .resultCompleteness;
  };

  it("rejects an unset completeness kind instead of reading it as complete", () => {
    expect(() => decodeCompletenessOf({ kind: 0, partialReasons: [] })).toThrow(
      /unknown result completeness 0/,
    );
  });

  it("rejects an unknown completeness kind", () => {
    expect(() => decodeCompletenessOf({ kind: 99, partialReasons: [] })).toThrow(
      /unknown result completeness 99/,
    );
  });

  it("rejects a partial surface that names no reason", () => {
    expect(() => decodeCompletenessOf({ kind: 2, partialReasons: [] })).toThrow(
      /partial surface carries no reason/,
    );
  });

  it("rejects a complete surface that nonetheless names reasons", () => {
    // Unreachable from the in-tree producer; a foreign producer sending it
    // must not have its reasons silently dropped and be republished clean.
    expect(() => decodeCompletenessOf({ kind: 1, partialReasons: [1] })).toThrow(
      /complete surface carries partial reasons/,
    );
  });

  it("still accepts the well-formed complete surface", () => {
    expect(decodeCompletenessOf({ kind: 1, partialReasons: [] })).toEqual({ kind: "complete" });
  });
});
