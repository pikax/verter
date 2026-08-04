/**
 * Discriminating regression tests for the operator-splitter behaviour.
 *
 * The legacy compat layer split top-level type operators by walking
 * `prop.rawType` text. These tests would FAIL against that legacy tree
 * because they construct `PropMeta` inputs where `prop.type` (TypeDescriptor)
 * carries the structural truth and `prop.rawType` is a deliberate decoy.
 *
 * The splitter callers consume `prop.type` (TypeDescriptor)
 * directly via `unionArms` / `intersectionArms` / `stripUndefinedArm` /
 * structural utility-type matches. The expected output below reflects that
 * descriptor-driven behaviour.
 */
import { describe, it, expect } from "vitest";
import { mapPropMeta, mapEventMeta } from "./checker.js";
import {
  array,
  primitive,
  ref,
  union,
  intersection,
  literal,
  func,
  object,
  typeParameter,
} from "@verter/type-ir";
import type { TypeDescriptor } from "@verter/type-ir";
import type { PropMeta } from "../types.js";

function makeProp(overrides: Partial<PropMeta> & { type: TypeDescriptor }): PropMeta {
  return {
    name: overrides.name ?? "value",
    type: overrides.type,
    required: overrides.required ?? false,
    hasDefault: false,
    rawType: overrides.rawType,
    tags: overrides.tags ?? [],
    description: overrides.description,
  };
}

