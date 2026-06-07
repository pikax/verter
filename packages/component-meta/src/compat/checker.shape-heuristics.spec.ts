/**
 * Discriminating regression tests for the shape-heuristic behaviour.
 *
 * The legacy compat layer's `looksLike*` helpers (`looksLikeBareTypeReference`,
 * `looksLikeIndexedAccessType`, `looksLikeSlotsHelperRawType`,
 * `looksLikeUiHelperRawType`, `looksLikeStringCompatibleType`) ran regex / substring
 * checks against `prop.rawType` text.
 *
 * Every helper now switches on `TypeDescriptor.kind`. These tests would
 * FAIL against the legacy tree because they construct `PropMeta` /
 * `SlotMeta.bindings` inputs where `prop.type` / `binding.type`
 * (`TypeDescriptor`) carries the structural truth and `prop.rawType` /
 * `binding.rawType` is a deliberate decoy.
 */
import { describe, it, expect } from "vitest";
import { mapPropMeta, mapSlotMeta } from "./checker.js";
import {
  primitive,
  ref,
  union,
  intersection,
  literal,
  indexedAccess,
  type TypeDescriptor,
} from "@verter/type-ir";
import type { PropMeta, SlotMeta } from "../types.js";

function makeProp(overrides: Partial<PropMeta> & { type: TypeDescriptor }): PropMeta {
  return {
    name: overrides.name ?? "value",
    type: overrides.type,
    required: overrides.required ?? false,
    hasDefault: overrides.hasDefault ?? false,
    rawType: overrides.rawType,
    tags: overrides.tags ?? [],
    description: overrides.description,
    default: overrides.default,
  };
}

function makeSlot(
  bindings: Array<{ name: string; type: TypeDescriptor; rawType?: string }>,
  overrides: Partial<SlotMeta> = {},
): SlotMeta {
  return {
    name: overrides.name ?? "default",
    isScoped: overrides.isScoped ?? true,
    bindings: bindings.map((b) => ({
      name: b.name,
      type: b.type,
      rawType: b.rawType,
    })),
    isRequired: overrides.isRequired,
    hasFallbackContent: overrides.hasFallbackContent,
    returnType: overrides.returnType,
    description: overrides.description,
    tags: overrides.tags,
  };
}

