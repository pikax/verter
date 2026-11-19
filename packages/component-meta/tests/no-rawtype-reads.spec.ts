import { describe, it, expect } from "vitest";
import { mapPropMeta, mapSlotMeta } from "../src/compat/checker.js";
import { primitive, ref, object, union } from "@verter/type-ir";
import type { TypeDescriptor } from "@verter/type-ir";
import type { PropMeta, SlotMeta } from "../src/types.js";

function makeProp(type: TypeDescriptor, rawType: string): PropMeta {
  return {
    name: "value",
    type,
    typeFact: { kind: "complete", value: type },
    rawType,
    required: true,
    hasDefault: false,
  };
}

describe("descriptor schema stays independent of decoy display text", () => {
  it.each([
    ["Numberish", "string"],
    ["HTMLAttributeReferrerPolicy", "string"],
    ['NuxtLinkProps["to"]', "number"],
    ['ButtonHTMLAttributes["type"]', "string"],
    ['"a" | "b" | (string & {})', "string"],
    [
      '"visibility" | "interaction" | Partial<{ visibility: boolean; interaction: boolean; }>',
      "string",
    ],
  ] as const)("keeps the primitive schema when display says %s", (display, name) => {
    expect(mapPropMeta(makeProp(primitive(name), display)).schema).toBe(name);
  });

  it("keeps a structural alias when display says Booleanish", () => {
    const result = mapPropMeta(makeProp(ref("CustomEnum"), "Booleanish"));
    expect(result.type).toBe("CustomEnum");
    expect(result.schema).toEqual({ kind: "object", type: "CustomEnum", schema: {} });
  });

  it("retains any-containing union arms despite decoy display text", () => {
    const result = mapPropMeta(
      makeProp(union([primitive("any"), primitive("undefined")]), "string | undefined"),
    );
    expect(result.type).toBe("string | undefined");
    expect(result.schema).toEqual({
      kind: "enum",
      type: "string | undefined",
      schema: ["any", "undefined"],
    });
  });

  it("uses the structural reference for bare-alias display", () => {
    expect(mapPropMeta(makeProp(ref("Foo"), "DECOY_DISPLAY")).type).toBe("Foo");
  });

  it("passes through display text for an expanded object without changing its schema", () => {
    const type = object([{ name: "id", type: primitive("number"), optional: false }]);
    const result = mapPropMeta(makeProp(type, 'Pick<FullUser, "id">'));
    expect(result.type).toBe('Pick<FullUser, "id">');
    expect(result.schema).toMatchObject({
      kind: "object",
      schema: { id: { type: "number", schema: "number" } },
    });
  });
});

describe("slot display follows binding descriptors", () => {
  it.each([
    [primitive("string"), undefined, "{ item: string; }"],
    [
      object([{ name: "id", type: primitive("number"), optional: false }]),
      undefined,
      "{ item: { id: number; }; }",
    ],
    [
      ref("Item"),
      new Map([["Item", object([{ name: "label", type: primitive("string"), optional: false }])]]),
      "{ item: { label: string; }; }",
    ],
  ] as const)("renders descriptor %j with a decoy binding display", (type, registry, expected) => {
    const slot: SlotMeta = {
      name: "default",
      isScoped: true,
      isRequired: true,
      bindings: [
        {
          name: "item",
          type,
          typeFact: { kind: "complete", value: type },
          rawType: "DECOY_DISPLAY",
        },
      ],
    };
    expect(mapSlotMeta(slot, undefined, registry).type).toBe(expected);
  });
});
