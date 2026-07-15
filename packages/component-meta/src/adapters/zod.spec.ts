import { describe, it, expect } from "vitest";
import { typeToZodString, propsToZodString, typeToZodSchema, propsToZodSchema } from "./zod.js";
import type { ComponentMeta } from "../types.js";
import { z } from "zod";

describe("typeToZodString (codegen)", () => {
  it("converts primitives", () => {
    expect(typeToZodString({ kind: "primitive", name: "string" })).toBe("z.string()");
    expect(typeToZodString({ kind: "primitive", name: "number" })).toBe("z.number()");
    expect(typeToZodString({ kind: "primitive", name: "boolean" })).toBe("z.boolean()");
    expect(typeToZodString({ kind: "primitive", name: "null" })).toBe("z.null()");
    expect(typeToZodString({ kind: "primitive", name: "undefined" })).toBe("z.undefined()");
    expect(typeToZodString({ kind: "primitive", name: "void" })).toBe("z.void()");
    expect(typeToZodString({ kind: "primitive", name: "never" })).toBe("z.never()");
    expect(typeToZodString({ kind: "primitive", name: "any" })).toBe("z.any()");
    expect(typeToZodString({ kind: "primitive", name: "unknown" })).toBe("z.unknown()");
  });

  it("converts string literal", () => {
    expect(typeToZodString({ kind: "literal", value: "primary" })).toBe('z.literal("primary")');
  });

  it("converts number literal", () => {
    expect(typeToZodString({ kind: "literal", value: 42 })).toBe("z.literal(42)");
  });

  it("converts boolean literal", () => {
    expect(typeToZodString({ kind: "literal", value: true })).toBe("z.literal(true)");
  });

  it("converts union", () => {
    const result = typeToZodString({
      kind: "union",
      types: [
        { kind: "primitive", name: "string" },
        { kind: "primitive", name: "number" },
      ],
    });
    expect(result).toBe("z.union([z.string(), z.number()])");
    // Should not produce a plain z.string()
    expect(result).not.toBe("z.string()");
  });

  it("converts array", () => {
    expect(
      typeToZodString({
        kind: "array",
        element: { kind: "primitive", name: "string" },
      }),
    ).toBe("z.array(z.string())");
  });

  it("converts tuple", () => {
    expect(
      typeToZodString({
        kind: "tuple",
        elements: [
          { kind: "primitive", name: "string" },
          { kind: "primitive", name: "number" },
        ],
      }),
    ).toBe("z.tuple([z.string(), z.number()])");
  });

  it("converts object", () => {
    const result = typeToZodString({
      kind: "object",
      properties: [
        { name: "name", type: { kind: "primitive", name: "string" }, optional: false },
        { name: "age", type: { kind: "primitive", name: "number" }, optional: true },
      ],
    });
    expect(result).toContain('"name": z.string()');
    expect(result).toContain('"age": z.number().optional()');
  });

  it("converts index-signature objects to z.record()", () => {
    const result = typeToZodString({
      kind: "object",
      properties: [],
      indexSignatures: [
        {
          keyName: "key",
          keyType: { kind: "primitive", name: "string" },
          valueType: { kind: "primitive", name: "number" },
        },
      ],
    });
    expect(result).toBe("z.record(z.string(), z.number())");
    expect(result).not.toContain("z.object({})");
  });

  it("derives a z.number() key for number index signatures", () => {
    const result = typeToZodString({
      kind: "object",
      properties: [],
      indexSignatures: [
        {
          keyName: "key",
          keyType: { kind: "primitive", name: "number" },
          valueType: { kind: "primitive", name: "string" },
        },
      ],
    });
    expect(result).toBe("z.record(z.number(), z.string())");
    // The number key must NOT be widened to a string key.
    expect(result).not.toContain("z.string(), z.string()");
    expect(result).not.toContain("z.object({})");
  });

  it("converts function", () => {
    expect(
      typeToZodString({
        kind: "function",
        parameters: [],
        returnType: { kind: "primitive", name: "void" },
      }),
    ).toBe("z.function()");
  });

  it("converts unknown/ref to z.unknown()", () => {
    expect(typeToZodString({ kind: "unknown", rawType: "complex" })).toBe("z.unknown()");
    expect(typeToZodString({ kind: "ref", name: "MyType" })).toBe("z.unknown()");
  });
});

describe("propsToZodString", () => {
  it("generates z.object for props", () => {
    const meta: ComponentMeta = {
      filePath: "Comp.vue",
      componentName: "Comp",
      optionsApi: false,
      props: [
        {
          name: "title",
          type: { kind: "primitive", name: "string" },
          required: true,
          hasDefault: false,
        },
        {
          name: "count",
          type: { kind: "primitive", name: "number" },
          required: false,
          hasDefault: true,
        },
      ],
      events: [],
      slots: [],
      models: [],
      exposed: [],
      components: [],
      templateRefs: [],
      imports: [],
      bindings: [],
      vueApiCalls: [],
      styles: [],
      flags: {
        asyncSetup: false,
        hasReactiveState: false,
        hasComputed: false,
        hasWatchers: false,
        hasLifecycleHooks: false,
        hasProvide: false,
        hasInject: false,
        hasInheritAttrsFalse: false,
        hasStoreUsage: false,
      },
    };

    const result = propsToZodString(meta);
    expect(result).toContain("z.object(");
    expect(result).toContain('"title": z.string()');
    expect(result).toContain('"count": z.number().optional()');
    // required prop should not have .optional()
    expect(result).not.toContain('"title": z.string().optional()');
  });
});

