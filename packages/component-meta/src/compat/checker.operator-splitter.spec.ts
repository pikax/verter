/**
 * Discriminating regression tests for the W7.1 operator-splitter migration.
 *
 * Before W7.1 the compat layer split top-level type operators by walking
 * `prop.rawType` text. These tests would FAIL against that pre-W7.1 tree
 * because they construct `PropMeta` inputs where `prop.type` (TypeDescriptor)
 * carries the structural truth and `prop.rawType` is a deliberate decoy.
 *
 * After W7.1 the splitter callers consume `prop.type` (TypeDescriptor)
 * directly via `unionArms` / `intersectionArms` / `stripUndefinedArm` /
 * structural utility-type matches. The expected output below reflects that
 * descriptor-driven behaviour.
 */
import { describe, it, expect } from "vitest";
import { mapPropMeta, mapEventMeta } from "./checker.js";
import { primitive, ref, union, intersection, literal, func } from "@verter/type-ir";
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

describe("W7.1: operator splitters consume TypeDescriptor (not prop.rawType)", () => {
  describe("union arm extraction (replaces splitTopLevelTypeUnion / splitTopLevelTypeOperator '|')", () => {
    it("buildCompatAnyPropMeta: detects 'any' arm from prop.type, not from prop.rawType text", () => {
      // Decoy: rawType text claims a non-`any` shape, descriptor says union including any.
      const prop = makeProp({
        type: union([primitive("any"), primitive("undefined")]),
        rawType: "string | undefined",
        required: false,
      });
      const result = mapPropMeta(prop);
      // Post-cutover: descriptor wins → `any` projection.
      expect(result.type).toBe("any");
    });

    it("buildCompatAnyPropMeta: multi-arm union with 'any' text but no descriptor 'any' arm declines projection", () => {
      // Decoy: rawType text contains a multi-arm union including "any" as one
      // arm. Pre-W7.1 the splitter-on-text would have found the "any" arm
      // (`splitTopLevelTypeUnion("string | any").some(part => part.trim() === "any")`
      // = true) and projected the `any` shape. Post-W7.1 the arm extraction is
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
        // Decoy rawType: bogus shape that pre-W7.1 splitter would not match.
        rawType: "bogus | shape",
        required: false,
      });
      const result = mapPropMeta(prop);
      // Post-cutover: descriptor structural match wins.
      expect(result.type).toBe('"button" | "submit" | "reset" | undefined');
    });
  });

  describe("intersection arm extraction (replaces splitTopLevelTypeIntersection / splitTopLevelTypeOperator '&')", () => {
    it("buildCompatStringBrandUnionPropMeta: extracts arms from descriptor structure, with arm count matching descriptor not rawType", () => {
      // Decoy descriptor: 3 distinct union arms structurally (no brand arm rendered as `string & {}`
      // in this case — we use `ref("UnknownBrand")` to keep the rendering simple). The pre-W7.1
      // splitter would have parsed `rawType` text and produced its arm count from text. Post-W7.1
      // the arm count is derived structurally from `prop.type`.
      const prop = makeProp({
        name: "rel",
        type: union([literal("noopener"), literal("noreferrer"), primitive("undefined")]),
        // Decoy rawType with `(string & {})` to satisfy the function's gate, but containing
        // a different arm count than the descriptor.
        rawType: '"a" | "b" | "c" | "d" | (string & {}) | undefined',
        required: false,
      });
      const result = mapPropMeta(prop);
      // Post-cutover: the arm set in the rendered type comes from prop.type
      // (3 arms minus undefined), NOT from the 5-arm rawType.
      expect(result.type).toContain('"noopener"');
      expect(result.type).toContain('"noreferrer"');
      expect(result.type).not.toContain('"a"');
      expect(result.type).not.toContain('"d"');
    });
  });

  describe("undefined stripping (replaces stripTopLevelUndefinedFromTypeString)", () => {
    it("Numberish gate declines when descriptor is not Numberish, even if rawType text says so", () => {
      // Descriptor is structurally `string | undefined` (NOT Numberish). Pre-W7.1 the
      // function would have stripped `prop.rawType` text via the deleted splitter, found
      // the stripped form equal to "Numberish", and constructed a Numberish compat result.
      // Post-W7.1 `stripTopLevelUndefinedFromCompatType` walks the descriptor structurally,
      // produces "string", and the Numberish gate declines.
      const prop = makeProp({
        type: union([primitive("string"), primitive("undefined")]),
        rawType: "Numberish | undefined",
        required: false,
      });
      const result = mapPropMeta(prop);
      // Post-cutover: descriptor wins → not a Numberish projection.
      expect(result.type).not.toBe("Numberish | undefined");
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
        // Decoy rawSignature: malformed string the pre-W7.1 splitter could not parse;
        // the post-W7.1 walker reads structurally from `event.payload`.
        rawSignature: "definitely not a parseable signature",
        hasValidator: false,
        isDeclared: true,
      };
      const result = mapEventMeta(event);
      // Post-cutover: structural walk drops the leading event-name literal arm
      // and reconstructs the tuple from remaining parameter descriptors.
      expect(result.type).toBe("[number]");
    });
  });
});
