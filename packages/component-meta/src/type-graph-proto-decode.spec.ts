import { create, toBinary } from "@bufbuild/protobuf";
import { describe, expect, it } from "vitest";
import {
  LegacyComponentMetaPayloadSchema as ComponentMetaPayloadSchema,
  ComponentMetaPayloadSchema as FactPayloadSchema,
  createTestComponentMetaPayload as factFixture,
  createLegacyTestComponentMetaPayload as createTestComponentMetaPayload,
  OriginGraphSchema,
  SurfacePartialReason,
  SurfacePartialReasonSchema,
} from "@verter/proto";
import {
  BoolFactSchema,
  ExposedMetaSchema,
  ModelMetaSchema,
  TypeRegistryEntrySchema,
} from "../../proto/src/gen/verter/v1/component_meta_pb.js";
import { GRAPH_FORMAT_VERSION } from "./type-graph-core.js";
import { decodeTypedComponentMetaPayload } from "./type-graph-proto-decode.js";

describe("decodeTypedComponentMetaPayload", () => {
  it("publishes independent semantic facts", () => {
    const fixture = factFixture();
    const result = decodeTypedComponentMetaPayload(
      toBinary(FactPayloadSchema, create(FactPayloadSchema, fixture)),
    );
    expect(result).toMatchObject({
      facts: { props: { kind: "complete" }, events: { kind: "complete" } },
    });
  });
  it.each([1, 2])("does not infer domain closure from legacy summary %d", (kind) => {
    const current = createTestComponentMetaPayload();
    const payload = create(ComponentMetaPayloadSchema, {
      ...current,
      body: {
        ...current.body!,
        props: [],
        resultCompleteness: { kind, partialReasons: kind === 2 ? [14] : [] },
      },
    });
    const result = decodeTypedComponentMetaPayload(toBinary(ComponentMetaPayloadSchema, payload));
    expect(result.facts!.props).toMatchObject({
      kind: kind === 1 ? "complete" : "approximate",
      open: true,
      value: [],
    });
    expect(result.facts!.events).toMatchObject({ open: true });
  });
  it("accepts the current schema version and rejects an older response", () => {
    const current = createTestComponentMetaPayload();
    expect(current.schemaVersion).toBe(12);
    expect(() =>
      decodeTypedComponentMetaPayload(
        toBinary(
          ComponentMetaPayloadSchema,
          create(ComponentMetaPayloadSchema, { ...current, schemaVersion: 7 }),
        ),
      ),
    ).toThrow(/expected 14, found 7/);
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
      schemaVersion: 12,
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
    const { LegacyComponentMetaPayloadSchema: ComponentMetaPayloadSchema } =
      await import("@verter/proto");
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
    const { LegacyComponentMetaPayloadSchema: ComponentMetaPayloadSchema } =
      await import("@verter/proto");
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
    const { LegacyComponentMetaPayloadSchema: ComponentMetaPayloadSchema } =
      await import("@verter/proto");
    const payload = create(ComponentMetaPayloadSchema, {
      ...payloadWithMembers([]),
      schemaVersion: 4,
    });
    const bytes = toBinary(ComponentMetaPayloadSchema, payload);
    expect(() => decodeTypedComponentMetaPayload(bytes)).toThrow(/expected 14, found 4/);
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
    // A non-empty taxonomy of unique positive tags. Keyed on the DECLARED
    // value set, not on contiguity: retiring a reason under the never-reuse
    // rule leaves a reserved hole, which is a legal change — while every
    // declared value is still driven through the real decoder by the
    // per-value cases below, so an added-but-unmapped tag still fails.
    expect(named.length).toBeGreaterThan(0);
    const numbers = named.map((value) => value.number);
    expect(new Set(numbers).size).toBe(numbers.length);
    expect(numbers.every((number) => number >= 1)).toBe(true);
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
    // The end-of-taxonomy probe derives from the DECLARED maximum, so it
    // stays an undeclared tag even when the declared set carries holes.
    const beyond = Math.max(...named.map((value) => value.number)) + 1;
    expect(() => decodeReasonsFor([beyond])).toThrow(/unknown surface partial reason/);
    expect(() => decodeReasonsFor([0])).toThrow(/unknown surface partial reason/);
  });

  describe("schema version gate", () => {
    const fanout = SurfacePartialReason.FANOUT_LIMIT;

    const messageAt = (schemaVersion: number, cause: number) => {
      const payload = create(FactPayloadSchema, { ...factFixture(), schemaVersion });
      payload.body!.facts!.delivery = {
        $typeName: "verter.v1.FactGuarantee",
        kind: 3,
        causes: [cause],
      };
      return payload;
    };
    const payloadAt = (schemaVersion: number, cause: number): Uint8Array =>
      toBinary(FactPayloadSchema, messageAt(schemaVersion, cause));

    it("publishes the fan-out reason past the exact gate of schema 13 readers", () => {
      // Those readers accept 12 and 13, then throw on a reason they do not
      // know: the only clean failure for them is the version mismatch.
      expect([12, 13]).not.toContain(GRAPH_FORMAT_VERSION);
      expect(
        decodeTypedComponentMetaPayload(payloadAt(GRAPH_FORMAT_VERSION, fanout)).resultCompleteness,
      ).toEqual({ kind: "partial", reasons: ["fanoutLimit"] });
    });

    it("rejects a payload above its own version at the gate, before its vocabulary", () => {
      // Nothing past the gate is readable: a reason this decoder does not
      // know, in a body it cannot decode. Only the gate running first answers
      // with the version, so a newer payload never fails on what it added.
      const unknown = Math.max(...named.map((value) => value.number)) + 1;
      const newer = messageAt(GRAPH_FORMAT_VERSION + 1, unknown);
      newer.typeGraph = undefined;
      expect(() => decodeTypedComponentMetaPayload(toBinary(FactPayloadSchema, newer))).toThrow(
        /version mismatch/,
      );
    });

    it("keeps decoding schema 13 payloads", () => {
      expect(
        decodeTypedComponentMetaPayload(payloadAt(13, SurfacePartialReason.BUDGET_EXCEEDED))
          .resultCompleteness,
      ).toEqual({ kind: "partial", reasons: ["budgetExceeded"] });
    });
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

  it("stamps a schema-12 failed publication without dropping the healthy sibling", async () => {
    const base = createTestComponentMetaPayload();
    const healthy = base.body!.props![0]!;
    base.body!.props!.push({
      ...healthy,
      nameId: 6,
      typeNodeId: 0,
      publication: {
        ...healthy.publication!,
        kind: 1,
        failure: 1,
        provenance: 1,
      },
      terminalDisplay: { textId: 0 },
    });
    const result = decodeTypedComponentMetaPayload(
      toBinary(ComponentMetaPayloadSchema, create(ComponentMetaPayloadSchema, base)),
    );
    expect(result.facts!.props.kind).not.toBe("unavailable");
    expect(result.props.map((prop) => prop.name)).toEqual(["root", "label"]);
    expect(result.props[1]?.typeFact).toEqual({ kind: "unavailable", causes: [102] });
    expect(result.props[0]?.typeFact?.kind).toBe("complete");
    const { nativeComponentMetaToComponentMeta } = await import("./native-component-meta.js");
    const projected = nativeComponentMetaToComponentMeta(result);
    expect(projected.props.map((prop) => prop.name)).toEqual(["root"]);
  });

  it("localizes a schema-12 invalid node without turning bool fields into facts", async () => {
    const base = createTestComponentMetaPayload();
    base.typeGraph!.nodes[0].kind = {
      case: "array",
      value: { $typeName: "verter.v1.ArrayNode", elementNodeId: 999, readonly: false },
    };
    const first = base.body!.props![0]!;
    first.typeNodeId = 1;
    base.body!.props!.push({ ...first, nameId: 6, typeNodeId: 2 });
    const result = decodeTypedComponentMetaPayload(
      toBinary(ComponentMetaPayloadSchema, create(ComponentMetaPayloadSchema, base)),
    );
    expect(result.props.map((prop) => prop.name)).toEqual(["root", "label"]);
    expect(result.props[0]?.typeFact).toEqual({ kind: "unavailable", causes: [102] });
    expect(result.props[0]?.required).toBe(true);
    expect(result.props[0]?.requiredFact).toBeUndefined();
    expect(result.props[1]?.typeFact?.kind).toBe("complete");
    expect(result.props[1]?.type).toBeDefined();
    const { nativeComponentMetaToComponentMeta } = await import("./native-component-meta.js");
    const projected = nativeComponentMetaToComponentMeta(result);
    expect(projected.props.map((prop) => prop.name)).toEqual(["label"]);
  });
});

describe("independent component facts", () => {
  const fixture = () => create(FactPayloadSchema, factFixture());
  const decode = (payload: ReturnType<typeof fixture>) =>
    decodeTypedComponentMetaPayload(toBinary(FactPayloadSchema, payload));

  it("retains unknown causes and isolates unavailable facts from exact siblings", () => {
    const payload = fixture();
    payload.body!.facts!.events = {
      ...payload.body!.facts!.events!,
      guarantee: { $typeName: "verter.v1.FactGuarantee", kind: 3, causes: [9001] },
      value: undefined,
    };
    const result = decode(payload);
    expect(result.facts!.events).toEqual({ kind: "unavailable", causes: [9001] });
    expect(result.facts!.props.kind).toBe("complete");
    expect(result.props[0]?.typeFact?.kind).toBe("complete");
    expect(result.resultCompleteness).toEqual({ kind: "partial", reasons: ["propagated"] });
    expect(result.events).toEqual([]);
  });

  it("preserves false, proven absence, and a complete open membership domain", () => {
    const payload = fixture();
    payload.body!.facts!.props!.value!.open = true;
    payload.body!.facts!.props!.value!.entries[0].requiredFact!.value = false;
    const result = decode(payload);
    expect(result.facts!.props).toMatchObject({ kind: "complete", open: true });
    expect(result.props[0]?.required).toBe(false);
    expect(result.props[0]?.hasDefault).toBe(false);
    expect(result.props[0]?.requiredFact).toEqual({ kind: "complete", value: false });
    expect(result.props[0]?.hasDefaultFact).toEqual({ kind: "complete", value: false });
    expect(result.facts!.publicInstance).toEqual({ kind: "complete", value: undefined });
  });

  it.each(["approximateWithoutCauses", "unavailableWithValue"] as const)(
    "refuses a contradictory %s without poisoning siblings",
    (mode) => {
      const payload = fixture();
      payload.body!.facts!.props!.guarantee!.kind = mode === "approximateWithoutCauses" ? 2 : 3;
      const result = decode(payload);
      expect(result.facts!.props).toEqual({ kind: "unavailable", causes: [100] });
      expect(result.facts!.events.kind).toBe("complete");
    },
  );

  it("propagates an invalid shared node only to dependent value facts", async () => {
    const payload = fixture();
    payload.typeGraph!.nodes[0].kind = {
      case: "array",
      value: { $typeName: "verter.v1.ArrayNode", elementNodeId: 999, readonly: false },
    };
    const first = payload.body!.facts!.props!.value!.entries[0];
    payload.body!.facts!.props!.value!.entries.push({ ...first, nameId: 6, typeNodeId: 2 });
    const result = decode(payload);
    expect(result.facts!.props.kind).toBe("complete");
    expect(result.props[0]?.typeFact).toEqual({ kind: "unavailable", causes: [100] });
    expect(result.props[0]?.type).toBeUndefined();
    expect(result.props[1]?.typeFact?.kind).toBe("complete");
    expect(result.slots[0]?.bindings[0]?.typeFact?.kind).toBe("unavailable");
    expect(result.facts!.events.kind).toBe("complete");
    const { nativeComponentMetaToComponentMeta } = await import("./native-component-meta.js");
    const projected = nativeComponentMetaToComponentMeta(result);
    expect(projected.props.map((prop) => prop.name)).toEqual([result.props[1].name]);
    expect(projected.slots[0].bindings).toEqual([]);
    expect(projected.facts!.props.kind).toBe("complete");
  });

  it("keeps a public-instance fact when one member type node is invalid", async () => {
    const payload = fixture();
    payload.typeGraph!.nodes[0].kind = {
      case: "array",
      value: { $typeName: "verter.v1.ArrayNode", elementNodeId: 999, readonly: false },
    };
    const instance = payload.body!.facts!.publicInstance!;
    instance.absent = false;
    instance.value = {
      $typeName: "verter.v1.PublicInstanceMeta",
      completeness: 1,
      members: [
        {
          $typeName: "verter.v1.PublicInstanceMemberMeta",
          nameId: 6,
          kind: 3,
          typeNodeId: 1,
          rawTypeId: 0,
          descriptionId: 0,
          tags: [],
        },
        {
          $typeName: "verter.v1.PublicInstanceMemberMeta",
          nameId: 7,
          kind: 3,
          typeNodeId: 2,
          rawTypeId: 0,
          descriptionId: 0,
          tags: [],
        },
      ],
    };
    const result = decode(payload);
    expect(result.facts!.publicInstance.kind).not.toBe("unavailable");
    expect(result.publicInstance?.members.map((member) => member.name)).toEqual(["label", "next"]);
    expect(result.publicInstance?.members[0]?.typeFact).toEqual({
      kind: "unavailable",
      causes: [100],
    });
    expect(result.publicInstance?.members[1]?.type).toBeDefined();
    const { nativeComponentMetaToComponentMeta } = await import("./native-component-meta.js");
    const projected = nativeComponentMetaToComponentMeta(result);
    expect(projected.publicInstance?.members.map((member) => member.name)).toEqual(["next"]);
  });

  it("localizes a string literal whose string id is zero", async () => {
    const payload = fixture();
    payload.typeGraph!.nodes[0].kind = {
      case: "literal",
      value: { $typeName: "verter.v1.LiteralNode", literalKind: 1, stringId: 0 },
    };
    const props = payload.body!.facts!.props!.value!.entries;
    props[0].typeNodeId = 1;
    props.push({ ...props[0], nameId: 6, typeNodeId: 2 });
    const result = decode(payload);
    expect(result.props[0]?.typeFact).toEqual({ kind: "unavailable", causes: [100] });
    expect(result.props[1]?.typeFact?.kind).toBe("complete");
    const { nativeComponentMetaToComponentMeta } = await import("./native-component-meta.js");
    const projected = nativeComponentMetaToComponentMeta(result);
    expect(projected.props.map((prop) => prop.name)).toEqual(["label"]);
  });

  it("keeps constrained binders distinct and renders their constraints", async () => {
    const { typeExprToDescriptor } = await import("./type-expr-bridge.js");
    const payload = fixture();
    payload.typeGraph!.nodes.push({
      $typeName: "verter.v1.TypeNode",
      kind: {
        case: "infer",
        value: { $typeName: "verter.v1.InferNode", nameId: 2, constraintNodeId: 1 },
      },
    });
    payload.typeGraph!.nodes.push({
      $typeName: "verter.v1.TypeNode",
      kind: {
        case: "infer",
        value: { $typeName: "verter.v1.InferNode", nameId: 2, constraintNodeId: 2 },
      },
    });
    // A binder declares in a conditional's `extends` clause and is
    // referenced by name in its true branch.
    for (const inferNodeId of [6, 7]) {
      payload.typeGraph!.nodes.push({
        $typeName: "verter.v1.TypeNode",
        kind: {
          case: "conditional",
          value: {
            $typeName: "verter.v1.ConditionalNode",
            checkNodeId: 1,
            extendsNodeId: inferNodeId,
            trueTypeNodeId: inferNodeId,
            falseTypeNodeId: 1,
          },
        },
      });
    }
    const first = payload.body!.facts!.props!.value!.entries[0];
    first.typeNodeId = 8;
    payload.body!.facts!.props!.value!.entries.push({ ...first, nameId: 6, typeNodeId: 9 });
    const result = decode(payload);
    expect(typeExprToDescriptor(result.props[0].type!)).toMatchObject({
      rawType: "string extends (infer TreeNode extends string) ? TreeNode : string",
    });
    expect(typeExprToDescriptor(result.props[1].type!)).toMatchObject({
      rawType: "string extends (infer TreeNode extends undefined) ? TreeNode : string",
    });
  });

  it("keeps healthy models and registry entries when one value node is invalid", async () => {
    const payload = fixture();
    payload.typeGraph!.nodes[0].kind = {
      case: "array",
      value: { $typeName: "verter.v1.ArrayNode", elementNodeId: 999, readonly: false },
    };
    payload.body!.facts!.models!.value!.entries.push(
      create(ModelMetaSchema, { nameId: 6, typeNodeId: 1 }),
      create(ModelMetaSchema, { nameId: 7, typeNodeId: 2 }),
    );
    payload.body!.facts!.exposed!.value!.entries.push(
      create(ExposedMetaSchema, { nameId: 6, typeNodeId: 1, descriptionId: 0, tags: [] }),
      create(ExposedMetaSchema, { nameId: 7, typeNodeId: 2, descriptionId: 0, tags: [] }),
    );
    payload.body!.facts!.typeRegistry!.value!.entries.push(
      create(TypeRegistryEntrySchema, { nameId: 6, typeNodeId: 1, rawTypeId: 0 }),
      create(TypeRegistryEntrySchema, { nameId: 7, typeNodeId: 2, rawTypeId: 0 }),
    );
    const result = decode(payload);
    expect(result.facts!.models.kind).toBe("complete");
    expect(result.models.map((model) => model.name)).toEqual(["label", "next"]);
    expect(result.models[0]?.typeFact).toEqual({ kind: "unavailable", causes: [100] });
    expect(result.models[1]?.typeFact?.kind).toBe("complete");
    expect(result.models[1]?.type).toBeDefined();
    expect(result.facts!.exposed.kind).toBe("complete");
    expect(result.exposed.map((entry) => entry.name)).toEqual(["label", "next"]);
    expect(result.exposed[0]?.typeFact?.kind).toBe("unavailable");
    expect(result.exposed[1]?.type).toBeDefined();
    expect(result.facts!.typeRegistry.kind).toBe("complete");
    expect(result.typeRegistry.map((entry) => entry.name)).toEqual(["TreeNode", "label", "next"]);
    expect(result.typeRegistry.find((entry) => entry.name === "next")?.typeFact?.kind).toBe(
      "complete",
    );
    const { nativeComponentMetaToComponentMeta, nativeTypeRegistryToMap } =
      await import("./native-component-meta.js");
    const projected = nativeComponentMetaToComponentMeta(result);
    expect(projected.models.map((model) => model.name)).toEqual(["next"]);
    expect(projected.exposed.map((entry) => entry.name)).toEqual(["next"]);
    expect([...(nativeTypeRegistryToMap(result)?.keys() ?? [])]).toEqual(["next"]);
  });

  it("projects an unavailable boolean fact as absence, not false", async () => {
    const payload = fixture();
    const props = payload.body!.facts!.props!.value!.entries;
    const unavailableBool = () =>
      create(BoolFactSchema, {
        guarantee: { kind: 3, causes: [104] },
      });
    const falseBool = () =>
      create(BoolFactSchema, {
        guarantee: { kind: 1, causes: [] },
        value: false,
      });
    props[0].requiredFact = unavailableBool();
    props[0].hasDefaultFact = unavailableBool();
    props.push({ ...props[0], nameId: 6 });
    props[1].requiredFact = falseBool();
    props[1].hasDefaultFact = falseBool();
    const result = decode(payload);
    expect(result.facts!.props.kind).toBe("complete");
    expect(result.props.map((prop) => prop.name)).toEqual(["root", "label"]);
    expect(result.props[0]?.required).toBeUndefined();
    expect(result.props[0]?.hasDefault).toBeUndefined();
    expect(result.props[0]?.requiredFact).toEqual({ kind: "unavailable", causes: [104] });
    expect(result.props[1]?.required).toBe(false);
    expect(result.props[1]?.hasDefault).toBe(false);
    const { nativeComponentMetaToComponentMeta } = await import("./native-component-meta.js");
    const { propsToJsonSchema } = await import("./adapters/json-schema.js");
    const { propsToZodString } = await import("./adapters/zod.js");
    const projected = nativeComponentMetaToComponentMeta(result);
    expect(projected.props.map((prop) => prop.name)).toEqual(["root", "label"]);
    expect(projected.props[0]?.required).toBeUndefined();
    expect(projected.props[1]?.required).toBe(false);
    const schema = propsToJsonSchema(projected);
    expect(schema.properties).toHaveProperty("root");
    expect(schema.properties).toHaveProperty("label");
    expect(schema.required ?? []).not.toContain("root");
    expect(schema.required ?? []).not.toContain("label");
    const zod = propsToZodString(projected);
    expect(zod).toContain('"root"');
    expect(zod).toContain('"label"');
    expect(zod).toContain(".optional()");
  });

  it("keeps failed accepted and fallthrough values from aborting projection", async () => {
    const result = decode(fixture());
    const type = result.props[0].type!;
    const publication = result.props[0].publication;
    const terminalDisplay = result.props[0].terminalDisplay;
    const failedPublication = {
      kind: "failed" as const,
      failure: "unrepresentableRequiredMemberValue" as const,
      provenance: "semanticEvaluator" as const,
    };
    const jsonMember = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T;
    result.acceptedProps = [
      jsonMember({
        name: "bad",
        type: null,
        publication: failedPublication,
        terminalDisplay: {},
        required: true,
        provenance: { kind: "declared" },
        availability: { kind: "always" },
        kind: "declaredProp",
      }),
      {
        name: "ok",
        type,
        typeFact: { kind: "complete", value: type },
        publication,
        terminalDisplay,
        required: false,
        provenance: { kind: "declared" },
        availability: { kind: "always" },
        kind: "declaredProp",
      },
    ];
    result.acceptedEvents = [
      jsonMember({
        name: "bad",
        payload: null,
        provenance: { kind: "declared" },
        availability: { kind: "always" },
        kind: "listener",
      }),
      {
        name: "ok",
        payload: type,
        typeFact: { kind: "complete", value: type },
        provenance: { kind: "declared" },
        availability: { kind: "always" },
        kind: "declaredEmit",
      },
    ];
    result.fallthroughSurface = {
      kind: "branches",
      branches: [
        {
          branchKey: "root",
          props: [
            jsonMember({
              name: "bad",
              type: null,
              publication: failedPublication,
              terminalDisplay: {},
              sources: [],
            }),
            {
              name: "ok",
              type,
              typeFact: { kind: "complete", value: type },
              publication,
              terminalDisplay,
              sources: [],
            },
          ],
          events: [
            jsonMember({ name: "bad", payload: null, sources: [] }),
            {
              name: "ok",
              payload: type,
              typeFact: { kind: "complete", value: type },
              sources: [],
            },
          ],
          rootChain: [],
          status: { kind: "resolved" },
        },
      ],
    };
    const { nativeComponentMetaToComponentMeta } = await import("./native-component-meta.js");
    const projected = nativeComponentMetaToComponentMeta(result);
    expect(result.acceptedProps.map((prop) => prop.name)).toEqual(["bad", "ok"]);
    expect(projected.facts).toBe(result.facts);
    expect(projected.acceptedProps.map((prop) => prop.name)).toEqual(["ok"]);
    expect(projected.acceptedEvents.map((event) => event.name)).toEqual(["ok"]);
    if (projected.fallthroughSurface.kind !== "branches") {
      throw new Error("expected fallthrough branches");
    }
    expect(projected.fallthroughSurface.branches[0].props.map((prop) => prop.name)).toEqual(["ok"]);
    expect(projected.fallthroughSurface.branches[0].events.map((event) => event.name)).toEqual([
      "ok",
    ]);
  });

  it("ignores a conflicting legacy summary and preserves the delivery refusal", () => {
    const payload = fixture();
    payload.body!.facts!.delivery!.kind = 3;
    payload.body!.facts!.delivery!.causes = [20];
    const result = decode(payload);
    expect(result.facts!.delivery).toEqual({ kind: "unavailable", causes: [20] });
    expect(result.resultCompleteness).toEqual({ kind: "partial", reasons: ["operationBudget"] });
    expect(result.facts!.props.kind).toBe("complete");
  });

  it("keeps an unavailable adapter distinct from component-meta absence", () => {
    const payload = fixture();
    payload.body!.facts!.componentPublicContract = {
      $typeName: "verter.v1.ComponentContractFact",
      guarantee: { $typeName: "verter.v1.FactGuarantee", kind: 3, causes: [106] },
      absent: false,
    };
    const result = decode(payload);
    expect(result.componentPublicContract).toMatchObject({
      kind: "unsupported",
      reason: { kind: "adapterUnavailable" },
    });
    expect(result.facts!.props.kind).toBe("complete");
  });
});
