import { describe, it, expect } from "vitest";
import { typeDescriptorToSchema, typeDescriptorToString } from "./schema.js";
import {
  primitive,
  literal,
  union,
  intersection,
  array,
  tuple,
  object,
  func,
  ref,
  unknown,
} from "../type-ir.js";
import type { PropertyMetaSchema } from "./types.js";

describe("typeDescriptorToString", () => {
  it("converts primitives", () => {
    expect(typeDescriptorToString(primitive("string"))).toBe("string");
    expect(typeDescriptorToString(primitive("number"))).toBe("number");
    expect(typeDescriptorToString(primitive("boolean"))).toBe("boolean");
  });

  it("converts literals", () => {
    expect(typeDescriptorToString(literal("hello"))).toBe('"hello"');
    expect(typeDescriptorToString(literal(42))).toBe("42");
    expect(typeDescriptorToString(literal(true))).toBe("true");
  });

  it("converts unions", () => {
    expect(typeDescriptorToString(union([literal("a"), literal("b")]))).toBe('"a" | "b"');
  });

  it("converts intersections", () => {
    expect(typeDescriptorToString(intersection([primitive("string"), primitive("number")]))).toBe(
      "string & number",
    );
  });

  it("converts arrays", () => {
    expect(typeDescriptorToString(array(primitive("string")))).toBe("string[]");
  });

  it("converts tuples", () => {
    expect(typeDescriptorToString(tuple([primitive("string"), primitive("number")]))).toBe(
      "[string, number]",
    );
  });

  it("converts objects to 'object'", () => {
    expect(
      typeDescriptorToString(object([{ name: "x", type: primitive("string"), optional: false }])),
    ).toBe("object");
  });

  it("converts functions to 'function'", () => {
    expect(
      typeDescriptorToString(
        func([{ name: "x", type: primitive("string"), optional: false }], primitive("void")),
      ),
    ).toBe("function");
  });

  it("converts refs", () => {
    expect(typeDescriptorToString(ref("MyType"))).toBe("MyType");
    expect(typeDescriptorToString(ref("Map", [primitive("string"), primitive("number")]))).toBe(
      "Map<string, number>",
    );
  });

  it("converts unknown", () => {
    expect(typeDescriptorToString(unknown("SomeWeirdType"))).toBe("SomeWeirdType");
    expect(typeDescriptorToString(unknown(""))).toBe("unknown");
  });
});

describe("typeDescriptorToSchema", () => {
  it("converts primitives to strings", () => {
    expect(typeDescriptorToSchema(primitive("string"))).toBe("string");
    expect(typeDescriptorToSchema(primitive("number"))).toBe("number");
  });

  it("converts literals to quoted strings", () => {
    expect(typeDescriptorToSchema(literal("hello"))).toBe('"hello"');
    expect(typeDescriptorToSchema(literal(42))).toBe("42");
  });

  it("converts unions to enum schema", () => {
    const schema = typeDescriptorToSchema(union([literal("a"), literal("b")]));
    expect(schema).toEqual({
      kind: "enum",
      type: '"a" | "b"',
      schema: ['"a"', '"b"'],
    });
  });

  it("converts intersections to object schema", () => {
    const schema = typeDescriptorToSchema(intersection([primitive("string"), primitive("number")]));
    expect(schema).toEqual({
      kind: "object",
      type: "string & number",
      schema: ["string", "number"],
    });
  });

  it("converts arrays to array schema", () => {
    const schema = typeDescriptorToSchema(array(primitive("string")));
    expect(schema).toEqual({
      kind: "array",
      type: "string[]",
      schema: ["string"],
    });
  });

  it("converts tuples to array schema", () => {
    const schema = typeDescriptorToSchema(tuple([primitive("string"), primitive("number")]));
    expect(schema).toEqual({
      kind: "array",
      type: "[string, number]",
      schema: ["string", "number"],
    });
  });

  it("converts objects to object schema", () => {
    const schema = typeDescriptorToSchema(
      object([{ name: "x", type: primitive("string"), optional: false }]),
    );
    expect(schema).toEqual({
      kind: "object",
      type: "{ x: string }",
      schema: ["string"],
    });
  });

  it("converts functions to string (no schema)", () => {
    const schema = typeDescriptorToSchema(
      func([{ name: "x", type: primitive("string"), optional: false }], primitive("void")),
    );
    expect(typeof schema).toBe("string");
    expect(schema).toBe("(x: string) => void");
  });

  it("converts refs to string", () => {
    expect(typeDescriptorToSchema(ref("MyType"))).toBe("MyType");
    expect(typeDescriptorToSchema(ref("Map", [primitive("string"), primitive("number")]))).toBe(
      "Map<string, number>",
    );
  });

  it("converts unknown to rawType string", () => {
    expect(typeDescriptorToSchema(unknown("SomeType"))).toBe("SomeType");
  });

  it("returns 'unknown' when schema: false", () => {
    expect(typeDescriptorToSchema(primitive("string"), { schema: false })).toBe("unknown");
  });

  it("respects schema.ignore filter", () => {
    const schema = typeDescriptorToSchema(union([literal("a"), literal("b")]), {
      schema: { ignore: (type) => type.includes("|") },
    });
    // When ignored, returns the type as a string instead of schema object
    expect(typeof schema).toBe("string");
    expect(schema).toBe('"a" | "b"');
  });

  it("does not produce schema objects for ignored nested types", () => {
    const schema = typeDescriptorToSchema(array(primitive("string")), {
      schema: { ignore: () => true },
    });
    expect(typeof schema).toBe("string");
  });
});