describe("shape-detection heuristics switch on TypeDescriptor.kind (not prop.rawType)", () => {
  describe("looksLikeSlotsHelperRawType (gate for buildCompatSlotsPropMeta)", () => {
    it("projects slots-helper shape when descriptor is a ComponentSlots ref, regardless of rawType decoy", () => {
      // Descriptor: `ComponentSlots<typeof Theme>` — the Vue helper ref kind
      // tag. `unwrapComponentSlotsDescriptor` understands this shape and
      // pulls the slot field names from its typeArgument's `.slots` object.
      const slotsBody: TypeDescriptor = {
        kind: "object",
        properties: [
          { name: "header", type: ref("ClassNameValue"), optional: true },
          { name: "footer", type: ref("ClassNameValue"), optional: true },
        ],
      };
      const themeShape: TypeDescriptor = {
        kind: "object",
        properties: [{ name: "slots", type: slotsBody, optional: false }],
      };
      const prop = makeProp({
        name: "slots",
        type: ref("ComponentSlots", [themeShape]),
        // Decoy rawType: the legacy regex (/\["slots"\]$/) would NOT match.
        rawType: "ButThisRawTypeDoesNotEndInSlots",
        required: false,
      });
      const result = mapPropMeta(prop);
      // Descriptor ComponentSlots ref structural marker
      // triggers the projection → slot names extracted via the unwrap helper.
      expect(result.type).toContain("header?: ClassNameValue");
      expect(result.type).toContain("footer?: ClassNameValue");
    });

    it('declines slots projection when descriptor is plain object (no slots-helper structural marker), even if rawType ends in ["slots"]', () => {
      // Descriptor: plain `{ items: string[] }` (no IndexedAccess/ComponentSlots marker).
      const prop = makeProp({
        name: "items",
        type: {
          kind: "object",
          properties: [
            {
              name: "items",
              type: { kind: "array", element: primitive("string") },
              optional: false,
            },
          ],
        },
        // Decoy rawType: ends in ["slots"] — the legacy regex matched and would
        // have entered the slots projection branch. The gate is now structural.
        rawType: 'MyType["slots"]',
        required: true,
      });
      const result = mapPropMeta(prop);
      // No IndexedAccess<_,"slots"> nor ComponentSlots ref →
      // declines slots projection. The resulting rendered type is NOT the
      // ClassNameValue-rewrite produced by buildCompatSlotsPropMeta.
      expect(result.type).not.toContain("items?: ClassNameValue");
    });
  });

  describe("looksLikeUiHelperRawType (gate for buildCompatUiBindingType in slot bindings)", () => {
    it("projects UI-helper binding shape when descriptor is a ComponentUI ref, regardless of rawType decoy", () => {
      // Slot binding descriptor: `ComponentUI<typeof Theme>` — the Vue helper
      // ref. The typed `looksLikeUiHelperRawType` matches the ref
      // structurally, and `extractCompatUiBindingFieldNames` extracts the slot
      // field names via `unwrapComponentUiDescriptor`.
      const slotsFunctionMap: TypeDescriptor = {
        kind: "object",
        properties: [
          {
            name: "wrapper",
            type: {
              kind: "function",
              parameters: [],
              returnType: primitive("string"),
            },
            optional: false,
          },
          {
            name: "label",
            type: {
              kind: "function",
              parameters: [],
              returnType: primitive("string"),
            },
            optional: false,
          },
        ],
      };
      const themeShape: TypeDescriptor = {
        kind: "object",
        properties: [{ name: "slots", type: slotsFunctionMap, optional: false }],
      };
      const slot = makeSlot([
        {
          name: "ui",
          type: ref("ComponentUI", [themeShape]),
          // Decoy rawType: the legacy regex (/\["ui"\]$/) would NOT match.
          rawType: "ButThisRawTypeDoesNotEndInUi",
        },
      ]);
      const result = mapSlotMeta(slot);
      // Descriptor ComponentUI ref marker triggers UI projection.
      expect(result.type).toContain("wrapper: (props?: Record<string, any> | undefined) => string");
      expect(result.type).toContain("label: (props?: Record<string, any> | undefined) => string");
    });

    it("declines UI-helper projection when descriptor lacks the IndexedAccess<…, 'ui'> marker, even if rawType ends in [\"ui\"]", () => {
      // Slot binding descriptor: plain string primitive — NOT a UI-helper shape.
      const slot = makeSlot([
        {
          name: "ui",
          type: primitive("string"),
          // Decoy rawType: ends in ["ui"] — the legacy regex matched.
          rawType: 'MyType["ui"]',
        },
      ]);
      const result = mapSlotMeta(slot);
      // Not a UI-helper shape → declines projection. The rendered
      // type is the slot-binding default (a function signature), NOT the
      // `{ ui: (...) => string }` UI-helper expansion.
      expect(result.type).not.toContain("(props?: Record<string, any> | undefined) => string");
    });
  });

  describe("looksLikeBareTypeReference (used by shouldPreferDescriptorForProp / shouldPreferRawAliasForExpandedDescriptor)", () => {
    it("declines descriptor-over-rawType swap when descriptor is primitive, even if rawType TEXT looks like a bare identifier", () => {
      // Descriptor: primitive("number") — NOT a Ref.
      const prop = makeProp({
        name: "value",
        type: primitive("number"),
        // Decoy rawType: bare identifier TEXT. The legacy regex
        // /^[A-Za-z_$][A-Za-z0-9_$]*$/ matched, `shouldPreferDescriptorForProp`
        // returned true (bare-ref text sniff), and the descriptor text
        // "number" replaced the raw alias "Color". The typed
        // `looksLikeBareTypeReference(descriptor)` returns false (descriptor
        // is primitive, not Ref), so the rawType passthrough wins.
        rawType: "Color",
        required: true,
      });
      const result = mapPropMeta(prop);
      // Descriptor is primitive (no bare-ref marker) → rawType
      // passthrough wins ("Color"). (The legacy path produced "number".)
      expect(result.type).toBe("Color");
    });

    it("triggers descriptor-over-rawType swap when descriptor IS a bare Ref, even if rawType text is neither bare nor indexed-access shape", () => {
      // Descriptor: a bare Ref(Foo) without typeArguments.
      const prop = makeProp({
        name: "value",
        type: ref("Foo"),
        // Decoy rawType: text shape that the legacy regex would NOT recognise
        // as bare-ref (parens) and NOT as indexed-access (no trailing brackets).
        // Both regex checks FAIL → shouldPreferDescriptorForProp returns false
        // → rawType passthrough wins. The typed
        // `looksLikeBareTypeReference(descriptor)` reads kind="ref" with no
        // typeArguments → returns true → descriptor text wins.
        rawType: "someText(notAType)",
        required: true,
      });
      const result = mapPropMeta(prop);
      // Descriptor structural bare-Ref → descriptor text wins.
      // (The legacy path would have kept the rawType.)
      expect(result.type).toBe("Foo");
    });
  });

  describe("looksLikeIndexedAccessType (used by shouldPreferDescriptorForProp / shouldPreferRawSchemaType)", () => {
    it("declines descriptor-over-rawType swap when descriptor is primitive (no indexed-access marker), even if rawType TEXT has bracket syntax", () => {
      // Descriptor: union of `string | undefined` — primitives, NOT indexed access.
      const prop = makeProp({
        name: "value",
        type: union([primitive("string"), primitive("undefined")]),
        // Decoy rawType: bracket-access TEXT shape. The legacy regex
        // /\[[^\]]+\]$/ matched this and `shouldPreferDescriptorForProp`
        // returned true (text-based indexed-access check) → swapping the raw
        // alias for the resolved descriptor text. The typed
        // `looksLikeIndexedAccessType(descriptor)` returns false (descriptor
        // is primitive, not IndexedAccess), so the rawType passthrough wins.
        rawType: "Foo['bar']",
        required: false,
      });
      const result = mapPropMeta(prop);
      // rawType passthrough wins → `Foo["bar"]` (normalised
      // single-to-double-quote rendering). (The legacy path produced
      // "string | undefined" via the text-based shape sniff.)
      expect(result.type).toBe('Foo["bar"]');
    });

    it("triggers descriptor-over-rawType swap when descriptor is IndexedAccess, regardless of rawType text", () => {
      // Descriptor: `Foo['bar']` structurally.
      const prop = makeProp({
        name: "value",
        type: indexedAccess(ref("Foo"), literal("bar")),
        // Decoy rawType: bare alias TEXT. The legacy regex matched bare ref,
        // and shouldPreferDescriptorForProp returned true via the bare-ref text
        // sniff → descriptor swap. The typed
        // `looksLikeIndexedAccessType(descriptor)` returns true (descriptor IS
        // IndexedAccess), so the swap still happens — but driven by the
        // descriptor structure, not by parsing the rawType text.
        rawType: "PlainAlias",
        required: false,
      });
      const result = mapPropMeta(prop);
      // Descriptor structural match → swap to descriptor text.
      expect(result.type).toBe('Foo["bar"] | undefined');
    });
  });

  describe("looksLikeStringCompatibleType (used by normalizeDefaultForCompat)", () => {
    it("JSON.stringify-wraps default value when descriptor accepts string, regardless of rawType decoy", () => {
      // Descriptor: union of string literals + undefined — accepts strings.
      const prop = makeProp({
        name: "color",
        type: union([literal("red"), literal("green"), literal("blue"), primitive("undefined")]),
        // Decoy rawType: the legacy regex looked for `"`/`string` substring —
        // we set rawType to something that DOES contain those (preserving the
        // legacy default behaviour), but the structural decision must NOT
        // depend on rawType.
        rawType: "MyColorType",
        required: false,
        default: "red",
      });
      const result = mapPropMeta(prop);
      // Descriptor walk identifies string-literal arms →
      // string-compatible → default value `red` JSON.stringify-wrapped to `"red"`.
      expect(result.default).toBe('"red"');
    });

    it("declines string-stringify when descriptor is purely numeric, even if rawType text contains 'string'", () => {
      // Descriptor: pure number primitive — NOT string-compatible.
      const prop = makeProp({
        name: "count",
        type: primitive("number"),
        // Decoy rawType: contains the substring 'string'. The legacy
        // text-based helper would have returned true and wrapped a non-numeric
        // default. The descriptor walk returns false.
        rawType: "Stringish",
        required: false,
        default: "foo",
      });
      const result = mapPropMeta(prop);
      // Descriptor identifies number-only → string-stringify
      // declines → default value `foo` passes through unmodified.
      expect(result.default).toBe("foo");
    });

    it("recognises (string & {}) brand-intersection arm as string-compatible structurally", () => {
      // Descriptor: union including `string & {}` brand intersection.
      const prop = makeProp({
        name: "value",
        type: union([
          literal("a"),
          literal("b"),
          intersection([primitive("string"), { kind: "object", properties: [] }]),
        ]),
        // Decoy rawType: text that does NOT contain '(string & {})' literally.
        rawType: "MyBrand",
        required: true,
        default: "custom",
      });
      const result = mapPropMeta(prop);
      // Descriptor identifies string-literal arm → string-compatible
      // → default wrapped as JSON string.
      expect(result.default).toBe('"custom"');
    });
  });
});
