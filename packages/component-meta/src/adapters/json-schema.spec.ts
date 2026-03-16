import { describe, it, expect } from "vitest";
import { typeToJsonSchema, propsToJsonSchema } from "./json-schema.js";
import type { ComponentMeta } from "../types.js";

describe("typeToJsonSchema", () => {
  it("converts string primitive", () => {
    expect(typeToJsonSchema({ kind: "primitive", name: "string" })).toEqual({ type: "string" });
  });

  it("converts number primitive", () => {
    expect(typeToJsonSchema({ kind: "primitive", name: "number" })).toEqual({ type: "number" });
  });

  it("converts boolean primitive", () => {
    expect(typeToJsonSchema({ kind: "primitive", name: "boolean" })).toEqual({ type: "boolean" });
  });

  it("converts null primitive", () => {
    expect(typeToJsonSchema({ kind: "primitive", name: "null" })).toEqual({ type: "null" });
  });

  it("converts never to not:{}", () => {
    expect(typeToJsonSchema({ kind: "primitive", name: "never" })).toEqual({ not: {} });
  });

  it("converts literal to const", () => {
    expect(typeToJsonSchema({ kind: "literal", value: "primary" })).toEqual({ const: "primary" });
    expect(typeToJsonSchema({ kind: "literal", value: 42 })).toEqual({ const: 42 });
    expect(typeToJsonSchema({ kind: "literal", value: true })).toEqual({ const: true });
  });

  it("converts literal union to enum", () => {
    const schema = typeToJsonSchema({
      kind: "union",
      types: [
        { kind: "literal", value: "a" },
        { kind: "literal", value: "b" },
      ],
    });
    expect(schema).toEqual({ enum: ["a", "b"] });
    // Should not use anyOf for all-literal unions
    expect(schema).not.toHaveProperty("anyOf");
  });

  it("converts mixed union to anyOf", () => {
    const schema = typeToJsonSchema({
      kind: "union",
      types: [
        { kind: "primitive", name: "string" },
        { kind: "primitive", name: "number" },
      ],
    });
    expect(schema).toEqual({
      anyOf: [{ type: "string" }, { type: "number" }],
    });
  });

  it("converts nullable to anyOf with null", () => {
    const schema = typeToJsonSchema({
      kind: "union",
      types: [
        { kind: "primitive", name: "string" },
        { kind: "primitive", name: "null" },
      ],
    });
    expect(schema).toEqual({
      anyOf: [{ type: "string" }, { type: "null" }],
    });
  });

  it("converts array", () => {
    expect(
      typeToJsonSchema({
        kind: "array",
        element: { kind: "primitive", name: "string" },
      }),
    ).toEqual({
      type: "array",
      items: { type: "string" },
    });
  });

  it("converts tuple", () => {
    const schema = typeToJsonSchema({
      kind: "tuple",
      elements: [
        { kind: "primitive", name: "string" },
        { kind: "primitive", name: "number" },
      ],
    });
    expect(schema).toEqual({
      type: "array",
      items: [{ type: "string" }, { type: "number" }],
      minItems: 2,
      maxItems: 2,
    });
  });

  it("converts object", () => {
    const schema = typeToJsonSchema({
      kind: "object",
      properties: [
        { name: "name", type: { kind: "primitive", name: "string" }, optional: false },
        { name: "age", type: { kind: "primitive", name: "number" }, optional: true },
      ],
    });
    expect(schema).toEqual({
      type: "object",
      properties: {
        name: { type: "string" },
        age: { type: "number" },
      },
      required: ["name"],
    });
    // Optional props should not be in required
    expect(schema.required).not.toContain("age");
  });

  it("converts intersection to allOf", () => {
    const schema = typeToJsonSchema({
      kind: "intersection",
      types: [
        { kind: "ref", name: "A" },
        { kind: "ref", name: "B" },
      ],
    });
    expect(schema).toHaveProperty("allOf");
    expect(schema.allOf).toHaveLength(2);
  });
});

describe("propsToJsonSchema", () => {
  it("generates schema for component props", () => {
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

    const schema = propsToJsonSchema(meta);
    expect(schema.type).toBe("object");
    expect(schema.properties?.title).toEqual({ type: "string" });
    expect(schema.properties?.count).toEqual({ type: "number" });
    expect(schema.required).toEqual(["title"]);
    // Optional prop should not be required
    expect(schema.required).not.toContain("count");
  });
});