describe("typeToZodSchema (runtime)", () => {
  it("creates string schema", () => {
    const schema = typeToZodSchema({ kind: "primitive", name: "string" }) as z.ZodString;
    expect(schema.parse("hello")).toBe("hello");
    expect(() => schema.parse(42)).toThrow();
  });

  it("creates number schema", () => {
    const schema = typeToZodSchema({ kind: "primitive", name: "number" }) as z.ZodNumber;
    expect(schema.parse(42)).toBe(42);
    expect(() => schema.parse("oops")).toThrow();
  });

  it("creates boolean schema", () => {
    const schema = typeToZodSchema({ kind: "primitive", name: "boolean" }) as z.ZodBoolean;
    expect(schema.parse(true)).toBe(true);
    expect(() => schema.parse("yes")).toThrow();
  });

  it("creates literal schema", () => {
    const schema = typeToZodSchema({
      kind: "literal",
      value: "primary",
    }) as z.ZodLiteral<"primary">;
    expect(schema.parse("primary")).toBe("primary");
    expect(() => schema.parse("secondary")).toThrow();
  });

  it("creates union schema", () => {
    const schema = typeToZodSchema({
      kind: "union",
      types: [
        { kind: "primitive", name: "string" },
        { kind: "primitive", name: "number" },
      ],
    }) as z.ZodUnion<any>;
    expect(schema.parse("hello")).toBe("hello");
    expect(schema.parse(42)).toBe(42);
    expect(() => schema.parse(true)).toThrow();
  });

  it("creates array schema", () => {
    const schema = typeToZodSchema({
      kind: "array",
      element: { kind: "primitive", name: "string" },
    }) as z.ZodArray<any>;
    expect(schema.parse(["a", "b"])).toEqual(["a", "b"]);
    expect(() => schema.parse([1, 2])).toThrow();
  });

  it("creates record schemas for string index signatures", () => {
    const schema = typeToZodSchema({
      kind: "object",
      properties: [],
      indexSignatures: [
        {
          keyName: "key",
          keyType: { kind: "primitive", name: "string" },
          valueType: { kind: "primitive", name: "number" },
        },
      ],
    }) as z.ZodRecord<any, any>;
    expect(schema.parse({ a: 1, b: 2 })).toEqual({ a: 1, b: 2 });
    expect(() => schema.parse({ a: "nope" })).toThrow();
  });

  it("creates a number-keyed record for number index signatures", () => {
    const schema = typeToZodSchema({
      kind: "object",
      properties: [],
      indexSignatures: [
        {
          keyName: "key",
          keyType: { kind: "primitive", name: "number" },
          valueType: { kind: "primitive", name: "string" },
        },
      ],
    }) as z.ZodRecord<any, any>;
    // zod v4: a z.number() record key accepts numeric-looking keys ...
    expect(schema.parse({ 1: "a", 2: "b" })).toEqual({ 1: "a", 2: "b" });
    // ... and rejects non-numeric keys (this is what z.string() would have allowed).
    expect(() => schema.parse({ foo: "a" })).toThrow();
    // The value schema still applies.
    expect(() => schema.parse({ 1: 99 })).toThrow();
  });
});

describe("propsToZodSchema (runtime)", () => {
  it("creates object schema for props", () => {
    const meta: ComponentMeta = {
      filePath: "Comp.vue",
      componentName: "Comp",
      optionsApi: false,
      props: [
        {
          name: "title",
          type: { kind: "primitive", name: "string" },
          required: true,
          hasDefault: false,
        },
        {
          name: "count",
          type: { kind: "primitive", name: "number" },
          required: false,
          hasDefault: true,
        },
      ],
      events: [],
      slots: [],
      models: [],
      exposed: [],
      components: [],
      templateRefs: [],
      imports: [],
      bindings: [],
      vueApiCalls: [],
      styles: [],
      flags: {
        asyncSetup: false,
        hasReactiveState: false,
        hasComputed: false,
        hasWatchers: false,
        hasLifecycleHooks: false,
        hasProvide: false,
        hasInject: false,
        hasInheritAttrsFalse: false,
        hasStoreUsage: false,
      },
    };

    const schema = propsToZodSchema(meta) as z.ZodObject<any>;
    expect(schema.parse({ title: "Hello", count: 5 })).toEqual({ title: "Hello", count: 5 });
    expect(schema.parse({ title: "Hello" })).toEqual({ title: "Hello" });
    expect(() => schema.parse({ count: 5 })).toThrow(); // missing required title
  });
});