describe("operator splitters consume TypeDescriptor (not prop.rawType)", () => {
  describe("union arm extraction (replaces splitTopLevelTypeUnion / splitTopLevelTypeOperator '|')", () => {
    it("buildCompatAnyPropMeta: detects 'any' arm from prop.type, not from prop.rawType text", () => {
      // Decoy: rawType text claims a non-`any` shape, descriptor says union including any.
      const prop = makeProp({
        type: union([primitive("any"), primitive("undefined")]),
        rawType: "string | undefined",
        required: false,
      });
      const result = mapPropMeta(prop);
      // Descriptor wins → `any` projection.
      expect(result.type).toBe("any");
    });

    it("buildCompatAnyPropMeta: multi-arm union with 'any' text but no descriptor 'any' arm declines projection", () => {
      // Decoy: rawType text contains a multi-arm union including "any" as one
      // arm. The legacy splitter-on-text would have found the "any" arm
      // (`splitTopLevelTypeUnion("string | any").some(part => part.trim() === "any")`
      // = true) and projected the `any` shape. The arm extraction is
      // structural on `prop.type` — which has no `primitive("any")` arm — so
      // the projection declines.
      const prop = makeProp({
        type: primitive("string"),
        rawType: "string | any",
        required: false,
      });
      const result = mapPropMeta(prop);
      expect(result.type).not.toBe("any");
    });

    it("buildCompatHtmlButtonTypePropMeta: extracts button-type union arms from prop.type, not from rawType text", () => {
      // Descriptor: `"button" | "submit" | "reset" | undefined` (structurally).
      const prop = makeProp({
        name: "type",
        type: union([
          literal("button"),
          literal("submit"),
          literal("reset"),
          primitive("undefined"),
        ]),
        // Decoy rawType: bogus shape that the legacy splitter would not match.
        rawType: "bogus | shape",
        required: false,
      });
      const result = mapPropMeta(prop);
      // Descriptor structural match wins.
      expect(result.type).toBe('"button" | "submit" | "reset" | undefined');
    });

    it("classifies structurally equivalent function and array arms without comparing rendered parameter names", () => {
      const functionArm = func(
        [{ name: "event", type: ref("MouseEvent"), optional: false }],
        primitive("void"),
      );
      const arrayElement = func(
        [{ name: "value", type: ref("MouseEvent"), optional: false }],
        primitive("void"),
      );
      const result = mapPropMeta(
        makeProp({
          name: "onClick",
          type: union([functionArm, array(arrayElement)]),
          rawType: "HOSTILE_DISPLAY",
          required: true,
        }),
      );

      expect(result.schema).toEqual({
        kind: "enum",
        type: "((event: MouseEvent) => void) | ((value: MouseEvent) => void)[]",
        schema: [
          {
            kind: "array",
            type: "((value: MouseEvent) => void)[]",
            schema: [{ kind: "event", type: "(value: MouseEvent): void", schema: [] }],
          },
          { kind: "event", type: "(event: MouseEvent): void", schema: [] },
        ],
      });
    });

    // @ai-generated - Rejects generic function refs whose instantiated arguments differ.
    it("declines function-array projection when generic alias arguments differ", () => {
      const genericFn = func(
        [{ name: "value", type: typeParameter("T"), optional: false }],
        primitive("void"),
        { typeParameters: [typeParameter("T")] },
      );
      const result = mapPropMeta(
        makeProp({
          name: "handler",
          type: union([ref("Fn", [primitive("string")]), array(ref("Fn", [primitive("number")]))]),
          rawType: "Fn<string> | Fn<number>[]",
          required: true,
        }),
        undefined,
        new Map([["Fn", genericFn]]),
      );

      expect(result.type).toBe("Fn<string> | Fn<number>[]");
      expect(JSON.stringify(result.schema)).not.toContain('"kind":"event"');
    });

    // @ai-generated - Requires Numberish classification to consume the complete descriptor.
    it("declines Numberish projection when an unrelated union arm is present", () => {
      const result = mapPropMeta(
        makeProp({
          type: union([ref("Numberish"), ref("Date")]),
          rawType: "Numberish | Date",
          required: true,
        }),
      );

      expect(result.type).toBe("Numberish | Date");
      expect(result.schema).toMatchObject({
        kind: "enum",
        type: "Numberish | Date",
      });
    });

    // @ai-generated - Requires referrer-policy classification to consume the complete descriptor.
    it("declines referrer-policy projection when an unrelated union arm is present", () => {
      const result = mapPropMeta(
        makeProp({
          name: "referrerpolicy",
          type: union([ref("HTMLAttributeReferrerPolicy"), ref("URL")]),
          rawType: "HTMLAttributeReferrerPolicy | URL",
          required: true,
        }),
      );

      expect(result.type).toBe("HTMLAttributeReferrerPolicy | URL");
      expect(JSON.stringify(result.schema)).not.toContain('"no-referrer"');
    });

    // @ai-generated - Requires prefetch classification to reject extra descriptor arms.
    it("declines prefetch projection when an extra literal arm is present", () => {
      const partialPair = ref("Partial", [
        object([
          { name: "visibility", type: primitive("boolean"), optional: false },
          { name: "interaction", type: primitive("boolean"), optional: false },
        ]),
      ]);
      const result = mapPropMeta(
        makeProp({
          name: "prefetchOn",
          type: union([
            literal("visibility"),
            literal("interaction"),
            partialPair,
            literal("idle"),
          ]),
          rawType:
            '"visibility" | "interaction" | Partial<{ visibility: boolean; interaction: boolean; }> | "idle"',
          required: true,
        }),
      );

      expect(result.type).toContain('"idle"');
      expect(result.schema).toMatchObject({
        kind: "enum",
        type: expect.stringContaining('"idle"'),
      });
    });

    it("does not resurrect an array schema arm from union-looking terminal display", () => {
      const functionOnly = func(
        [{ name: "event", type: ref("MouseEvent"), optional: false }],
        primitive("void"),
      );
      const result = mapPropMeta(
        makeProp({
          name: "onClick",
          type: functionOnly,
          rawType: "((event: MouseEvent) => void) | Array<((event: MouseEvent) => void)>",
          required: false,
        }),
      );

      expect(result.type).toContain("Array<");
      expect(result.schema).toEqual({
        kind: "enum",
        type: "((event: MouseEvent) => void) | Array<((event: MouseEvent) => void)> | undefined",
        schema: ["(event: MouseEvent) => void", "undefined"],
      });
      expect(JSON.stringify(result.schema)).not.toContain('"kind":"array"');
    });
  });

  describe("intersection arm extraction (replaces splitTopLevelTypeIntersection / splitTopLevelTypeOperator '&')", () => {
    it("buildCompatStringBrandUnionPropMeta: extracts arms from descriptor structure, with arm count matching descriptor not rawType", () => {
      // Descriptor: union with 2 literal arms + a `string & {}` branded arm
      // (the structural gate for the function) + `undefined`.
      // The legacy splitter would have parsed `rawType` text and produced
      // its arm count from text. The structural gate is descriptor-
      // based: the branded `intersection(string, {})` arm triggers projection
      // AND the arm count is derived from `prop.type`.
      const brandedArm: TypeDescriptor = {
        kind: "intersection",
        types: [primitive("string"), { kind: "object", properties: [] }],
      };
      const prop = makeProp({
        name: "rel",
        type: union([
          literal("noopener"),
          literal("noreferrer"),
          brandedArm,
          primitive("undefined"),
        ]),
        // Decoy rawType with a different arm set than the descriptor.
        rawType: '"a" | "b" | "c" | "d" | (string & {}) | undefined',
        required: false,
      });
      const result = mapPropMeta(prop);
      // The arm set in the rendered type comes from prop.type
      // (3 non-undefined arms), NOT from the 5-arm rawType.
      expect(result.type).toContain('"noopener"');
      expect(result.type).toContain('"noreferrer"');
      expect(result.type).not.toContain('"a"');
      expect(result.type).not.toContain('"d"');
    });

    // @ai-generated - Rejects a branded intersection when rendering would erase a constraint.
    it("declines string-brand projection when the branded intersection has an extra constraint", () => {
      const result = mapPropMeta(
        makeProp({
          name: "target",
          type: union([
            literal("_self"),
            intersection([primitive("string"), object([]), ref("Constraint")]),
          ]),
          rawType: '"_self" | (string & {} & Constraint)',
          required: true,
        }),
      );

      expect(result.type).toContain("Constraint");
      expect(result.schema).toMatchObject({
        kind: "enum",
        type: expect.stringContaining("Constraint"),
      });
    });
  });

  describe("undefined stripping (replaces stripTopLevelUndefinedFromTypeString)", () => {
    it("Numberish gate declines when descriptor is not Numberish, even if rawType text says so", () => {
      // Descriptor is structurally `string | undefined` (NOT Numberish). The legacy
      // function would have stripped `prop.rawType` text via the deleted splitter, found
      // the stripped form equal to "Numberish", and constructed a Numberish compat result.
      // The Numberish projection gate reads the descriptor's typed kind
      // (no Booleanish/Numberish ref arm) and DECLINES the Numberish projection — the
      // schema does NOT carry the Numberish enum entries.
      const prop = makeProp({
        type: union([primitive("string"), primitive("undefined")]),
        rawType: "Numberish | undefined",
        required: false,
      });
      const result = mapPropMeta(prop);
      // The schema is NOT the Numberish enum projection — the
      // Numberish-specific schema entries (`"\"true\""`, `"true"`, etc.) are
      // absent, demonstrating the gate declined irrespective of the rawType
      // display passthrough.
      expect(JSON.stringify(result.schema)).not.toContain('"true"');
      expect(JSON.stringify(result.schema)).not.toContain('"1"');
    });
  });

  describe("function payload (replaces splitTopLevelCommaList over rawSignature text)", () => {
    it("derives emit payload tuple from event.payload function descriptor, not from rawSignature text", () => {
      // We import mapEventMeta locally to keep the surface narrow.
      // The function descriptor: (event: "click", id: number) => void.
      const payload: TypeDescriptor = func(
        [
          { name: "event", type: literal("click"), optional: false },
          { name: "id", type: primitive("number"), optional: false },
        ],
        primitive("void"),
      );
      const event = {
        name: "click",
        payload,
        // Decoy rawSignature: malformed string the legacy splitter could not parse;
        // the walker reads structurally from `event.payload`.
        rawSignature: "definitely not a parseable signature",
        hasValidator: false,
        isDeclared: true,
      };
      const result = mapEventMeta(event);
      // Structural walk drops the leading event-name literal arm
      // and reconstructs the tuple from remaining parameter descriptors.
      expect(result.type).toBe("[number]");
    });

    it.each(["void", "undefined", "never"] as const)(
      "treats primitive %s as a structural empty event payload",
      (name) => {
        const result = mapEventMeta({
          name: "close",
          payload: primitive(name),
          rawSignature: "DECOY_SIGNATURE",
          hasValidator: false,
          isDeclared: true,
        });

        expect(result.type).toBe("[]");
        expect(result.schema).toEqual([]);
      },
    );

    it("does not treat unknown('void') diagnostic text as a void payload", () => {
      const result = mapEventMeta({
        name: "close",
        payload: { kind: "unknown", rawType: "void" },
        rawSignature: "[]",
        hasValidator: false,
        isDeclared: true,
      });

      expect(result.type).toBe("[unknown]");
      expect(result.schema).toEqual(["unknown"]);
    });
  });
});
