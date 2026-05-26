/**
 * R22-final S0 discriminating test: synthetic-carrier typed-IR cutover.
 *
 * Asserts that a `SyntheticSlotBindingNode` produced in a `ComponentMetaPayload`
 * TypeGraph round-trips through:
 *   - proto decode (`decodeComponentMetaPayload`)
 *   - typed-IR bridge (`typeExprToDescriptor` / `nativeComponentMetaToComponentMeta`)
 *   - compat display (`mapPropMeta` → public `PropertyMeta.type`)
 *   - schema (`typeDescriptorToSchema`)
 *   - bench refiner (`typeMetaToString` → `benchmarkTypeDescriptorToString`)
 *
 * Negative gate: even when the `TypeRegistry` contains an entry whose `name`
 * matches the carrier's `bindingName`, the consumer pipelines MUST NOT
 * resolve the carrier through the registry (same-name poisoning risk).
 *
 * Pre-S0: the variant did not exist; the test fails because (a) the proto
 * field is unknown to the decoder OR (b) `TypeDescriptor` does not include
 * `kind: "syntheticSlotBinding"` so `nativeComponentMetaToComponentMeta`
 * cannot produce it OR (c) the compat / schema / bench paths fall through
 * to "ref" / "unknown".
 * Post-S0: all assertions pass.
 */

import { describe, expect, it } from "vitest";

import { typeDescriptorToSchema } from "./compat/schema.js";
import { mapPropMeta } from "./compat/checker.js";
import { nativeComponentMetaToComponentMeta } from "./native-component-meta.js";
import { typeExprToDescriptor } from "./type-expr-bridge.js";
import { decodeComponentMetaPayload, isGraphTypeExprRef } from "./type-graph.js";
import {
  buildTestComponentMetaProtoPayload,
  encodeTestComponentMetaPayload,
} from "./type-graph.test-utils.js";
import { typeMetaToString } from "../../benchmark/src/meta-ui-meta.js";

const CARRIER_SCOPE = "scope://Card.vue";
const CARRIER_BINDING_NAME = "FooSlotBinding";
const CARRIER_VALUE_NODE = "0";
const CARRIER_SLOT_NAME = "default";

describe("synthetic slot binding carrier (R22-final S0)", () => {
  it("decodes the proto SyntheticSlotBindingNode and produces a SyntheticSlotBindingType descriptor", () => {
    const payload = encodeTestComponentMetaPayload({
      filePath: "/project/src/Card.vue",
      props: [
        {
          name: "carrier",
          type: {
            kind: "syntheticSlotBinding",
            scopeCanonicalId: CARRIER_SCOPE,
            surfaceKind: "slotBinding",
            slotName: CARRIER_SLOT_NAME,
            bindingName: CARRIER_BINDING_NAME,
            valueNode: CARRIER_VALUE_NODE,
          },
          rawType: CARRIER_BINDING_NAME,
          required: true,
        },
      ],
    });

    const native = decodeComponentMetaPayload(payload);

    // Decode yields a graph-backed ref; bridging it produces the carrier
    // descriptor.
    expect(isGraphTypeExprRef(native.props[0]!.type)).toBe(true);

    const descriptor = typeExprToDescriptor(native.props[0]!.type);
    expect(descriptor.kind).toBe("syntheticSlotBinding");

    // Narrow for field assertions.
    if (descriptor.kind !== "syntheticSlotBinding") {
      throw new Error("unreachable — kind narrowed above");
    }

    expect(descriptor.bindingName).toBe(CARRIER_BINDING_NAME);
    expect(descriptor.scopeCanonicalId).toBe(CARRIER_SCOPE);
    expect(descriptor.surfaceKind).toBe("slotBinding");
    expect(descriptor.slotName).toBe(CARRIER_SLOT_NAME);

    // `valueNode` is a Rust `u64` SemanticNodeId carried as a STRING to
    // avoid JS Number precision loss. Amendment 7: `SemanticNodeId(0)` is
    // a legitimate value — assert NON-EMPTY, not `!= "0"`.
    expect(typeof descriptor.valueNode).toBe("string");
    expect(descriptor.valueNode.length).toBeGreaterThan(0);
  });

  it("renders the carrier through the compat display path as bindingName", () => {
    const payload = encodeTestComponentMetaPayload({
      filePath: "/project/src/Card.vue",
      props: [
        {
          name: "carrier",
          type: {
            kind: "syntheticSlotBinding",
            scopeCanonicalId: CARRIER_SCOPE,
            surfaceKind: "binding",
            bindingName: CARRIER_BINDING_NAME,
            valueNode: CARRIER_VALUE_NODE,
          },
          rawType: CARRIER_BINDING_NAME,
          required: true,
        },
      ],
      // Negative-gate fixture: a registry entry whose name *matches* the
      // carrier's `bindingName`. The compat display path MUST NOT resolve
      // the carrier through this entry — that would be same-name
      // poisoning.
      typeRegistry: [
        {
          name: CARRIER_BINDING_NAME,
          type: {
            kind: "object",
            properties: [{ name: "poisonedField", type: { kind: "primitive", name: "string" } }],
          },
          rawType: `{ poisonedField: string }`,
        },
      ],
    });

    const native = decodeComponentMetaPayload(payload);
    const compat = nativeComponentMetaToComponentMeta(native);
    const propMeta = mapPropMeta(compat.props[0]!);

    // The compat display routes through `typeDescriptorToCompatDisplay`
    // → carrier case → returns `bindingName`.
    expect(propMeta.type).toBe(CARRIER_BINDING_NAME);
    expect(propMeta.type).not.toContain("poisonedField");
  });

  it("renders the carrier through compat schema as an opaque object with bindingName", () => {
    const payload = encodeTestComponentMetaPayload({
      filePath: "/project/src/Card.vue",
      props: [
        {
          name: "carrier",
          type: {
            kind: "syntheticSlotBinding",
            scopeCanonicalId: CARRIER_SCOPE,
            surfaceKind: "slotBinding",
            slotName: CARRIER_SLOT_NAME,
            bindingName: CARRIER_BINDING_NAME,
            valueNode: CARRIER_VALUE_NODE,
          },
          rawType: CARRIER_BINDING_NAME,
          required: true,
        },
      ],
      // Same negative-gate fixture: schema layer must not consult this.
      typeRegistry: [
        {
          name: CARRIER_BINDING_NAME,
          type: {
            kind: "object",
            properties: [{ name: "poisonedField", type: { kind: "primitive", name: "string" } }],
          },
          rawType: `{ poisonedField: string }`,
        },
      ],
    });

    const native = decodeComponentMetaPayload(payload);
    const compat = nativeComponentMetaToComponentMeta(native);
    const descriptor = compat.props[0]!.type;
    expect(descriptor.kind).toBe("syntheticSlotBinding");

    const schema = typeDescriptorToSchema(descriptor);
    expect(schema).toEqual({
      kind: "object",
      type: CARRIER_BINDING_NAME,
      schema: {},
    });

    // Negative assertion: the schema must not contain the registry's
    // poisoned-shape members.
    expect(JSON.stringify(schema)).not.toContain("poisonedField");
  });

  it("renders the carrier through the bench refiner as bindingName", () => {
    const carrierDescriptor = {
      kind: "syntheticSlotBinding" as const,
      scopeCanonicalId: CARRIER_SCOPE,
      surfaceKind: "binding" as const,
      bindingName: CARRIER_BINDING_NAME,
      valueNode: CARRIER_VALUE_NODE,
    };

    // The bench refiner display path treats the carrier as opaque and
    // surfaces `bindingName` directly — no registry resolution attempt.
    expect(typeMetaToString(carrierDescriptor)).toBe(CARRIER_BINDING_NAME);
  });

  it("decodes a payload that carries the SyntheticSlotBindingNode and validates required graph fields", () => {
    // Build via the test-utils helper which exercises the same proto
    // schema path the producer would use.
    const init = buildTestComponentMetaProtoPayload({
      filePath: "/project/src/Card.vue",
      props: [
        {
          name: "carrier",
          type: {
            kind: "syntheticSlotBinding",
            scopeCanonicalId: CARRIER_SCOPE,
            surfaceKind: "slotBinding",
            slotName: CARRIER_SLOT_NAME,
            bindingName: CARRIER_BINDING_NAME,
            valueNode: CARRIER_VALUE_NODE,
          },
          rawType: CARRIER_BINDING_NAME,
        },
      ],
    });

    // The encoded TypeGraph must contain exactly one node whose oneof
    // discriminator is "syntheticSlotBinding".
    const nodes = init.typeGraph!.nodes!;
    const syntheticNodes = nodes.filter((n) => {
      const k = (n as { kind?: { case?: string } }).kind;
      return k?.case === "syntheticSlotBinding";
    });
    expect(syntheticNodes).toHaveLength(1);
  });
});
